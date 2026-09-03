use std::collections::BTreeMap;
use std::ffi::c_void;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows_sys::core::{GUID, HRESULT};
use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, RPC_E_CHANGED_MODE, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};

use crate::windows_activation_capability::WindowsActivationCapability;
pub use crate::windows_activation_capability::{
    windows_debugger_route, windows_transient_debugger_route, WindowsDebuggerRoute,
};
use crate::windows_app::validate_codex_package_full_name;
use crate::windows_install_state::{read_windows_install_state, WindowsInstallState};
use crate::windows_launch::{
    WindowsActivationEnvironment, WindowsActivationEnvironmentPipe, WindowsLaunchMode,
};
use crate::windows_process::{
    assign_debugged_process_to_job, process_command_line, resume_debugged_package_process,
    snapshot_process_ids, WindowsPendingJob, WindowsProcessTree,
};
use crate::windows_registration::{
    retire_windows_debug_registration, stage_transient_windows_debug_registration,
};

const PACKAGE_DEBUGGER_MODE: &str = "__incodex_windows_package_debugger";
const INSTALLED_DEBUGGER_MODE: &str = "__incodex_windows_installed_debugger";
const PACKAGE_DEBUGGER_COMMAND_LIMIT_UTF16: usize = 260;
const PACKAGE_ACTIVATION_LOCK_NAME: &str = "Local\\Incodex-OpenAI.Codex-Activation";
const PACKAGE_ACTIVATION_LOCK_TIMEOUT_MS: u32 = 15_000;
const ACTIVATION_ENVIRONMENT_TIMEOUT: Duration = Duration::from_secs(15);

const CLSID_APPLICATION_ACTIVATION_MANAGER: GUID =
    GUID::from_u128(0x45ba127d_10a8_46ea_8ab7_56ea9078943c);
const IID_APPLICATION_ACTIVATION_MANAGER: GUID =
    GUID::from_u128(0x2e941141_7f97_4756_ba1d_9decde894a3d);
const CLSID_PACKAGE_DEBUG_SETTINGS: GUID = GUID::from_u128(0xb1aec16f_2383_4852_b0e9_8f0b1dc66b4d);
const IID_PACKAGE_DEBUG_SETTINGS: GUID = GUID::from_u128(0xf27c3930_8029_4ad1_94e3_3dba417810c1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsActivationRequest {
    package_full_name: String,
    app_user_model_id: String,
    arguments: String,
    activation_user_data_dir: Option<String>,
    environment: Vec<u16>,
    claimed_environment: BTreeMap<String, String>,
    transient_debug_environment: Option<Vec<u16>>,
    transient_debugger_executable: Option<PathBuf>,
    transient_user_root: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsInstalledRuntimeRegistration {
    package_full_name: String,
    debugger_command_line: String,
    environment: Vec<u16>,
}

impl WindowsInstalledRuntimeRegistration {
    pub fn environment_from_install_state(
        state: &WindowsInstallState,
    ) -> Result<BTreeMap<String, OsString>, String> {
        let user_root = state
            .state_path
            .parent()
            .ok_or_else(|| "Windows install state has no parent directory".to_string())?;
        let bootstrap = user_root
            .join("runtime")
            .join("incodex-windows-bootstrap.cjs");
        Ok(BTreeMap::from([
            ("NODE_OPTIONS".to_string(), node_require_option(&bootstrap)?),
            (
                "INCODEX_WINDOWS_REGISTRATION_ID".to_string(),
                OsString::from(&state.registration_id),
            ),
            (
                "INCODEX_WINDOWS_PACKAGE_FULL_NAME".to_string(),
                OsString::from(&state.package_full_name),
            ),
            (
                "INCODEX_WINDOWS_STATE_PATH".to_string(),
                state.state_path.as_os_str().to_os_string(),
            ),
            (
                "INCODEX_WINDOWS_HELPER".to_string(),
                state.helper_path.as_os_str().to_os_string(),
            ),
        ]))
    }

    pub fn from_install_state(state: &WindowsInstallState) -> Result<Self, String> {
        Self::new(
            &state.package_full_name,
            &state.helper_path,
            Self::environment_from_install_state(state)?,
        )
    }

    pub fn new(
        package_full_name: &str,
        helper_path: &Path,
        environment: BTreeMap<String, OsString>,
    ) -> Result<Self, String> {
        validate_text(package_full_name, "package full name")?;
        if !helper_path.is_absolute() {
            return Err("Windows installed Runtime helper path must be absolute".to_string());
        }
        let debugger_command_line = [
            helper_path.as_os_str().to_os_string(),
            OsString::from(INSTALLED_DEBUGGER_MODE),
            OsString::from("--package"),
            OsString::from(package_full_name),
        ]
        .iter()
        .map(|argument| quote_windows_argument(argument))
        .collect::<Result<Vec<_>, _>>()?
        .join(" ");
        let debugger_command_line = validate_package_debugger_command(debugger_command_line)?;
        Ok(Self {
            package_full_name: package_full_name.to_string(),
            debugger_command_line,
            environment: environment_block(environment)?,
        })
    }

    pub fn package_full_name(&self) -> &str {
        &self.package_full_name
    }

    pub fn debugger_command_line(&self) -> &str {
        &self.debugger_command_line
    }

    pub fn environment(&self) -> &[u16] {
        &self.environment
    }
}

fn node_require_option(path: &Path) -> Result<OsString, String> {
    let mut path = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if path.starts_with(&[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16]) {
        path.drain(..4);
    }
    if path.len() < 3
        || path[1] != b':' as u16
        || !matches!(path[2], unit if unit == b'\\' as u16 || unit == b'/' as u16)
        || path.contains(&0)
        || path.contains(&(b'"' as u16))
    {
        return Err(
            "Windows Runtime bootstrap path cannot be encoded for NODE_OPTIONS".to_string(),
        );
    }
    for unit in &mut path {
        if *unit == b'\\' as u16 {
            *unit = b'/' as u16;
        }
    }
    let mut option = OsStr::new("--require=\"").encode_wide().collect::<Vec<_>>();
    option.extend(path);
    option.push(b'"' as u16);
    Ok(OsString::from_wide(&option))
}

struct InstalledDebuggerRegistrationEvidence {
    helper: std::path::PathBuf,
    package_full_name: String,
    user_root: std::path::PathBuf,
}

fn installed_debugger_registration_evidence(
    registered_package: &str,
) -> Result<InstalledDebuggerRegistrationEvidence, String> {
    let helper = std::env::current_exe()
        .map_err(|error| format!("cannot locate the Windows installed debugger: {error}"))?;
    let user_root = installed_debugger_user_root(&helper)?;
    let helper = std::fs::canonicalize(&helper)
        .map_err(|error| format!("cannot resolve the Windows installed debugger: {error}"))?;
    validate_codex_package_full_name(registered_package)?;
    Ok(InstalledDebuggerRegistrationEvidence {
        helper,
        package_full_name: registered_package.to_string(),
        user_root,
    })
}

fn installed_debugger_user_root(helper: &Path) -> Result<std::path::PathBuf, String> {
    let hash_dir = helper
        .parent()
        .ok_or_else(|| "Windows installed debugger has no release directory".to_string())?;
    let helper_name = helper
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Windows installed debugger name is not valid Unicode".to_string())?;
    let hash = hash_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Windows installed debugger release is not valid Unicode".to_string())?;
    let collection = hash_dir
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Windows installed debugger has no helper collection".to_string())?;
    let lowercase_hex = |value: &str| {
        value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    };
    let short = helper_name.eq_ignore_ascii_case("i.exe")
        && collection.eq_ignore_ascii_case("i")
        && hash.len() == 16
        && lowercase_hex(hash);
    let legacy = helper_name.eq_ignore_ascii_case("incodex-helper.exe")
        && collection.eq_ignore_ascii_case("helpers")
        && hash.len() == 64
        && lowercase_hex(hash);
    if !short && !legacy {
        return Err("Windows installed debugger is not a content-addressed helper".to_string());
    }
    let helpers_dir = hash_dir
        .parent()
        .ok_or_else(|| "Windows installed debugger is outside the helpers directory".to_string())?;
    let windows_dir = helpers_dir
        .parent()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("windows"))
        })
        .ok_or_else(|| {
            "Windows installed debugger is outside the Windows Runtime directory".to_string()
        })?;
    windows_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Windows installed debugger has no Incodex root".to_string())
}

