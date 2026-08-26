use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use incodex_core::windows_path::require_local_disk_absolute;
use incodex_core::windows_session::{
    apply_private_windows_acl, burn_windows_session, copy_windows_settings, create_windows_session,
    sweep_orphan_windows_sessions, verify_private_acl, WindowsCleanupResult, WindowsSessionHome,
};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use crate::cdp::OFFICIAL_NEW_CODEX_URL;
use crate::windows_app::{discover_codex_package, WindowsCodexApp};
use crate::windows_cleanup::cleanup_windows_session_after_shutdown;
use crate::windows_install_state::{
    read_windows_install_state, WindowsInstallPhase, WindowsInstallState,
};
use crate::windows_process::{spawn_kill_on_drop, WindowsProcessTree};

const RUNTIME_OPEN_MODE: &str = "__incodex_windows_runtime_open";
const READY_TIMEOUT: Duration = Duration::from_secs(30);
const VISIBLE_APPEAR_TIMEOUT: Duration = Duration::from_secs(15);
const VISIBLE_CLOSE_GRACE: Duration = Duration::from_millis(250);
const POLL_INTERVAL: Duration = Duration::from_millis(25);
const READY_FILE_LIMIT: u64 = 64;

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
    pub env_remove: Vec<String>,
    pub session: WindowsSessionHome,
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
            env_remove: vec!["INCODEX_WINDOWS_BOOTSTRAPPED".to_string()],
            session: session.clone(),
        })
    })();
    match prepared {
        Ok(plan) => Ok(plan),
        Err(error) => Err(with_prepare_cleanup(error, &session)),
    }
}

fn run_windows_runtime_open(request: WindowsRuntimeOpenRequest) -> Result<(), String> {
    let state = installed_state_from_environment()?;
    let app = discover_codex_package()?;
    if app.package_full_name != state.package_full_name {
        return Err("installed Windows Runtime package identity changed".to_string());
    }
    let user_root = state
        .state_path
        .parent()
        .ok_or_else(|| "Windows install state has no parent directory".to_string())?;
    let plan = prepare_windows_runtime_open(
        &app,
        user_root,
        &request.source_home,
        request.source_bounds.as_deref(),
    )?;
    execute_windows_runtime_open(plan)
}

fn execute_windows_runtime_open(plan: WindowsRuntimeOpenPlan) -> Result<(), String> {
    let mut command = Command::new(&plan.bin);
    command.args(&plan.args);
    for (key, value) in &plan.env {
        command.env(key, value);
    }
    for (key, value) in &plan.env_flags {
        command.env(key, value);
    }
    for key in &plan.env_remove {
        command.env_remove(key);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let process_tree = match spawn_kill_on_drop(&mut command) {
        Ok(process_tree) => process_tree,
        Err(error) => {
            return Err(with_shutdown_cleanup(
                format!("cannot launch installed Windows Runtime: {error}"),
                &plan.session,
                Ok(()),
            ))
        }
    };
    run_guardian_lifecycle(plan.session, process_tree)
}

fn run_guardian_lifecycle(
    session: WindowsSessionHome,
    mut process_tree: WindowsProcessTree,
) -> Result<(), String> {
    let ready_deadline = Instant::now() + READY_TIMEOUT;
    loop {
        match process_tree.try_wait() {
            Ok(Some(status)) => {
                return Err(with_shutdown_cleanup(
                    format!("Codex exited with {status} before the shared Runtime was ready"),
                    &session,
                    Ok(()),
                ))
            }
            Ok(None) => {}
            Err(error) => {
                return guardian_failure(
                    format!("cannot inspect installed Windows Runtime process: {error}"),
                    &session,
                    &mut process_tree,
                )
            }
        }
        match validate_windows_runtime_ready(&session) {
            Ok(true) => break,
            Ok(false) => {}
            Err(error) => return guardian_failure(error, &session, &mut process_tree),
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

    let visible_deadline = Instant::now() + VISIBLE_APPEAR_TIMEOUT;
    let mut seen_visible = false;
    let mut missing_since = None;
    loop {
        match process_tree.try_wait() {
            Ok(Some(_)) => return finish_guardian(&session, Ok(())),
            Ok(None) => {}
            Err(error) => {
                return guardian_failure(
                    format!("cannot inspect installed Windows Runtime process: {error}"),
                    &session,
                    &mut process_tree,
                )
            }
        }
        match process_tree.has_visible_window() {
            Ok(true) => {
                seen_visible = true;
                missing_since = None;
            }
            Ok(false) if seen_visible => {
                let missing = missing_since.get_or_insert_with(Instant::now);
                if missing.elapsed() >= VISIBLE_CLOSE_GRACE {
                    let shutdown = process_tree.terminate_successfully().map(|_| ()).map_err(
                        |error| {
                            format!(
                                "cannot prove Windows Runtime shutdown after its window closed: {error}"
                            )
                        },
                    );
                    return finish_guardian(&session, shutdown);
                }
            }
            Ok(false) if Instant::now() >= visible_deadline => {
                return guardian_failure(
                    "shared Windows Runtime became ready without a visible Codex window"
                        .to_string(),
                    &session,
                    &mut process_tree,
                )
            }
            Ok(false) => {}
            Err(error) => {
                return guardian_failure(
                    format!("cannot inspect the installed Windows Runtime window: {error}"),
                    &session,
                    &mut process_tree,
                )
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
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

pub fn validate_windows_runtime_ready(session: &WindowsSessionHome) -> Result<bool, String> {
    let path = session.root.join("ready");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("cannot inspect Windows Runtime readiness: {error}")),
    };
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.len() > READY_FILE_LIMIT
    {
        return Err("Windows Runtime readiness marker is unsafe".to_string());
    }
    apply_private_windows_acl(&path)?;
    verify_private_acl(&path)?;
    let body = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read Windows Runtime readiness: {error}"))?;
    body.trim()
        .parse::<u64>()
        .map(|_| true)
        .map_err(|_| "Windows Runtime readiness marker is invalid".to_string())
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
