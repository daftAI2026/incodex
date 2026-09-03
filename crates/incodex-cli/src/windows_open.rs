use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use incodex_core::windows_session::{
    burn_windows_session, copy_windows_settings, create_windows_session,
    sweep_orphan_windows_sessions, WindowsCleanupResult, WindowsSessionHome,
};
use incodex_core::{format_kv, format_ok, format_step, format_warn};

use crate::cdp::{
    allocate_debug_port, debug_launch_args,
    inject_shared_ui_with_options_while_alive_and_guard_with_readiness,
    is_terminal_codex_mode_error, start_lifecycle_signal_monitor,
    start_profile_mask_signal_monitor, CodexModeReadiness, InjectionOptions,
};
use crate::open_presentation::{
    classify_completed_open, completed_open_failure_message, CompletedOpenState,
    CLOSED_REMOVED_MESSAGE, DRY_RUN_COMPLETE, DRY_RUN_HEADING, OPENED_MESSAGE, OPENING_MESSAGE,
    REMOVING_SESSION_MESSAGE, UI_READY_WAIT_MESSAGE, WAITING_MESSAGE,
};
use crate::profile_mask::{resolve_profile_mask, ProfileMask};
use crate::windows_activation::{
    activate_packaged_kill_on_drop, activate_packaged_with_installed_cdp,
    disable_installed_runtime, enable_installed_runtime, WindowsActivationFailure,
    WindowsActivationRequest, WindowsInstalledRuntimeRegistration,
};
use crate::windows_app::{
    codex_package_full_name_is_installed, discover_codex_package, validate_codex_package_full_name,
    WindowsCodexApp,
};
use crate::windows_cleanup::cleanup_windows_session_after_shutdown;
use crate::windows_helper::publish_windows_transient_helper;
use crate::windows_install_state::{
    acquire_windows_install_state, read_windows_install_state, WindowsInstallPhase,
    WindowsInstallState,
};
use crate::windows_locale::read_locale_override;
#[cfg(test)]
use crate::windows_process::spawn_kill_on_drop;
use crate::windows_process::{
    running_package_process_ids, VisibleWindowLifecycle, WindowsCdpListenerStatus,
    WindowsCdpOwnershipGuard,
};
use crate::windows_registration::recover_transient_windows_debug_registration_with_restore;
use crate::windows_system::windows_path_for_display;
use crate::{parse::ParsedCli, CliFailure};

#[derive(Debug)]
pub struct WindowsOpenPlan {
    package_full_name: String,
    app_user_model_id: String,
    pub bin: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, PathBuf>,
    pub env_flags: BTreeMap<String, String>,
    pub session: WindowsSessionHome,
    pub debug_port: u16,
    pub injection: InjectionOptions,
    user_root: PathBuf,
    transient_helper: PathBuf,
}