fn installed_state_for_current_helper(
    evidence: &InstalledDebuggerRegistrationEvidence,
) -> Result<WindowsInstallState, String> {
    let state = read_windows_install_state(&evidence.user_root)?
        .ok_or_else(|| "Windows installed debugger state does not exist".to_string())?;
    if state.helper_path != evidence.helper {
        return Err("Windows installed debugger state does not authorize this helper".to_string());
    }
    if state.package_full_name != evidence.package_full_name {
        return Err("Windows installed debugger package evidence changed".to_string());
    }
    Ok(state)
}

fn installed_debugger_route_from_state(
    state: Result<WindowsInstallState, String>,
    registered_package: &str,
    command_line: Option<&str>,
) -> Result<(String, WindowsDebuggerRoute), String> {
    validate_codex_package_full_name(registered_package)?;
    let Ok(state) = state else {
        return Ok((
            registered_package.to_string(),
            WindowsDebuggerRoute::ResumeNormally,
        ));
    };
    if state.package_full_name != registered_package {
        return Ok((
            registered_package.to_string(),
            WindowsDebuggerRoute::ResumeNormally,
        ));
    }
    let route = command_line
        .map(|command_line| windows_installed_debugger_route(&state, command_line))
        .transpose()?
        .unwrap_or(WindowsDebuggerRoute::ResumeNormally);
    Ok((state.package_full_name, route))
}

pub fn windows_installed_debugger_route(
    state: &WindowsInstallState,
    command_line: &str,
) -> Result<WindowsDebuggerRoute, String> {
    if !state.desired_enabled() {
        return Ok(WindowsDebuggerRoute::ResumeNormally);
    }
    windows_debugger_route(command_line)
}

