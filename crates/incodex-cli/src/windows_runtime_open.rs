use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use incodex_core::windows_path::require_local_disk_absolute;
use incodex_core::windows_session::{
    burn_windows_session, copy_windows_settings, create_windows_session,
    sweep_orphan_windows_sessions, WindowsCleanupResult, WindowsSessionHome,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, ERROR_PIPE_CONNECTED, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Cryptography::{
    BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};
use windows_sys::Win32::Storage::FileSystem::{
    ReadFile, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND,
};
use windows_sys::Win32::System::Pipes::{
    CallNamedPipeW, ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId,
    PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex};

use crate::cdp::OFFICIAL_NEW_CODEX_URL;
use crate::windows_activation::{
    activate_packaged_with_installed_runtime, WindowsActivationRequest,
    WindowsInstalledRuntimeRegistration,
};
use crate::windows_app::{discover_codex_package, WindowsCodexApp};
use crate::windows_cleanup::cleanup_windows_session_after_shutdown;
use crate::windows_install_state::{
    acquire_windows_install_state, read_windows_install_state, WindowsInstallPhase,
    WindowsInstallState, WindowsInstallStateGuard,
};
use crate::windows_process::WindowsProcessTree;

pub use crate::windows_runtime_lifecycle::{
    windows_runtime_ready_for_handshake, windows_runtime_shutdown_authorized,
    windows_runtime_startup_action, WindowsRuntimeStartupAction,
};

const RUNTIME_OPEN_MODE: &str = "__incodex_windows_runtime_open";
const READY_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(25);
const READY_MESSAGE_LIMIT: usize = 64;
const RUNTIME_OWNER_MUTEX: &str = "Local\\Incodex-OpenAI.Codex-Runtime-Owner";
const RUNTIME_RAISE_PIPE: &str = r"\\.\pipe\Incodex-Runtime-Raise";
const RAISE_TIMEOUT_MS: u32 = 3_000;

pub enum WindowsRuntimeOwnerClaim {
    Owned(WindowsRuntimeOwner),
    Existing,
}

pub struct WindowsRuntimeOwner {
    handle: HANDLE,
}

impl WindowsRuntimeOwnerClaim {
    pub fn acquire() -> Result<Self, String> {
        let name = RUNTIME_OWNER_MUTEX
            .encode_utf16()
            .chain([0])
            .collect::<Vec<_>>();
        let handle = unsafe { CreateMutexW(ptr::null(), 1, name.as_ptr()) };
        if handle.is_null() {
            return Err(format!(
                "cannot create Windows Runtime owner lock: {}",
                std::io::Error::last_os_error()
            ));
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            Ok(Self::Existing)
        } else {
            Ok(Self::Owned(WindowsRuntimeOwner { handle }))
        }
    }
}

impl Drop for WindowsRuntimeOwner {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.handle);
            CloseHandle(self.handle);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsRuntimeAcceptance {
    pub process_id: u32,
    pub message: String,
}

pub struct WindowsRuntimeReadyPipe {
    handle: HANDLE,
    name: String,
}

unsafe impl Send for WindowsRuntimeReadyPipe {}

impl WindowsRuntimeReadyPipe {
    pub fn create() -> Result<Self, String> {
        Self::create_for("Ready")
    }

    pub fn create_close() -> Result<Self, String> {
        Self::create_for("Closed")
    }

    fn create_for(kind: &str) -> Result<Self, String> {
        let mut random = [0u8; 16];
        let status = unsafe {
            BCryptGenRandom(
                ptr::null_mut(),
                random.as_mut_ptr(),
                random.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status != 0 {
            return Err(format!(
                "cannot generate Windows Runtime ready pipe name: NTSTATUS 0x{:08X}",
                status as u32
            ));
        }
        let nonce = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let name = format!(r"\\.\pipe\Incodex-Runtime-{kind}-{nonce}");
        let wide = name.encode_utf16().chain([0]).collect::<Vec<_>>();
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                0,
                READY_MESSAGE_LIMIT as u32,
                0,
                ptr::null(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(format!(
                "cannot create Windows Runtime ready pipe: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self { handle, name })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn accept(self) -> Result<WindowsRuntimeAcceptance, String> {
        let connected = unsafe { ConnectNamedPipe(self.handle, ptr::null_mut()) };
        if connected == 0
            && std::io::Error::last_os_error().raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32)
        {
            return Err(format!(
                "cannot accept Windows Runtime readiness: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut process_id = 0;
        if unsafe { GetNamedPipeClientProcessId(self.handle, &mut process_id) } == 0 {
            return Err(format!(
                "cannot identify Windows Runtime ready writer: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut body = [0u8; READY_MESSAGE_LIMIT];
        let mut read = 0;
        if unsafe {
            ReadFile(
                self.handle,
                body.as_mut_ptr(),
                body.len() as u32,
                &mut read,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(format!(
                "cannot read Windows Runtime readiness: {}",
                std::io::Error::last_os_error()
            ));
        }
        let message = std::str::from_utf8(&body[..read as usize])
            .map_err(|_| "Windows Runtime readiness is not UTF-8".to_string())?
            .trim()
            .to_string();
        Ok(WindowsRuntimeAcceptance {
            process_id,
            message,
        })
    }
}

impl Drop for WindowsRuntimeReadyPipe {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsRuntimeOpenRequest {
    pub source_home: PathBuf,
    pub source_bounds: Option<String>,
}

#[derive(Debug)]
pub struct WindowsRuntimeOpenPlan {
    pub bin: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, PathBuf>,
    pub env_flags: BTreeMap<String, String>,
    pub session: WindowsSessionHome,
    package_full_name: String,
    app_user_model_id: String,
    base_environment: BTreeMap<String, OsString>,
}

impl WindowsRuntimeOpenPlan {
    pub fn activation_request(&self) -> Result<WindowsActivationRequest, String> {
        let mut environment = self.base_environment.clone();
        environment.extend(
            self.env
                .iter()
                .map(|(key, value)| (key.clone(), value.as_os_str().to_os_string())),
        );
        environment.extend(
            self.env_flags
                .iter()
                .map(|(key, value)| (key.clone(), OsString::from(value))),
        );
        WindowsActivationRequest::new(
            &self.package_full_name,
            &self.app_user_model_id,
            self.args.iter().map(OsString::from),
            environment,
        )
    }
}

pub fn parse_windows_runtime_open(
    arguments: &[String],
) -> Option<Result<WindowsRuntimeOpenRequest, String>> {
    if arguments.first().map(String::as_str) != Some(RUNTIME_OPEN_MODE) {
        return None;
    }
    Some((|| {
        let source_home = PathBuf::from(flag_value(arguments, "--source-home")?);
        require_local_disk_absolute(&source_home, "Windows Runtime source home")?;
        let raw_bounds = flag_value(arguments, "--source-bounds")?;
        let source_bounds = if raw_bounds.is_empty() {
            None
        } else {
            validate_source_bounds(raw_bounds)?;
            Some(raw_bounds.to_string())
        };
        Ok(WindowsRuntimeOpenRequest {
            source_home,
            source_bounds,
        })
    })())
}

pub fn try_run_windows_runtime_open(arguments: &[String]) -> Option<Result<(), String>> {
    let request = match parse_windows_runtime_open(arguments)? {
        Ok(request) => request,
        Err(error) => return Some(Err(error)),
    };
    Some(run_windows_runtime_open(request))
}

pub fn prepare_windows_runtime_open(
    app: &WindowsCodexApp,
    user_root: &Path,
    source_home: &Path,
    source_bounds: Option<&str>,
) -> Result<WindowsRuntimeOpenPlan, String> {
    if let Some(bounds) = source_bounds {
        validate_source_bounds(bounds)?;
    }
    let _ = sweep_orphan_windows_sessions(user_root);
    let session = create_windows_session(user_root)?;
    let prepared = (|| {
        copy_windows_settings(&session, source_home)?;
        let env = BTreeMap::from([
            ("CODEX_HOME".to_string(), session.home.clone()),
            (
                "CODEX_ELECTRON_USER_DATA_PATH".to_string(),
                session.chromium.clone(),
            ),
            ("INCODEX_SESSION_ROOT".to_string(), session.root.clone()),
            ("INCODEX_SOURCE_HOME".to_string(), source_home.to_path_buf()),
        ]);
        let env_flags = BTreeMap::from([
            ("INCODEX_INCOGNITO".to_string(), "1".to_string()),
            ("INCODEX_CLEANUP_OWNER".to_string(), "native".to_string()),
            (
                "INCODEX_WINDOWS_RAISE_PIPE".to_string(),
                RUNTIME_RAISE_PIPE.to_string(),
            ),
            ("INCODEX_SESSION_ID".to_string(), session.session_id.clone()),
            (
                "INCODEX_SOURCE_BOUNDS".to_string(),
                source_bounds.unwrap_or_default().to_string(),
            ),
        ]);
        Ok(WindowsRuntimeOpenPlan {
            bin: app.executable.clone(),
            args: vec![
                format!("--user-data-dir={}", session.chromium.display()),
                OFFICIAL_NEW_CODEX_URL.to_string(),
            ],
            env,
            env_flags,
            session: session.clone(),
            package_full_name: app.package_full_name.clone(),
            app_user_model_id: app.app_user_model_id.clone(),
            base_environment: BTreeMap::new(),
        })
    })();
    match prepared {
        Ok(plan) => Ok(plan),
        Err(error) => Err(with_prepare_cleanup(error, &session)),
    }
}

fn run_windows_runtime_open(request: WindowsRuntimeOpenRequest) -> Result<(), String> {
    let owner = match WindowsRuntimeOwnerClaim::acquire()? {
        WindowsRuntimeOwnerClaim::Owned(owner) => owner,
        WindowsRuntimeOwnerClaim::Existing => {
            raise_existing_windows_runtime()?;
            println!("ready");
            std::io::stdout()
                .flush()
                .map_err(|error| format!("cannot report existing Windows Runtime: {error}"))?;
            return Ok(());
        }
    };
    let launch_gate = acquire_windows_install_state()?;
    let state = installed_state_from_environment()?;
    let app = discover_codex_package()?;
    if app.package_full_name != state.package_full_name {
        return Err("installed Windows Runtime package identity changed".to_string());
    }
    let user_root = state
        .state_path
        .parent()
        .ok_or_else(|| "Windows install state has no parent directory".to_string())?;
    let mut plan = prepare_windows_runtime_open(
        &app,
        user_root,
        &request.source_home,
        request.source_bounds.as_deref(),
    )?;
    plan.base_environment =
        WindowsInstalledRuntimeRegistration::environment_from_install_state(&state)?;
    let registration = WindowsInstalledRuntimeRegistration::from_install_state(&state)?;
    execute_windows_runtime_open(plan, &registration, launch_gate, owner)
}

fn raise_existing_windows_runtime() -> Result<(), String> {
    let pipe = RUNTIME_RAISE_PIPE
        .encode_utf16()
        .chain([0])
        .collect::<Vec<_>>();
    let deadline = Instant::now() + Duration::from_millis(RAISE_TIMEOUT_MS as u64);
    loop {
        let request = b"raise\n";
        let mut response = [0u8; 32];
        let mut read = 0;
        if unsafe {
            CallNamedPipeW(
                pipe.as_ptr(),
                request.as_ptr().cast(),
                request.len() as u32,
                response.as_mut_ptr().cast(),
                response.len() as u32,
                &mut read,
                250,
            )
        } != 0
        {
            return if &response[..read as usize] == b"raised\n" {
                Ok(())
            } else {
                Err("existing Windows Runtime returned an invalid raise response".to_string())
            };
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "cannot raise the existing Windows Runtime: {}",
                std::io::Error::last_os_error()
            ));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn execute_windows_runtime_open(
    mut plan: WindowsRuntimeOpenPlan,
    registration: &WindowsInstalledRuntimeRegistration,
    launch_gate: WindowsInstallStateGuard,
    owner: WindowsRuntimeOwner,
) -> Result<(), String> {
    let ready_pipe = WindowsRuntimeReadyPipe::create()
        .map_err(|error| with_shutdown_cleanup(error, &plan.session, Ok(())))?;
    let close_pipe = WindowsRuntimeReadyPipe::create_close()
        .map_err(|error| with_shutdown_cleanup(error, &plan.session, Ok(())))?;
    plan.env_flags.insert(
        "INCODEX_WINDOWS_READY_PIPE".to_string(),
        ready_pipe.name().to_string(),
    );
    plan.env_flags.insert(
        "INCODEX_WINDOWS_CLOSE_PIPE".to_string(),
        close_pipe.name().to_string(),
    );
    let activation = match plan.activation_request() {
        Ok(activation) => activation,
        Err(error) => return Err(with_shutdown_cleanup(error, &plan.session, Ok(()))),
    };
    let process_tree = match activate_packaged_with_installed_runtime(&activation, registration) {
        Ok(process_tree) => process_tree,
        Err(failure) => {
            let (error, shutdown) = failure.into_parts();
            return Err(with_shutdown_cleanup(
                format!("cannot launch installed Windows Runtime: {error}"),
                &plan.session,
                shutdown,
            ));
        }
    };
    drop(launch_gate);
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (close_sender, close_receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = ready_sender.send(ready_pipe.accept());
    });
    thread::spawn(move || {
        let _ = close_sender.send(close_pipe.accept());
    });
    run_guardian_lifecycle(
        plan.session,
        process_tree,
        ready_receiver,
        close_receiver,
        owner,
    )
}

fn run_guardian_lifecycle(
    session: WindowsSessionHome,
    mut process_tree: WindowsProcessTree,
    ready_receiver: Receiver<Result<WindowsRuntimeAcceptance, String>>,
    close_receiver: Receiver<Result<WindowsRuntimeAcceptance, String>>,
    _owner: WindowsRuntimeOwner,
) -> Result<(), String> {
    let cancelled = listen_for_guardian_cancellation();
    let ready_deadline = Instant::now() + READY_TIMEOUT;
    let mut runtime_accepted = false;
    loop {
        let authenticated_close = match receive_authenticated_close(&close_receiver, &process_tree)
        {
            Ok(authenticated_close) => authenticated_close,
            Err(error) => return guardian_failure(error, &session, &mut process_tree),
        };
        let process_status = match process_tree.try_wait() {
            Ok(status) => status,
            Err(error) => {
                return guardian_failure(
                    format!("cannot inspect installed Windows Runtime process: {error}"),
                    &session,
                    &mut process_tree,
                )
            }
        };
        match windows_runtime_startup_action(authenticated_close, process_status.is_some()) {
            WindowsRuntimeStartupAction::Finish => {
                return finish_authenticated_close(
                    &session,
                    &mut process_tree,
                    process_status.is_some(),
                )
            }
            WindowsRuntimeStartupAction::FailExited => {
                return Err(with_shutdown_cleanup(
                    format!(
                        "Codex exited with {} before the shared Runtime was ready",
                        process_status.expect("exited startup action requires process status")
                    ),
                    &session,
                    Ok(()),
                ))
            }
            WindowsRuntimeStartupAction::Continue => {}
        }
        if cancelled.load(Ordering::Acquire) {
            return guardian_failure(
                "Windows Runtime launch was cancelled before readiness".to_string(),
                &session,
                &mut process_tree,
            );
        }
        if !runtime_accepted {
            match ready_receiver.try_recv() {
                Ok(Ok(acceptance)) => {
                    if let Err(error) = authenticate_runtime_signal(
                        &process_tree,
                        &acceptance,
                        "accepted",
                        "readiness",
                    ) {
                        return guardian_failure(error, &session, &mut process_tree);
                    }
                    runtime_accepted = true;
                }
                Ok(Err(error)) => return guardian_failure(error, &session, &mut process_tree),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    return guardian_failure(
                        "Windows Runtime readiness channel closed before acceptance".to_string(),
                        &session,
                        &mut process_tree,
                    )
                }
            }
        }
        let visible = match process_tree.has_visible_window() {
            Ok(visible) => visible,
            Err(error) => {
                return guardian_failure(
                    format!("cannot inspect the installed Windows Runtime window: {error}"),
                    &session,
                    &mut process_tree,
                )
            }
        };
        if windows_runtime_ready_for_handshake(runtime_accepted, visible) {
            break;
        }
        if Instant::now() >= ready_deadline {
            return guardian_failure(
                "timed out waiting for the shared Windows Runtime UI".to_string(),
                &session,
                &mut process_tree,
            );
        }
        thread::sleep(POLL_INTERVAL);
    }

    println!("ready");
    if let Err(error) = std::io::stdout().flush() {
        return guardian_failure(
            format!("cannot report Windows Runtime readiness: {error}"),
            &session,
            &mut process_tree,
        );
    }

    loop {
        if cancelled.load(Ordering::Acquire) {
            return guardian_failure(
                "Windows Runtime lifecycle was cancelled".to_string(),
                &session,
                &mut process_tree,
            );
        }
        let authenticated_close = match receive_authenticated_close(&close_receiver, &process_tree)
        {
            Ok(authenticated_close) => authenticated_close,
            Err(error) => return guardian_failure(error, &session, &mut process_tree),
        };
        let job_empty = match process_tree.try_wait() {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(error) => {
                return guardian_failure(
                    format!("cannot inspect installed Windows Runtime process: {error}"),
                    &session,
                    &mut process_tree,
                )
            }
        };
        if windows_runtime_shutdown_authorized(authenticated_close, job_empty) {
            return finish_authenticated_close(&session, &mut process_tree, job_empty);
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn receive_authenticated_close(
    close_receiver: &Receiver<Result<WindowsRuntimeAcceptance, String>>,
    process_tree: &WindowsProcessTree,
) -> Result<bool, String> {
    match close_receiver.try_recv() {
        Ok(Ok(closure)) => {
            authenticate_runtime_signal(process_tree, &closure, "closed", "closure")?;
            Ok(true)
        }
        Ok(Err(error)) => Err(error),
        Err(TryRecvError::Empty) => Ok(false),
        Err(TryRecvError::Disconnected) => {
            Err("Windows Runtime close channel ended without evidence".to_string())
        }
    }
}

fn finish_authenticated_close(
    session: &WindowsSessionHome,
    process_tree: &mut WindowsProcessTree,
    job_empty: bool,
) -> Result<(), String> {
    if job_empty {
        return finish_guardian(session, Ok(()));
    }
    let shutdown = process_tree
        .terminate_successfully()
        .map(|_| ())
        .map_err(|error| {
            format!("cannot prove Windows Runtime shutdown after its main window closed: {error}")
        });
    finish_guardian(session, shutdown)
}

fn authenticate_runtime_signal(
    process_tree: &WindowsProcessTree,
    signal: &WindowsRuntimeAcceptance,
    expected_message: &str,
    label: &str,
) -> Result<(), String> {
    if signal.message != expected_message {
        return Err(format!("Windows Runtime {label} message is invalid"));
    }
    match process_tree.contains_process(signal.process_id) {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!(
            "Windows Runtime {label} writer is outside the isolated Job"
        )),
        Err(error) => Err(format!(
            "cannot authenticate Windows Runtime {label} writer: {error}"
        )),
    }
}

fn listen_for_guardian_cancellation() -> Arc<AtomicBool> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&cancelled);
    thread::spawn(move || {
        let mut line = String::new();
        if std::io::stdin().lock().read_line(&mut line).is_ok() && line.trim() == "cancel" {
            signal.store(true, Ordering::Release);
        }
    });
    cancelled
}

fn installed_state_from_environment() -> Result<WindowsInstallState, String> {
    let state_path = PathBuf::from(
        std::env::var_os("INCODEX_WINDOWS_STATE_PATH")
            .ok_or_else(|| "Windows Runtime state path is unavailable".to_string())?,
    );
    if state_path.file_name().and_then(|name| name.to_str()) != Some("windows-install.json") {
        return Err("Windows Runtime state path is invalid".to_string());
    }
    let user_root = state_path
        .parent()
        .ok_or_else(|| "Windows Runtime state path has no parent".to_string())?;
    let state = read_windows_install_state(user_root)?
        .ok_or_else(|| "Windows Runtime is not installed".to_string())?;
    if state.state_path
        != fs::canonicalize(&state_path)
            .map_err(|error| format!("cannot resolve the Windows Runtime state path: {error}"))?
    {
        return Err("Windows Runtime state path identity changed".to_string());
    }
    if !state.desired_enabled()
        || !matches!(
            state.phase,
            WindowsInstallPhase::EnabledUnobserved | WindowsInstallPhase::EnabledObserved
        )
    {
        return Err("Windows Runtime is disabled".to_string());
    }
    let current = fs::canonicalize(
        std::env::current_exe()
            .map_err(|error| format!("cannot locate the Windows Runtime helper: {error}"))?,
    )
    .map_err(|error| format!("cannot resolve the Windows Runtime helper: {error}"))?;
    if current != state.helper_path {
        return Err("Windows Runtime helper identity changed".to_string());
    }
    if std::env::var("INCODEX_WINDOWS_PACKAGE_FULL_NAME")
        .ok()
        .as_deref()
        != Some(state.package_full_name.as_str())
    {
        return Err("Windows Runtime environment package identity changed".to_string());
    }
    Ok(state)
}

fn guardian_failure(
    message: String,
    session: &WindowsSessionHome,
    process_tree: &mut WindowsProcessTree,
) -> Result<(), String> {
    let shutdown = process_tree.terminate().map(|_| ()).map_err(|error| {
        format!("cannot prove the installed Windows Runtime Job is empty: {error}")
    });
    Err(with_shutdown_cleanup(message, session, shutdown))
}

fn finish_guardian(
    session: &WindowsSessionHome,
    shutdown: Result<(), String>,
) -> Result<(), String> {
    match cleanup_windows_session_after_shutdown(session, shutdown) {
        WindowsCleanupResult::Removed => Ok(()),
        WindowsCleanupResult::Retained { reason } | WindowsCleanupResult::Unknown { reason } => {
            Err(format!(
                "Windows Runtime session retained at {}: {reason}",
                session.root.display()
            ))
        }
    }
}

fn with_shutdown_cleanup(
    message: String,
    session: &WindowsSessionHome,
    shutdown: Result<(), String>,
) -> String {
    match cleanup_windows_session_after_shutdown(session, shutdown) {
        WindowsCleanupResult::Removed => message,
        WindowsCleanupResult::Retained { reason } | WindowsCleanupResult::Unknown { reason } => {
            format!(
                "{message}; Windows Runtime session retained at {}: {reason}",
                session.root.display()
            )
        }
    }
}

fn with_prepare_cleanup(message: String, session: &WindowsSessionHome) -> String {
    match burn_windows_session(session) {
        WindowsCleanupResult::Removed => message,
        WindowsCleanupResult::Retained { reason } | WindowsCleanupResult::Unknown { reason } => {
            format!(
                "{message}; Windows Runtime session retained at {}: {reason}",
                session.root.display()
            )
        }
    }
}

fn validate_source_bounds(value: &str) -> Result<(), String> {
    let values = value.split(',').collect::<Vec<_>>();
    if values.len() != 4
        || values
            .iter()
            .any(|value| value.len() > 11 || value.parse::<i32>().is_err())
        || values[2].parse::<i32>().is_ok_and(|value| value <= 0)
        || values[3].parse::<i32>().is_ok_and(|value| value <= 0)
    {
        return Err("Windows Runtime source bounds are invalid".to_string());
    }
    Ok(())
}

fn flag_value<'a>(arguments: &'a [String], flag: &str) -> Result<&'a str, String> {
    arguments
        .windows(2)
        .find_map(|pair| (pair[0] == flag).then_some(pair[1].as_str()))
        .ok_or_else(|| format!("Windows Runtime open is missing {flag}"))
}