impl WindowsOpenPlan {
    fn base_activation_request(&self) -> Result<WindowsActivationRequest, String> {
        let mut environment = BTreeMap::new();
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

    pub fn activation_request(&self) -> Result<WindowsActivationRequest, String> {
        self.base_activation_request()
            .and_then(|request| request.with_transient_cdp(&self.transient_helper, &self.user_root))
    }

    pub fn installed_activation_request(&self) -> Result<WindowsActivationRequest, String> {
        self.activation_request()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsOpenProcessResult {
    Closed,
    Exited(i32),
    SpawnFailed(String),
    ProcessStateUnknown(String),
    ListenerOwnershipFailed(String),
    InjectionFailed(String),
}

const LISTENER_SHUTDOWN_GRACE: Duration = Duration::from_millis(200);
const VISIBLE_WINDOW_CLOSE_GRACE: Duration = Duration::from_millis(250);
type WindowsMonitorWorkers = Vec<thread::JoinHandle<()>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsOpenOutcome {
    pub process: WindowsOpenProcessResult,
    pub ui_ready: bool,
    pub cleanup: WindowsCleanupResult,
    pub session_root: PathBuf,
}

pub fn installed_activation_for_open(
    state: Option<&WindowsInstallState>,
    expected_package_full_name: &str,
) -> Result<Option<WindowsInstalledRuntimeRegistration>, String> {
    validate_codex_package_full_name(expected_package_full_name)?;
    let Some(state) = state else {
        return Ok(None);
    };
    if state.package_full_name != expected_package_full_name {
        return Err(
            "Windows installed Runtime package does not match the current Store package"
                .to_string(),
        );
    }
    if !state.desired_enabled()
        || !matches!(
            state.phase,
            WindowsInstallPhase::EnabledUnobserved | WindowsInstallPhase::EnabledObserved
        )
    {
        return Err(format!(
            "Windows installed Runtime is in {:?}; finish install or uninstall before `incodex open`",
            state.phase
        ));
    }
    WindowsInstalledRuntimeRegistration::from_install_state(state).map(Some)
}

pub fn run_open(parsed: &ParsedCli) -> Result<(), CliFailure> {
    if parsed.app.is_some() {
        return Err(CliFailure::new(
            "Windows open discovers the current user's official Microsoft Store package; --app is not supported",
        ));
    }
    let app = discover_codex_package().map_err(CliFailure::from)?;
    let profile_mask = resolve_profile_mask(
        parsed.mask,
        parsed.name.as_deref(),
        parsed.avatar.as_deref().map(Path::new),
    )
    .map_err(CliFailure::from)?;
    if parsed.dry_run {
        println!("{}", format_step(DRY_RUN_HEADING, None));
        println!("{}", format_kv("Package", &app.package_full_name, None));
        println!(
            "{}",
            format_kv("Binary", &windows_path_for_display(&app.executable), None)
        );
        if let Some(mask) = &profile_mask {
            println!("{}", format_kv("Profile", &mask.name, None));
        }
        println!("{}", format_warn(DRY_RUN_COMPLETE, None));
        return Ok(());
    }
    let profile = crate::windows_profile::windows_user_profile().map_err(CliFailure::from)?;
    let source_home = std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| profile.join(".codex"));
    let user_root = profile.join(".incodex");
    let plan = prepare_windows_open(&app, &user_root, &source_home, profile_mask)
        .map_err(CliFailure::from)?;
    println!("{}", format_step(OPENING_MESSAGE, None));
    println!(
        "{}",
        format_kv("Binary", &windows_path_for_display(&plan.bin), None)
    );
    println!(
        "{}",
        format_kv("Home", &windows_path_for_display(&plan.session.home), None,)
    );
    println!("{}", format_kv("Session", &plan.session.session_id, None));
    let outcome = execute_windows_open_with(plan, launch_windows_open, inject_windows_ui);
    finish_windows_open(outcome)
}

pub fn prepare_windows_open(
    app: &WindowsCodexApp,
    user_root: &Path,
    source_home: &Path,
    profile_mask: Option<ProfileMask>,
) -> Result<WindowsOpenPlan, String> {
    let _ = sweep_orphan_windows_sessions(user_root);
    let session = create_windows_session(user_root)?;
    let prepared = (|| {
        copy_windows_settings(&session, source_home)?;
        let helper_source = std::env::current_exe()
            .map_err(|error| format!("cannot locate the running Incodex executable: {error}"))?;
        let transient_helper =
            publish_windows_transient_helper(user_root, &helper_source)?.executable;
        let debug_port = allocate_debug_port()?;
        let args = debug_launch_args(&session.chromium.display().to_string(), debug_port);
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
        ]);
        Ok(WindowsOpenPlan {
            package_full_name: app.package_full_name.clone(),
            app_user_model_id: app.app_user_model_id.clone(),
            bin: app.executable.clone(),
            args,
            env,
            env_flags,
            session: session.clone(),
            debug_port,
            injection: InjectionOptions {
                locale: read_locale_override(&session.home),
                profile_mask,
            },
            user_root: user_root.to_path_buf(),
            transient_helper,
        })
    })();

    match prepared {
        Ok(plan) => Ok(plan),
        Err(error) => match burn_windows_session(&session) {
            WindowsCleanupResult::Removed => Err(error),
            WindowsCleanupResult::Retained { reason } => Err(format!(
                "{error}; incomplete Windows session retained at {}: {reason}",
                windows_path_for_display(&session.root)
            )),
            WindowsCleanupResult::Unknown { reason } => Err(format!(
                "{error}; Windows session cleanup state is unknown at {}: {reason}",
                windows_path_for_display(&session.root)
            )),
        },
    }
}