impl WindowsActivationRequest {
    pub fn new<I>(
        package_full_name: &str,
        app_user_model_id: &str,
        arguments: I,
        environment: BTreeMap<String, OsString>,
    ) -> Result<Self, String>
    where
        I: IntoIterator<Item = OsString>,
    {
        validate_text(package_full_name, "package full name")?;
        validate_text(app_user_model_id, "app user model id")?;
        let mut activation_user_data_dir = None;
        let arguments = arguments
            .into_iter()
            .map(|argument| {
                if let Some(argument) = argument.to_str() {
                    if let Some(value) = argument.strip_prefix("--user-data-dir=") {
                        if activation_user_data_dir
                            .replace(value.to_string())
                            .is_some()
                        {
                            return Err(
                                "Windows activation arguments repeat the user data directory"
                                    .to_string(),
                            );
                        }
                    }
                }
                let quoted = quote_windows_argument(&argument)?;
                Ok(quoted)
            })
            .collect::<Result<Vec<_>, String>>()?
            .join(" ");
        let claimed_environment = environment
            .iter()
            .map(|(name, value)| {
                let value = value.to_str().ok_or_else(|| {
                    format!("Windows activation environment value for {name} is not valid Unicode")
                })?;
                Ok((name.clone(), value.to_string()))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        let environment = environment_block(environment)?;
        Ok(Self {
            package_full_name: package_full_name.to_string(),
            app_user_model_id: app_user_model_id.to_string(),
            arguments,
            activation_user_data_dir,
            environment,
            claimed_environment,
            transient_debug_environment: None,
            transient_debugger_executable: None,
            transient_user_root: None,
        })
    }

    pub fn with_transient_runtime(
        mut self,
        bootstrap: &Path,
        debugger_executable: &Path,
        user_root: &Path,
    ) -> Result<Self, String> {
        let mut environment = self
            .claimed_environment
            .iter()
            .map(|(name, value)| (name.clone(), OsString::from(value)))
            .collect::<BTreeMap<_, _>>();
        environment.insert("NODE_OPTIONS".to_string(), node_require_option(bootstrap)?);
        self.transient_debug_environment = Some(environment_block(environment)?);
        self.transient_debugger_executable = Some(debugger_executable.to_path_buf());
        self.transient_user_root = Some(user_root.to_path_buf());
        Ok(self)
    }

    pub fn package_full_name(&self) -> &str {
        &self.package_full_name
    }

    pub fn app_user_model_id(&self) -> &str {
        &self.app_user_model_id
    }

    pub fn arguments(&self) -> &str {
        &self.arguments
    }

    pub fn environment(&self) -> &[u16] {
        &self.environment
    }

    pub fn transient_debug_environment(&self) -> &[u16] {
        self.transient_debug_environment.as_deref().unwrap_or(&[])
    }

    fn transient_debugger_executable(&self) -> Result<&Path, String> {
        self.transient_debugger_executable
            .as_deref()
            .ok_or_else(|| "Windows transient activation is missing its stable helper".to_string())
    }

    fn transient_user_root(&self) -> Result<&Path, String> {
        self.transient_user_root
            .as_deref()
            .ok_or_else(|| "Windows transient activation is missing its private root".to_string())
    }

    pub fn activation_capability(&self) -> Result<WindowsActivationCapability, String> {
        let user_data_dir = self.activation_user_data_dir.as_deref().ok_or_else(|| {
            "Windows installed activation is missing its isolated user data directory".to_string()
        })?;
        WindowsActivationCapability::from_user_data_dir(user_data_dir)
    }

    pub fn activation_environment(
        &self,
        mode: WindowsLaunchMode,
    ) -> Result<WindowsActivationEnvironment, String> {
        Ok(WindowsActivationEnvironment {
            mode,
            environment: self.claimed_environment.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsActivationFailure {
    message: String,
    shutdown: Result<(), String>,
}

impl WindowsActivationFailure {
    pub(crate) fn before_start(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            shutdown: Ok(()),
        }
    }

    pub(crate) fn after_start(message: impl Into<String>, shutdown: Result<(), String>) -> Self {
        Self {
            message: message.into(),
            shutdown,
        }
    }

    pub(crate) fn into_parts(self) -> (String, Result<(), String>) {
        (self.message, self.shutdown)
    }
}

impl From<String> for WindowsActivationFailure {
    fn from(message: String) -> Self {
        Self::before_start(message)
    }
}

struct ActivationEnvironmentServer {
    pipe_name: String,
    result: Receiver<Result<u32, String>>,
    worker: Option<JoinHandle<()>>,
}

impl ActivationEnvironmentServer {
    fn start(
        capability: &WindowsActivationCapability,
        package_full_name: &str,
        response: WindowsActivationEnvironment,
    ) -> Result<Self, String> {
        let pipe = WindowsActivationEnvironmentPipe::create(capability)?;
        let pipe_name = pipe.name().to_string();
        let job_name = capability.job_name().to_string();
        let package_full_name = package_full_name.to_string();
        let (sender, result) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let response = pipe.respond_once(&job_name, &package_full_name, &response);
            let _ = sender.send(response);
        });
        Ok(Self {
            pipe_name,
            result,
            worker: Some(worker),
        })
    }

    fn wait(&mut self, timeout: Duration) -> Result<u32, String> {
        match self.result.recv_timeout(timeout) {
            Ok(result) => {
                self.join();
                result
            }
            Err(RecvTimeoutError::Timeout) => {
                Err("timed out waiting for the Windows activation environment claim".to_string())
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.join();
                Err("Windows activation environment server ended without a result".to_string())
            }
        }
    }

    fn cancel(&mut self) {
        if self.worker.is_none() {
            return;
        }
        let _ = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.pipe_name);
        if self.result.recv_timeout(Duration::from_secs(1)).is_ok() {
            self.join();
        } else {
            self.worker.take();
        }
    }

    fn join(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for ActivationEnvironmentServer {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub fn activate_packaged_kill_on_drop(
    request: &WindowsActivationRequest,
) -> Result<WindowsProcessTree, WindowsActivationFailure> {
    activate_packaged(request, None)
}

pub fn activate_packaged_with_installed_runtime(
    request: &WindowsActivationRequest,
    registration: &WindowsInstalledRuntimeRegistration,
) -> Result<WindowsProcessTree, WindowsActivationFailure> {
    if request.package_full_name() != registration.package_full_name() {
        return Err(WindowsActivationFailure::before_start(
            "Windows installed Runtime registration does not match the activation package",
        ));
    }
    activate_packaged_with_capability(request, WindowsLaunchMode::Runtime)
}

pub fn activate_packaged_with_installed_cdp(
    request: &WindowsActivationRequest,
    registration: &WindowsInstalledRuntimeRegistration,
) -> Result<WindowsProcessTree, WindowsActivationFailure> {
    if request.package_full_name() != registration.package_full_name() {
        return Err(WindowsActivationFailure::before_start(
            "Windows installed Runtime registration does not match the activation package",
        ));
    }
    activate_packaged_with_capability(request, WindowsLaunchMode::Cdp)
}

pub fn activation_requires_runtime_environment_claim(mode: WindowsLaunchMode) -> bool {
    matches!(mode, WindowsLaunchMode::Runtime)
}

fn activate_packaged_with_capability(
    request: &WindowsActivationRequest,
    mode: WindowsLaunchMode,
) -> Result<WindowsProcessTree, WindowsActivationFailure> {
    let _activation_lock = acquire_package_activation_lock()?;
    let _apartment = ComApartment::initialize()?;
    let existing_processes = snapshot_process_ids()
        .map_err(|error| format!("cannot snapshot Windows processes before activation: {error}"))?;
    let capability = request.activation_capability()?;
    let pending_job = WindowsPendingJob::create_for_capability(&capability)
        .map_err(|error| format!("cannot create Windows activation Job Object: {error}"))?;
    let mut environment_server = if activation_requires_runtime_environment_claim(mode) {
        Some(ActivationEnvironmentServer::start(
            &capability,
            request.package_full_name(),
            request.activation_environment(mode)?,
        )?)
    } else {
        None
    };
    let manager = match ComPtr::create(
        &CLSID_APPLICATION_ACTIVATION_MANAGER,
        &IID_APPLICATION_ACTIVATION_MANAGER,
        "Windows application activation manager",
    ) {
        Ok(manager) => manager,
        Err(error) => {
            if let Some(server) = &mut environment_server {
                server.cancel();
            }
            return Err(WindowsActivationFailure::before_start(error));
        }
    };
    let app_user_model_id = wide_nul(request.app_user_model_id());
    let arguments = wide_nul(request.arguments());
    let mut process_id = 0;
    let result = unsafe {
        let vtable = *(manager.raw() as *mut *const ApplicationActivationManagerVtable);
        ((*vtable).activate_application)(
            manager.raw(),
            app_user_model_id.as_ptr(),
            arguments.as_ptr(),
            0,
            &mut process_id,
        )
    };
    if failed(result) {
        if let Some(server) = &mut environment_server {
            server.cancel();
        }
        let shutdown = if process_id != 0 && !existing_processes.contains(&process_id) {
            Err(format!(
                "activation failed after reporting new Windows process {process_id}; process shutdown is unproven"
            ))
        } else {
            Ok(())
        };
        return Err(WindowsActivationFailure::after_start(
            hresult_message("cannot activate the Windows Codex package", result),
            shutdown,
        ));
    }
    if process_id == 0 || existing_processes.contains(&process_id) {
        if let Some(server) = &mut environment_server {
            server.cancel();
        }
        return Err(WindowsActivationFailure::after_start(
            "Windows package activation did not create a new isolated Codex process",
            Ok(()),
        ));
    }
    let mut process_tree = match pending_job.attach(process_id, request.package_full_name()) {
        Ok(process_tree) => process_tree,
        Err(error) => {
            if let Some(server) = &mut environment_server {
                server.cancel();
            }
            return Err(WindowsActivationFailure::after_start(
                format!(
                    "cannot contain activated Windows Codex process {process_id} in its capability Job: {error}"
                ),
                Err(format!(
                    "cannot prove activated Windows Codex process {process_id} and its descendants exited after Job attachment failed"
                )),
            ));
        }
    };
    let Some(environment_server) = &mut environment_server else {
        return Ok(process_tree);
    };
    let environment_client = match environment_server.wait(ACTIVATION_ENVIRONMENT_TIMEOUT) {
        Ok(process_id) => process_id,
        Err(error) => {
            let shutdown = process_tree.terminate().map(|_| ()).map_err(|shutdown_error| {
                format!(
                    "cannot prove the isolated Windows Job is empty after environment claim failed: {shutdown_error}"
                )
            });
            environment_server.cancel();
            return Err(WindowsActivationFailure::after_start(error, shutdown));
        }
    };
    if environment_client != process_id {
        let shutdown = process_tree.terminate().map(|_| ()).map_err(|shutdown_error| {
            format!(
                "cannot prove the isolated Windows Job is empty after a foreign environment claim: {shutdown_error}"
            )
        });
        return Err(WindowsActivationFailure::after_start(
            "Windows activation environment was claimed by the wrong Job process",
            shutdown,
        ));
    }
    Ok(process_tree)
}

fn activate_packaged(
    request: &WindowsActivationRequest,
    restoration: Option<&WindowsInstalledRuntimeRegistration>,
) -> Result<WindowsProcessTree, WindowsActivationFailure> {
    let _activation_lock = acquire_package_activation_lock()?;
    let _apartment = ComApartment::initialize()?;
    let capability = request.activation_capability()?;
    if request.transient_debug_environment().is_empty() {
        return Err(WindowsActivationFailure::before_start(
            "Windows transient activation is missing its private Runtime bootstrap",
        ));
    }
    let debugger_executable = request.transient_debugger_executable()?;
    let user_root = request.transient_user_root()?;
    let package = wide_nul(request.package_full_name());
    let debugger_command = transient_debugger_command_line(
        debugger_executable,
        capability.job_name(),
        request.package_full_name(),
    )?;
    let evidence = stage_transient_windows_debug_registration(
        user_root,
        request.package_full_name(),
        debugger_executable,
    )?;
    let debugger_command = wide_nul(&debugger_command);
    let mut debugging = match PackageDebugGuard::enable(
        &package,
        &debugger_command,
        request.transient_debug_environment(),
        restoration,
    ) {
        Ok(debugging) => debugging,
        Err(error) => return Err(WindowsActivationFailure::before_start(error)),
    };
    let activation = activate_packaged_with_capability(request, WindowsLaunchMode::Cdp);
    let restored = debugging
        .restore()
        .and_then(|()| retire_windows_debug_registration(user_root, &evidence.registration_id));
    match (activation, restored) {
        (Ok(process_tree), Ok(())) => Ok(process_tree),
        (Ok(mut process_tree), Err(error)) => {
            let process_shutdown = process_tree.terminate().map(|_| ()).map_err(|shutdown_error| {
                format!(
                    "cannot prove the isolated Windows Job is empty after package debug restoration failed: {shutdown_error}"
                )
            });
            let shutdown = cleanup_proof_after_debugging(&Err(error.clone()), process_shutdown);
            Err(WindowsActivationFailure::after_start(error, shutdown))
        }
        (Err(failure), restoration) => {
            let shutdown = cleanup_proof_after_debugging(&restoration, failure.shutdown);
            Err(WindowsActivationFailure::after_start(
                join_cleanup_error(failure.message, restoration),
                shutdown,
            ))
        }
    }
}

pub fn enable_installed_runtime(
    registration: &WindowsInstalledRuntimeRegistration,
) -> Result<(), String> {
    let _activation_lock = acquire_package_activation_lock()?;
    let _apartment = ComApartment::initialize()?;
    let interface = ComPtr::create(
        &CLSID_PACKAGE_DEBUG_SETTINGS,
        &IID_PACKAGE_DEBUG_SETTINGS,
        "Windows package debug settings",
    )?;
    let package = wide_nul(registration.package_full_name());
    let debugger = wide_nul(registration.debugger_command_line());
    let vtable = unsafe { *(interface.raw() as *mut *const PackageDebugSettingsVtable) };
    let result = unsafe {
        ((*vtable).enable_debugging)(
            interface.raw(),
            package.as_ptr(),
            debugger.as_ptr(),
            registration.environment().as_ptr(),
        )
    };
    if failed(result) {
        Err(hresult_message(
            "cannot enable the installed Windows Runtime",
            result,
        ))
    } else {
        Ok(())
    }
}

pub fn disable_installed_runtime(package_full_name: &str) -> Result<(), String> {
    validate_text(package_full_name, "package full name")?;
    let _activation_lock = acquire_package_activation_lock()?;
    let _apartment = ComApartment::initialize()?;
    let interface = ComPtr::create(
        &CLSID_PACKAGE_DEBUG_SETTINGS,
        &IID_PACKAGE_DEBUG_SETTINGS,
        "Windows package debug settings",
    )?;
    let package = wide_nul(package_full_name);
    let vtable = unsafe { *(interface.raw() as *mut *const PackageDebugSettingsVtable) };
    let result = unsafe { ((*vtable).disable_debugging)(interface.raw(), package.as_ptr()) };
    if failed(result) {
        Err(hresult_message(
            "cannot disable the installed Windows Runtime",
            result,
        ))
    } else {
        Ok(())
    }
}

fn acquire_package_activation_lock() -> Result<PackageActivationLock, String> {
    let name: Vec<u16> = PACKAGE_ACTIVATION_LOCK_NAME
        .encode_utf16()
        .chain([0])
        .collect();
    let handle = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(format!(
            "cannot create the Windows package activation lock: {}",
            std::io::Error::last_os_error()
        ));
    }
    match unsafe { WaitForSingleObject(handle, PACKAGE_ACTIVATION_LOCK_TIMEOUT_MS) } {
        WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(PackageActivationLock(handle)),
        WAIT_TIMEOUT => {
            unsafe {
                CloseHandle(handle);
            }
            Err("timed out waiting for another Incodex Windows activation".to_string())
        }
        _ => {
            let error = std::io::Error::last_os_error();
            unsafe {
                CloseHandle(handle);
            }
            Err(format!(
                "cannot acquire the Windows package activation lock: {error}"
            ))
        }
    }
}

struct PackageActivationLock(HANDLE);

impl Drop for PackageActivationLock {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

pub fn try_run_package_debugger(arguments: &[String]) -> Option<Result<(), String>> {
    if arguments.first().map(String::as_str) != Some(PACKAGE_DEBUGGER_MODE) {
        return None;
    }
    let parsed = (|| {
        let job_name = flag_value(arguments, "--job")?;
        let package_full_name = flag_value(arguments, "--package")?;
        validate_text(package_full_name, "package full name")?;
        let process_id = flag_value(arguments, "-p")?
            .parse::<u32>()
            .map_err(|_| "Windows package debugger received an invalid process id".to_string())?;
        let thread_id = flag_value(arguments, "-tid")?
            .parse::<u32>()
            .map_err(|_| "Windows package debugger received an invalid thread id".to_string())?;
        let route = process_command_line(process_id)
            .ok()
            .map(|command_line| windows_transient_debugger_route(job_name, &command_line))
            .transpose()?
            .unwrap_or(WindowsDebuggerRoute::ResumeNormally);
        match route {
            WindowsDebuggerRoute::ResumeNormally => {
                resume_debugged_package_process(package_full_name, process_id, thread_id)
            }
            WindowsDebuggerRoute::AssignToJob(job_name) => {
                assign_debugged_process_to_job(&job_name, package_full_name, process_id, thread_id)
            }
        }
        .map_err(|error| format!("Windows package debugger failed: {error}"))
    })();
    Some(parsed)
}

pub fn try_run_installed_package_debugger(arguments: &[String]) -> Option<Result<(), String>> {
    if arguments.first().map(String::as_str) != Some(INSTALLED_DEBUGGER_MODE) {
        return None;
    }
    let parsed = (|| {
        let process_id = flag_value(arguments, "-p")?
            .parse::<u32>()
            .map_err(|_| "Windows installed debugger received an invalid process id".to_string())?;
        let thread_id = flag_value(arguments, "-tid")?
            .parse::<u32>()
            .map_err(|_| "Windows installed debugger received an invalid thread id".to_string())?;
        let registered_package = flag_value(arguments, "--package")?;
        let evidence = installed_debugger_registration_evidence(registered_package)?;
        let state = installed_state_for_current_helper(&evidence);
        let command_line = process_command_line(process_id).ok();
        let (package_full_name, route) = installed_debugger_route_from_state(
            state,
            &evidence.package_full_name,
            command_line.as_deref(),
        )?;
        match route {
            WindowsDebuggerRoute::ResumeNormally => {
                resume_debugged_package_process(&package_full_name, process_id, thread_id)
            }
            WindowsDebuggerRoute::AssignToJob(job_name) => {
                assign_debugged_process_to_job(&job_name, &package_full_name, process_id, thread_id)
            }
        }
        .map_err(|error| format!("Windows installed debugger failed: {error}"))
    })();
    Some(parsed)
}

fn flag_value<'a>(arguments: &'a [String], flag: &str) -> Result<&'a str, String> {
    arguments
        .windows(2)
        .find_map(|pair| (pair[0] == flag).then_some(pair[1].as_str()))
        .ok_or_else(|| format!("Windows package debugger is missing {flag}"))
}

pub fn transient_debugger_command_line(
    executable: &Path,
    job_name: &str,
    package_full_name: &str,
) -> Result<String, String> {
    if !executable.is_absolute() {
        return Err("Windows transient debugger helper path must be absolute".to_string());
    }
    let command = [
        executable.as_os_str().to_os_string(),
        OsString::from(PACKAGE_DEBUGGER_MODE),
        OsString::from("--job"),
        OsString::from(job_name),
        OsString::from("--package"),
        OsString::from(package_full_name),
    ]
    .iter()
    .map(|argument| quote_windows_argument(argument))
    .collect::<Result<Vec<_>, _>>()
    .map(|arguments| arguments.join(" "))?;
    validate_package_debugger_command(command)
}

fn validate_package_debugger_command(command: String) -> Result<String, String> {
    let utf16_units = command.encode_utf16().count();
    if utf16_units >= PACKAGE_DEBUGGER_COMMAND_LIMIT_UTF16 {
        return Err(format!(
            "Windows package debugger command uses {utf16_units} UTF-16 units; PackageDebugSettings requires fewer than {PACKAGE_DEBUGGER_COMMAND_LIMIT_UTF16} UTF-16 units"
        ));
    }
    Ok(command)
}

fn join_cleanup_error(primary: String, cleanup: Result<(), String>) -> String {
    match cleanup {
        Ok(()) => primary,
        Err(error) => format!("{primary}; {error}"),
    }
}

#[cfg(test)]
fn activation_manager_failure(
    primary: String,
    restoration: Result<(), String>,
) -> WindowsActivationFailure {
    activation_failure_after_debugging(primary, restoration, Ok(()))
}

#[cfg(test)]
fn activation_failure_after_debugging(
    primary: String,
    restoration: Result<(), String>,
    process_shutdown: Result<(), String>,
) -> WindowsActivationFailure {
    let shutdown = cleanup_proof_after_debugging(&restoration, process_shutdown);
    WindowsActivationFailure::after_start(join_cleanup_error(primary, restoration), shutdown)
}

fn cleanup_proof_after_debugging(
    restoration: &Result<(), String>,
    process_shutdown: Result<(), String>,
) -> Result<(), String> {
    let mut failures = Vec::new();
    if let Err(error) = restoration {
        failures.push(format!(
            "Windows package debug settings restoration is unproven: {error}"
        ));
    }
    if let Err(error) = process_shutdown {
        failures.push(error);
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn wide_nul(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn failed(result: HRESULT) -> bool {
    result < 0
}

fn hresult_message(action: &str, result: HRESULT) -> String {
    format!("{action}: HRESULT 0x{:08X}", result as u32)
}

struct ComApartment {
    uninitialize: bool,
}

impl ComApartment {
    fn initialize() -> Result<Self, String> {
        let result = unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        if !failed(result) {
            Ok(Self { uninitialize: true })
        } else if result == RPC_E_CHANGED_MODE {
            Ok(Self {
                uninitialize: false,
            })
        } else {
            Err(hresult_message(
                "cannot initialize COM for Windows package activation",
                result,
            ))
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.uninitialize {
            unsafe { CoUninitialize() };
        }
    }
}

struct PackageDebugGuard {
    interface: ComPtr,
    package: Vec<u16>,
    restoration: Option<PackageDebugRegistration>,
    active: bool,
}

struct PackageDebugRegistration {
    debugger_command: Vec<u16>,
    environment: Vec<u16>,
}

impl PackageDebugGuard {
    fn enable(
        package: &[u16],
        debugger_command: &[u16],
        environment: &[u16],
        restoration: Option<&WindowsInstalledRuntimeRegistration>,
    ) -> Result<Self, String> {
        let interface = ComPtr::create(
            &CLSID_PACKAGE_DEBUG_SETTINGS,
            &IID_PACKAGE_DEBUG_SETTINGS,
            "Windows package debug settings",
        )?;
        let vtable = unsafe { *(interface.raw() as *mut *const PackageDebugSettingsVtable) };
        let result = unsafe {
            ((*vtable).enable_debugging)(
                interface.raw(),
                package.as_ptr(),
                debugger_command.as_ptr(),
                environment.as_ptr(),
            )
        };
        if failed(result) {
            return Err(hresult_message(
                "cannot set the isolated Windows Codex environment",
                result,
            ));
        }
        Ok(Self {
            interface,
            package: package.to_vec(),
            restoration: restoration.map(|registration| PackageDebugRegistration {
                debugger_command: wide_nul(registration.debugger_command_line()),
                environment: registration.environment().to_vec(),
            }),
            active: true,
        })
    }

    fn restore(&mut self) -> Result<(), String> {
        if !self.active {
            return Ok(());
        }
        let vtable = unsafe { *(self.interface.raw() as *mut *const PackageDebugSettingsVtable) };
        let result = match &self.restoration {
            Some(registration) => unsafe {
                ((*vtable).enable_debugging)(
                    self.interface.raw(),
                    self.package.as_ptr(),
                    registration.debugger_command.as_ptr(),
                    registration.environment.as_ptr(),
                )
            },
            None => unsafe {
                ((*vtable).disable_debugging)(self.interface.raw(), self.package.as_ptr())
            },
        };
        if failed(result) {
            Err(hresult_message(
                "cannot restore Windows Codex package debug settings",
                result,
            ))
        } else {
            self.active = false;
            Ok(())
        }
    }
}

impl Drop for PackageDebugGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

struct ComPtr(*mut c_void);

impl ComPtr {
    fn create(clsid: &GUID, iid: &GUID, label: &str) -> Result<Self, String> {
        let mut raw = ptr::null_mut();
        let result = unsafe {
            CoCreateInstance(clsid, ptr::null_mut(), CLSCTX_INPROC_SERVER, iid, &mut raw)
        };
        if failed(result) || raw.is_null() {
            Err(hresult_message(&format!("cannot create {label}"), result))
        } else {
            Ok(Self(raw))
        }
    }

    fn raw(&self) -> *mut c_void {
        self.0
    }
}

impl Drop for ComPtr {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let vtable = *(self.0 as *mut *const UnknownVtable);
                ((*vtable).release)(self.0);
            }
        }
    }
}

#[repr(C)]
struct UnknownVtable {
    query_interface:
        unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct ApplicationActivationManagerVtable {
    base: UnknownVtable,
    activate_application:
        unsafe extern "system" fn(*mut c_void, *const u16, *const u16, u32, *mut u32) -> HRESULT,
}

#[repr(C)]
struct PackageDebugSettingsVtable {
    base: UnknownVtable,
    enable_debugging:
        unsafe extern "system" fn(*mut c_void, *const u16, *const u16, *const u16) -> HRESULT,
    disable_debugging: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
}

fn validate_text(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.contains('\0') {
        return Err(format!("Windows {label} is empty or contains NUL"));
    }
    Ok(())
}

fn environment_block(environment: BTreeMap<String, OsString>) -> Result<Vec<u16>, String> {
    let mut block = Vec::new();
    for (name, value) in environment {
        if name.is_empty() || name.contains(['=', '\0']) {
            return Err("Windows activation environment name is invalid".to_string());
        }
        let value: Vec<u16> = value.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(format!(
                "Windows activation environment value for {name} contains NUL"
            ));
        }
        block.extend(name.encode_utf16());
        block.push('=' as u16);
        block.extend(value);
        block.push(0);
    }
    block.push(0);
    if block.len() == 1 {
        block.push(0);
    }
    Ok(block)
}

fn quote_windows_argument(argument: &OsStr) -> Result<String, String> {
    let wide: Vec<u16> = argument.encode_wide().collect();
    if wide.contains(&0) {
        return Err("Windows activation argument contains NUL".to_string());
    }
    let needs_quotes =
        wide.is_empty() || wide.iter().any(|unit| matches!(*unit, 0x20 | 0x09 | 0x22));
    if !needs_quotes {
        return String::from_utf16(&wide)
            .map_err(|_| "Windows activation argument is not valid Unicode".to_string());
    }

    let mut quoted = Vec::with_capacity(wide.len() + 2);
    quoted.push(b'"' as u16);
    let mut backslashes = 0;
    for unit in wide {
        if unit == b'\\' as u16 {
            backslashes += 1;
            continue;
        }
        if unit == b'"' as u16 {
            quoted.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2 + 1));
        } else {
            quoted.extend(std::iter::repeat_n(b'\\' as u16, backslashes));
        }
        backslashes = 0;
        quoted.push(unit);
    }
    quoted.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2));
    quoted.push(b'"' as u16);
    String::from_utf16(&quoted)
        .map_err(|_| "Windows activation argument is not valid Unicode".to_string())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::Path;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::{
        acquire_package_activation_lock, activation_manager_failure, cleanup_proof_after_debugging,
        installed_debugger_route_from_state, installed_debugger_user_root, node_require_option,
        WindowsDebuggerRoute,
    };

    #[test]
    fn unreadable_installed_state_resumes_the_registered_package() {
        let package = "OpenAI.Codex_26.820.9563.0_x64__2p2nqsd0c76g0";
        let (selected_package, route) = installed_debugger_route_from_state(
            Err("fixture state is missing".to_string()),
            package,
            Some(r"ChatGPT.exe --user-data-dir=C:\Users\test\.incodex\sessions\s-one\chromium"),
        )
        .expect("fall back to the registered package evidence");

        assert_eq!(selected_package, package);
        assert_eq!(route, WindowsDebuggerRoute::ResumeNormally);
    }

    #[test]
    fn node_require_option_survives_a_spaced_windows_profile_path() {
        assert_eq!(
            node_require_option(Path::new(
                r"C:\Users\Linus Torvalds\.incodex\runtime\bootstrap.cjs"
            ))
            .expect("serialize Node preload"),
            OsString::from(r#"--require="C:/Users/Linus Torvalds/.incodex/runtime/bootstrap.cjs""#)
        );
    }

    #[test]
    fn installed_debugger_finds_short_and_legacy_private_roots() {
        let root = Path::new(r"C:\Users\test\.incodex");
        assert_eq!(
            installed_debugger_user_root(&root.join(r"windows\i\0123456789abcdef\i.exe"))
                .expect("short installed helper"),
            root
        );
        assert_eq!(
            installed_debugger_user_root(&root.join(
                r"windows\helpers\0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\incodex-helper.exe",
            ))
            .expect("legacy installed helper"),
            root
        );
        assert!(installed_debugger_user_root(&root.join(r"windows\i\not-hex\i.exe")).is_err());
    }

    #[test]
    fn manager_creation_failure_preserves_debug_restoration_uncertainty() {
        let failure = activation_manager_failure(
            "cannot create activation manager".to_string(),
            Err("cannot restore package debug settings".to_string()),
        );
        let (message, shutdown) = failure.into_parts();

        assert!(
            message.contains("cannot create activation manager"),
            "{message}"
        );
        assert!(
            message.contains("cannot restore package debug settings"),
            "{message}"
        );
        assert!(shutdown.is_err(), "session cleanup must remain blocked");
    }

    #[test]
    fn cleanup_requires_both_debug_restoration_and_process_shutdown_proof() {
        assert!(cleanup_proof_after_debugging(&Ok(()), Ok(())).is_ok());
        assert!(cleanup_proof_after_debugging(
            &Err("debug restoration failed".to_string()),
            Ok(())
        )
        .is_err());
        assert!(cleanup_proof_after_debugging(
            &Ok(()),
            Err("process shutdown is unproven".to_string())
        )
        .is_err());
    }

    #[test]
    fn package_debug_settings_are_serialized_across_incodex_processes() {
        let first = acquire_package_activation_lock().expect("acquire first package lock");
        let (sender, receiver) = mpsc::channel();
        let contender = thread::spawn(move || {
            let second = acquire_package_activation_lock().expect("acquire second package lock");
            sender.send(()).expect("report second lock");
            drop(second);
        });

        assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
        drop(first);
        receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("second activation proceeds after release");
        contender.join().expect("join lock contender");
    }
}