fn execute_windows_open_with<L, F>(
    plan: WindowsOpenPlan,
    launch: L,
    inject: F,
) -> WindowsOpenOutcome
where
    L: FnOnce(
        &WindowsOpenPlan,
    ) -> Result<crate::windows_process::WindowsProcessTree, WindowsActivationFailure>,
    F: FnOnce(
            u16,
            InjectionOptions,
            Arc<AtomicBool>,
            Arc<AtomicBool>,
            Arc<AtomicBool>,
            Arc<WindowsCdpOwnershipGuard>,
        ) -> Result<WindowsMonitorWorkers, String>
        + Send
        + 'static,
{
    execute_windows_open_with_visibility(plan, launch, inject, |process_tree| {
        process_tree.has_visible_window()
    })
}

fn execute_windows_open_with_visibility<L, F, V>(
    plan: WindowsOpenPlan,
    launch: L,
    inject: F,
    has_visible_window: V,
) -> WindowsOpenOutcome
where
    L: FnOnce(
        &WindowsOpenPlan,
    ) -> Result<crate::windows_process::WindowsProcessTree, WindowsActivationFailure>,
    F: FnOnce(
            u16,
            InjectionOptions,
            Arc<AtomicBool>,
            Arc<AtomicBool>,
            Arc<AtomicBool>,
            Arc<WindowsCdpOwnershipGuard>,
        ) -> Result<WindowsMonitorWorkers, String>
        + Send
        + 'static,
    V: FnMut(&crate::windows_process::WindowsProcessTree) -> std::io::Result<bool>,
{
    let process_tree = match launch(&plan) {
        Ok(process_tree) => process_tree,
        Err(error) => {
            let (message, shutdown) = error.into_parts();
            return WindowsOpenOutcome {
                process: WindowsOpenProcessResult::SpawnFailed(message),
                ui_ready: false,
                cleanup: cleanup_windows_session_with_progress(&plan.session, shutdown),
                session_root: plan.session.root.clone(),
            };
        }
    };
    run_windows_open_lifecycle(plan, process_tree, inject, has_visible_window)
}

fn launch_windows_open(
    plan: &WindowsOpenPlan,
) -> Result<crate::windows_process::WindowsProcessTree, WindowsActivationFailure> {
    let _launch_gate =
        acquire_windows_install_state().map_err(WindowsActivationFailure::before_start)?;
    recover_transient_windows_debug_registration_with_restore(
        &plan.user_root,
        running_package_process_ids,
        codex_package_full_name_is_installed,
        disable_installed_runtime,
        |state| {
            WindowsInstalledRuntimeRegistration::from_install_state(state)
                .and_then(|registration| enable_installed_runtime(&registration))
        },
    )
    .map_err(WindowsActivationFailure::before_start)?;
    let installed_state = read_windows_install_state(&plan.user_root)
        .map_err(WindowsActivationFailure::before_start)?;
    let installed_activation =
        installed_activation_for_open(installed_state.as_ref(), &plan.package_full_name)
            .map_err(WindowsActivationFailure::before_start)?;
    let request = if installed_activation.is_some() {
        plan.installed_activation_request()
    } else {
        plan.activation_request()
    }
    .map_err(WindowsActivationFailure::before_start)?;
    match installed_activation.as_ref() {
        Some(registration) => activate_packaged_with_installed_cdp(&request, registration),
        None => activate_packaged_kill_on_drop(&request),
    }
}

fn run_windows_open_lifecycle<F, V>(
    plan: WindowsOpenPlan,
    mut process_tree: crate::windows_process::WindowsProcessTree,
    inject: F,
    mut has_visible_window: V,
) -> WindowsOpenOutcome
where
    F: FnOnce(
            u16,
            InjectionOptions,
            Arc<AtomicBool>,
            Arc<AtomicBool>,
            Arc<AtomicBool>,
            Arc<WindowsCdpOwnershipGuard>,
        ) -> Result<WindowsMonitorWorkers, String>
        + Send
        + 'static,
    V: FnMut(&crate::windows_process::WindowsProcessTree) -> std::io::Result<bool>,
{
    let mut spinner = crate::spinner::Spinner::start(UI_READY_WAIT_MESSAGE);
    let ownership_guard = match wait_for_owned_listener(&mut process_tree, plan.debug_port) {
        Ok(guard) => Arc::new(guard),
        Err(error) => {
            let (process, shutdown) = terminate_with_outcome(
                &mut process_tree,
                WindowsOpenProcessResult::ListenerOwnershipFailed(error),
                "Windows CDP listener ownership failed before injection",
            );
            drop(process_tree);
            spinner.stop();
            return WindowsOpenOutcome {
                process,
                ui_ready: false,
                cleanup: cleanup_windows_session_with_progress(&plan.session, shutdown),
                session_root: plan.session.root.clone(),
            };
        }
    };
    let alive = Arc::new(AtomicBool::new(true));
    let injection_alive = alive.clone();
    let close_requested = Arc::new(AtomicBool::new(false));
    let injection_close_requested = close_requested.clone();
    let cdp_failed = Arc::new(AtomicBool::new(false));
    let injection_cdp_failed = cdp_failed.clone();
    let debug_port = plan.debug_port;
    let options = plan.injection.clone();
    let injection_ownership_guard = ownership_guard.clone();
    let (injection_tx, injection_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let _ = injection_tx.send(inject(
            debug_port,
            options,
            injection_alive,
            injection_close_requested,
            injection_cdp_failed,
            injection_ownership_guard,
        ));
    });

    let mut ui_ready = false;
    let mut monitor_workers = Vec::new();
    let mut listener_missing_since = None;
    let mut window_lifecycle = VisibleWindowLifecycle::new(VISIBLE_WINDOW_CLOSE_GRACE);
    let (process, shutdown) = loop {
        match injection_rx.try_recv() {
            Ok(Ok(workers)) => {
                monitor_workers = workers;
                if !ui_ready {
                    spinner.stop();
                    println!("{}", format_ok(OPENED_MESSAGE, None));
                    let _ = std::io::stdout().flush();
                    spinner = crate::spinner::Spinner::start(WAITING_MESSAGE);
                    ui_ready = true;
                }
            }
            Ok(Err(error)) => {
                break terminate_with_outcome(
                    &mut process_tree,
                    WindowsOpenProcessResult::InjectionFailed(error),
                    "Windows CDP injection failed",
                );
            }
            Err(mpsc::TryRecvError::Disconnected) if !ui_ready => {
                break terminate_with_outcome(
                    &mut process_tree,
                    WindowsOpenProcessResult::InjectionFailed(
                        "Windows CDP injection worker disconnected".to_string(),
                    ),
                    "Windows CDP injection worker disconnected",
                );
            }
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => {}
        }
        if ui_ready && close_requested.load(Ordering::Acquire) {
            match process_tree.terminate_successfully() {
                Ok(_) => break (WindowsOpenProcessResult::Closed, Ok(())),
                Err(error) => {
                    let reason = format!(
                        "cannot prove Windows Job shutdown after the primary window closed: {error}"
                    );
                    break (
                        WindowsOpenProcessResult::ProcessStateUnknown(reason.clone()),
                        Err(reason),
                    );
                }
            }
        }
        match has_visible_window(&process_tree) {
            Ok(visible) if window_lifecycle.should_close(visible, Instant::now()) => {
                match process_tree.terminate_successfully() {
                    Ok(_) => break (WindowsOpenProcessResult::Closed, Ok(())),
                    Err(error) => {
                        let reason = format!(
                            "cannot prove Windows Job shutdown after its visible window closed: {error}"
                        );
                        break (
                            WindowsOpenProcessResult::ProcessStateUnknown(reason.clone()),
                            Err(reason),
                        );
                    }
                }
            }
            Ok(_) => {}
            Err(error) => {
                break terminate_with_outcome(
                    &mut process_tree,
                    WindowsOpenProcessResult::ProcessStateUnknown(format!(
                        "cannot inspect isolated Windows window state: {error}"
                    )),
                    "Windows visible-window state query failed",
                );
            }
        }
        if ui_ready && cdp_failed.load(Ordering::Acquire) {
            break terminate_with_outcome(
                &mut process_tree,
                WindowsOpenProcessResult::InjectionFailed(
                    "Windows CDP lifecycle became unavailable".to_string(),
                ),
                "Windows CDP lifecycle became unavailable",
            );
        }
        match process_tree.try_wait() {
            Ok(Some(status)) => {
                break (
                    WindowsOpenProcessResult::Exited(status.code().unwrap_or(1)),
                    Ok(()),
                );
            }
            Ok(None) => {}
            Err(error) => {
                break terminate_with_outcome(
                    &mut process_tree,
                    WindowsOpenProcessResult::ProcessStateUnknown(error.to_string()),
                    "Windows process state query failed",
                );
            }
        }
        match ownership_guard.listener_status() {
            Ok(WindowsCdpListenerStatus::Owned) => listener_missing_since = None,
            Ok(WindowsCdpListenerStatus::Missing) => {
                let missing_since = listener_missing_since.get_or_insert_with(Instant::now);
                if missing_since.elapsed() >= LISTENER_SHUTDOWN_GRACE {
                    let error = format!(
                        "Windows CDP listener 127.0.0.1:{} disappeared while Codex remained active",
                        plan.debug_port
                    );
                    break terminate_with_outcome(
                        &mut process_tree,
                        WindowsOpenProcessResult::ListenerOwnershipFailed(error),
                        "Windows CDP listener disappeared while Codex remained active",
                    );
                }
            }
            Ok(WindowsCdpListenerStatus::Foreign) => {
                let error =
                    "Windows CDP listener owner moved outside the isolated Job Object".to_string();
                break terminate_with_outcome(
                    &mut process_tree,
                    WindowsOpenProcessResult::ListenerOwnershipFailed(error),
                    "Windows CDP listener ownership moved outside the isolated Job Object",
                );
            }
            Err(error) => {
                break terminate_with_outcome(
                    &mut process_tree,
                    WindowsOpenProcessResult::ListenerOwnershipFailed(error),
                    "Windows CDP listener ownership query failed",
                );
            }
        }
        thread::sleep(Duration::from_millis(25));
    };

    spinner.stop();
    alive.store(false, Ordering::Release);
    let _ = worker.join();
    if monitor_workers.is_empty() {
        if let Ok(Ok(workers)) = injection_rx.try_recv() {
            monitor_workers = workers;
        }
    }
    for monitor in monitor_workers {
        let _ = monitor.join();
    }
    drop(ownership_guard);
    drop(process_tree);
    let cleanup = cleanup_windows_session_with_progress(&plan.session, shutdown);
    WindowsOpenOutcome {
        process,
        ui_ready,
        cleanup,
        session_root: plan.session.root,
    }
}

fn cleanup_windows_session_with_progress(
    session: &WindowsSessionHome,
    shutdown: Result<(), String>,
) -> WindowsCleanupResult {
    let mut spinner = crate::spinner::Spinner::start(REMOVING_SESSION_MESSAGE);
    let cleanup = cleanup_windows_session_after_shutdown(session, shutdown);
    spinner.stop();
    cleanup
}

fn terminate_after_failure(
    process_tree: &mut crate::windows_process::WindowsProcessTree,
    context: &str,
) -> Result<(), String> {
    process_tree.terminate().map(|_| ()).map_err(|error| {
        format!("{context}; cannot prove the isolated Windows Job is empty: {error}")
    })
}

fn terminate_with_outcome(
    process_tree: &mut crate::windows_process::WindowsProcessTree,
    outcome: WindowsOpenProcessResult,
    context: &str,
) -> (WindowsOpenProcessResult, Result<(), String>) {
    (outcome, terminate_after_failure(process_tree, context))
}

fn wait_for_owned_listener(
    process_tree: &mut crate::windows_process::WindowsProcessTree,
    port: u16,
) -> Result<WindowsCdpOwnershipGuard, String> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match process_tree.cdp_ownership_guard(port) {
            Ok(Some(guard)) => return Ok(guard),
            Ok(None) => {}
            Err(error) => return Err(format!("cannot prove Windows CDP listener owner: {error}")),
        }
        match process_tree.try_wait() {
            Ok(Some(status)) => {
                return Err(format!(
                    "Codex exited with {status} before its owned CDP listener was ready"
                ))
            }
            Ok(None) => {}
            Err(error) => {
                return Err(format!(
                    "cannot inspect Codex while proving CDP listener ownership: {error}"
                ))
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out proving that 127.0.0.1:{port} belongs to the Codex Job Object"
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn inject_windows_ui(
    port: u16,
    options: InjectionOptions,
    alive: Arc<AtomicBool>,
    close_requested: Arc<AtomicBool>,
    cdp_failed: Arc<AtomicBool>,
    ownership_guard: Arc<WindowsCdpOwnershipGuard>,
) -> Result<WindowsMonitorWorkers, String> {
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut last_error = "Codex CDP page is not ready".to_string();
    let mut mode_readiness = CodexModeReadiness::default();
    while alive.load(Ordering::Acquire) && Instant::now() < deadline {
        let mut primary_target = None;
        match inject_shared_ui_with_options_while_alive_and_guard_with_readiness(
            port,
            &options,
            &alive,
            |target_id| {
                primary_target = Some(target_id.to_string());
            },
            &mut mode_readiness,
            &|stream| ownership_guard.require_connection_owner(stream),
        ) {
            Ok(_) => {
                let mut monitor_workers = Vec::with_capacity(2);
                if let Some(target_id) = primary_target {
                    if options.profile_mask.is_some() {
                        let mask_ownership_guard = ownership_guard.clone();
                        monitor_workers.push(start_profile_mask_signal_monitor(
                            port,
                            alive.clone(),
                            cdp_failed.clone(),
                            move |stream| mask_ownership_guard.require_connection_owner(stream),
                        ));
                    }
                    monitor_workers.push(start_lifecycle_signal_monitor(
                        port,
                        target_id,
                        alive.clone(),
                        close_requested,
                        cdp_failed,
                    ));
                }
                return Ok(monitor_workers);
            }
            Err(error) if is_terminal_codex_mode_error(&error) => return Err(error),
            Err(error) => last_error = error,
        }
        thread::sleep(Duration::from_millis(400));
    }
    Err(format!("Windows UI injection failed: {last_error}"))
}

fn finish_windows_open(outcome: WindowsOpenOutcome) -> Result<(), CliFailure> {
    let cleanup_ok = match &outcome.cleanup {
        WindowsCleanupResult::Removed => {
            crate::terminal_presentation::print_terminal_result(&format_ok(
                CLOSED_REMOVED_MESSAGE,
                None,
            ));
            true
        }
        WindowsCleanupResult::Retained { reason } => {
            crate::terminal_presentation::print_terminal_result(&format_warn(
                &format!(
                    "Closed. Isolated session retained at {}: {reason}",
                    windows_path_for_display(&outcome.session_root)
                ),
                None,
            ));
            false
        }
        WindowsCleanupResult::Unknown { reason } => {
            crate::terminal_presentation::print_terminal_result(&format_warn(
                &format!(
                    "Window state unknown; isolated session retained for safety at {}: {reason}",
                    windows_path_for_display(&outcome.session_root)
                ),
                None,
            ));
            false
        }
    };
    if !cleanup_ok {
        return Err(CliFailure::with_code(2, ""));
    }
    match outcome.process {
        WindowsOpenProcessResult::Closed => Ok(()),
        WindowsOpenProcessResult::Exited(code) => {
            let state = classify_completed_open(code, outcome.ui_ready);
            match state {
                CompletedOpenState::Success => Ok(()),
                CompletedOpenState::ProcessFailure => {
                    Err(CliFailure::new(completed_open_failure_message(code, state)))
                }
                CompletedOpenState::UiInjectionFailure => Err(CliFailure::with_code(
                    3,
                    completed_open_failure_message(code, state),
                )),
            }
        }
        WindowsOpenProcessResult::SpawnFailed(error)
        | WindowsOpenProcessResult::ProcessStateUnknown(error) => Err(CliFailure::new(error)),
        WindowsOpenProcessResult::ListenerOwnershipFailed(error)
        | WindowsOpenProcessResult::InjectionFailed(error) => Err(CliFailure::with_code(3, error)),
    }
}

#[cfg(test)]
#[path = "windows_open_tests.rs"]
mod tests;
