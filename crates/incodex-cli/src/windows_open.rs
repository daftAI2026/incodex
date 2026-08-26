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
    allocate_debug_port, debug_launch_args, inject_shared_ui_with_options_while_alive,
    start_lifecycle_signal_monitor, InjectionOptions,
};
use crate::profile_mask::{resolve_profile_mask, ProfileMask};
use crate::windows_activation::{activate_packaged_kill_on_drop, WindowsActivationRequest};
use crate::windows_app::{discover_codex_package, WindowsCodexApp};
#[cfg(test)]
use crate::windows_process::spawn_kill_on_drop;
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
}

impl WindowsOpenPlan {
    pub fn activation_request(&self) -> Result<WindowsActivationRequest, String> {
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsOpenProcessResult {
    Exited(i32),
    SpawnFailed(String),
    ProcessStateUnknown(String),
    ListenerOwnershipFailed(String),
    InjectionFailed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsOpenOutcome {
    pub process: WindowsOpenProcessResult,
    pub ui_ready: bool,
    pub cleanup: WindowsCleanupResult,
}

pub fn run_open(parsed: &ParsedCli) -> Result<(), CliFailure> {
    if parsed.app.is_some() {
        return Err(CliFailure::new(
            "Windows open discovers the current user's official Microsoft Store package; --app is not supported",
        ));
    }
    let app = discover_codex_package().map_err(CliFailure::from)?;
    if parsed.dry_run {
        println!(
            "{}",
            format_step("Open incognito without patching Codex", None)
        );
        println!("{}", format_kv("Package", &app.package_full_name, None));
        println!(
            "{}",
            format_kv("Binary", &app.executable.display().to_string(), None)
        );
        println!("{}", format_warn("Dry run. No window opened.", None));
        return Ok(());
    }
    let profile = crate::windows_profile::windows_user_profile().map_err(CliFailure::from)?;
    let source_home = std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| profile.join(".codex"));
    let profile_mask = resolve_profile_mask(
        parsed.mask,
        parsed.name.as_deref(),
        parsed.avatar.as_deref().map(Path::new),
    )
    .map_err(CliFailure::from)?;
    let plan = prepare_windows_open(&app, &profile.join(".incodex"), &source_home, profile_mask)
        .map_err(CliFailure::from)?;
    println!("{}", format_step("Opening incognito Codex window", None));
    println!(
        "{}",
        format_kv("Binary", &plan.bin.display().to_string(), None)
    );
    println!(
        "{}",
        format_kv("Home", &plan.session.home.display().to_string(), None)
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
                locale: read_locale_override(source_home),
                profile_mask,
            },
        })
    })();

    match prepared {
        Ok(plan) => Ok(plan),
        Err(error) => match burn_windows_session(&session) {
            WindowsCleanupResult::Removed => Err(error),
            WindowsCleanupResult::Retained { reason } => Err(format!(
                "{error}; incomplete Windows session retained at {}: {reason}",
                session.root.display()
            )),
            WindowsCleanupResult::Unknown { reason } => Err(format!(
                "{error}; Windows session cleanup state is unknown at {}: {reason}",
                session.root.display()
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
    L: FnOnce(&WindowsOpenPlan) -> Result<crate::windows_process::WindowsProcessTree, String>,
    F: FnOnce(
            u16,
            InjectionOptions,
            Arc<AtomicBool>,
            Arc<AtomicBool>,
            Arc<AtomicBool>,
        ) -> Result<(), String>
        + Send
        + 'static,
{
    let process_tree = match launch(&plan) {
        Ok(process_tree) => process_tree,
        Err(error) => {
            return WindowsOpenOutcome {
                process: WindowsOpenProcessResult::SpawnFailed(error),
                ui_ready: false,
                cleanup: cleanup_windows_session(&plan.session),
            }
        }
    };
    run_windows_open_lifecycle(plan, process_tree, inject)
}

fn launch_windows_open(
    plan: &WindowsOpenPlan,
) -> Result<crate::windows_process::WindowsProcessTree, String> {
    let request = plan.activation_request()?;
    activate_packaged_kill_on_drop(&request)
}

fn run_windows_open_lifecycle<F>(
    plan: WindowsOpenPlan,
    mut process_tree: crate::windows_process::WindowsProcessTree,
    inject: F,
) -> WindowsOpenOutcome
where
    F: FnOnce(
            u16,
            InjectionOptions,
            Arc<AtomicBool>,
            Arc<AtomicBool>,
            Arc<AtomicBool>,
        ) -> Result<(), String>
        + Send
        + 'static,
{
    if let Err(error) = wait_for_owned_listener(&mut process_tree, plan.debug_port) {
        let _ = process_tree.terminate();
        drop(process_tree);
        return WindowsOpenOutcome {
            process: WindowsOpenProcessResult::ListenerOwnershipFailed(error),
            ui_ready: false,
            cleanup: cleanup_windows_session(&plan.session),
        };
    }
    let alive = Arc::new(AtomicBool::new(true));
    let injection_alive = alive.clone();
    let close_requested = Arc::new(AtomicBool::new(false));
    let injection_close_requested = close_requested.clone();
    let cdp_failed = Arc::new(AtomicBool::new(false));
    let injection_cdp_failed = cdp_failed.clone();
    let debug_port = plan.debug_port;
    let options = plan.injection.clone();
    let (injection_tx, injection_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let _ = injection_tx.send(inject(
            debug_port,
            options,
            injection_alive,
            injection_close_requested,
            injection_cdp_failed,
        ));
    });

    let mut ui_ready = false;
    let process = loop {
        match injection_rx.try_recv() {
            Ok(Ok(())) => ui_ready = true,
            Ok(Err(error)) => {
                let _ = process_tree.terminate();
                break WindowsOpenProcessResult::InjectionFailed(error);
            }
            Err(mpsc::TryRecvError::Disconnected) if !ui_ready => {
                let _ = process_tree.terminate();
                break WindowsOpenProcessResult::InjectionFailed(
                    "Windows CDP injection worker disconnected".to_string(),
                );
            }
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => {}
        }
        if ui_ready && cdp_failed.load(Ordering::Acquire) {
            let _ = process_tree.terminate();
            break WindowsOpenProcessResult::InjectionFailed(
                "Windows CDP lifecycle became unavailable".to_string(),
            );
        }
        if ui_ready && close_requested.load(Ordering::Acquire) {
            match process_tree.terminate_successfully() {
                Ok(status) => {
                    break WindowsOpenProcessResult::Exited(status.code().unwrap_or(0));
                }
                Err(error) => {
                    break WindowsOpenProcessResult::ProcessStateUnknown(error.to_string());
                }
            }
        }
        match process_tree.try_wait() {
            Ok(Some(status)) => {
                break WindowsOpenProcessResult::Exited(status.code().unwrap_or(1));
            }
            Ok(None) => {}
            Err(error) => {
                let _ = process_tree.terminate();
                break WindowsOpenProcessResult::ProcessStateUnknown(error.to_string());
            }
        }
        thread::sleep(Duration::from_millis(25));
    };

    alive.store(false, Ordering::Release);
    let _ = worker.join();
    drop(process_tree);
    WindowsOpenOutcome {
        process,
        ui_ready,
        cleanup: cleanup_windows_session(&plan.session),
    }
}

fn cleanup_windows_session(session: &WindowsSessionHome) -> WindowsCleanupResult {
    let mut last = WindowsCleanupResult::Unknown {
        reason: "Windows session cleanup was not attempted".to_string(),
    };
    for attempt in 1..=5 {
        last = burn_windows_session(session);
        if last == WindowsCleanupResult::Removed {
            return last;
        }
        if attempt < 5 {
            thread::sleep(Duration::from_millis(200 * attempt));
        }
    }
    last
}

fn wait_for_owned_listener(
    process_tree: &mut crate::windows_process::WindowsProcessTree,
    port: u16,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match process_tree.listener_owner_is_in_job(port) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
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
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut last_error = "Codex CDP page is not ready".to_string();
    while alive.load(Ordering::Acquire) && Instant::now() < deadline {
        let mut primary_target = None;
        match inject_shared_ui_with_options_while_alive(port, &options, &alive, |target_id| {
            primary_target = Some(target_id.to_string());
        }) {
            Ok(_) => {
                if let Some(target_id) = primary_target {
                    start_lifecycle_signal_monitor(
                        port,
                        target_id,
                        alive.clone(),
                        close_requested,
                        cdp_failed,
                    );
                }
                println!(
                    "{}",
                    format_ok("Opened. Incognito Codex window is ready.", None)
                );
                let _ = std::io::stdout().flush();
                return Ok(());
            }
            Err(error) => last_error = error,
        }
        thread::sleep(Duration::from_millis(400));
    }
    Err(format!("Windows UI injection failed: {last_error}"))
}

fn finish_windows_open(outcome: WindowsOpenOutcome) -> Result<(), CliFailure> {
    match &outcome.cleanup {
        WindowsCleanupResult::Removed => {
            println!("{}", format_ok("Closed. Isolated session removed.", None));
        }
        WindowsCleanupResult::Retained { reason } => {
            println!(
                "{}",
                format_warn(
                    &format!("Closed. Isolated session retained: {reason}"),
                    None
                )
            );
            return Err(CliFailure::with_code(2, ""));
        }
        WindowsCleanupResult::Unknown { reason } => {
            println!(
                "{}",
                format_warn(
                    &format!("Closed. Isolated session cleanup is unknown: {reason}"),
                    None,
                )
            );
            return Err(CliFailure::with_code(2, ""));
        }
    }
    match outcome.process {
        WindowsOpenProcessResult::Exited(0) if outcome.ui_ready => Ok(()),
        WindowsOpenProcessResult::Exited(code) => Err(CliFailure::new(format!(
            "Incognito Codex process exited with status {code}"
        ))),
        WindowsOpenProcessResult::SpawnFailed(error)
        | WindowsOpenProcessResult::ProcessStateUnknown(error) => Err(CliFailure::new(error)),
        WindowsOpenProcessResult::ListenerOwnershipFailed(error)
        | WindowsOpenProcessResult::InjectionFailed(error) => Err(CliFailure::with_code(3, error)),
    }
}

fn read_locale_override(source_home: &Path) -> Option<String> {
    let content = std::fs::read_to_string(source_home.join("config.toml")).ok()?;
    content.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        if name.trim() != "localeOverride" {
            return None;
        }
        let value = value.trim();
        value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                value
                    .strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
            })
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::net::{Ipv4Addr, TcpListener};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn plan() -> (PathBuf, WindowsOpenPlan) {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "incodex-windows-open-lifecycle-{}-{sequence}",
            std::process::id()
        ));
        let profile = root.join("profile");
        let source = profile.join(".codex");
        fs::create_dir_all(&source).expect("create source");
        fs::write(source.join("auth.json"), b"fixture").expect("write auth");
        let executable = std::env::current_exe().expect("test executable");
        let app = WindowsCodexApp {
            package_full_name: "OpenAI.Codex_fixture_x64__2p2nqsd0c76g0".to_string(),
            app_user_model_id: "OpenAI.Codex_2p2nqsd0c76g0!App".to_string(),
            install_location: root.join("package"),
            executable: executable.clone(),
            architecture: "X64".to_string(),
        };
        let mut plan = prepare_windows_open(&app, &profile.join(".incodex"), &source, None)
            .expect("prepare lifecycle plan");
        plan.args = vec![
            "windows_open::tests::open_process_fixture".to_string(),
            "--exact".to_string(),
            "--nocapture".to_string(),
        ];
        plan.env_flags
            .insert("INCODEX_WINDOWS_OPEN_FIXTURE".to_string(), "1".to_string());
        plan.env_flags.insert(
            "INCODEX_WINDOWS_OPEN_PORT".to_string(),
            plan.debug_port.to_string(),
        );
        (root, plan)
    }

    #[test]
    fn open_process_fixture() {
        if std::env::var_os("INCODEX_WINDOWS_OPEN_FIXTURE").is_none() {
            return;
        }
        let port = std::env::var("INCODEX_WINDOWS_OPEN_PORT")
            .expect("fixture port")
            .parse::<u16>()
            .expect("valid fixture port");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).expect("bind fixture CDP");
        thread::sleep(Duration::from_millis(250));
        if std::env::var_os("INCODEX_WINDOWS_OPEN_DROP_LISTENER").is_some() {
            drop(listener);
            thread::sleep(Duration::from_secs(2));
        }
    }

    fn launch_fixture(
        plan: &WindowsOpenPlan,
    ) -> Result<crate::windows_process::WindowsProcessTree, String> {
        let mut command = Command::new(&plan.bin);
        command.args(&plan.args);
        for (key, value) in &plan.env {
            command.env(key, value);
        }
        for (key, value) in &plan.env_flags {
            command.env(key, value);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        spawn_kill_on_drop(&mut command).map_err(|error| error.to_string())
    }

    #[test]
    fn successful_contained_process_exit_removes_the_session() {
        let (root, plan) = plan();
        let session_root = plan.session.root.clone();

        let outcome = execute_windows_open_with(
            plan,
            launch_fixture,
            |_port, _options, alive, _close_requested, _cdp_failed| {
                assert!(alive.load(Ordering::Acquire));
                Ok(())
            },
        );

        assert_eq!(outcome.process, WindowsOpenProcessResult::Exited(0));
        assert!(outcome.ui_ready);
        assert_eq!(outcome.cleanup, WindowsCleanupResult::Removed);
        assert!(!session_root.exists());
        fs::remove_dir_all(root).expect("remove lifecycle fixture");
    }

    #[test]
    fn closing_the_primary_window_terminates_background_electron_as_success() {
        let (root, plan) = plan();
        let session_root = plan.session.root.clone();

        let outcome = execute_windows_open_with(
            plan,
            launch_fixture,
            |_port, _options, _alive, close_requested, _cdp_failed| {
                close_requested.store(true, Ordering::Release);
                Ok(())
            },
        );

        assert_eq!(outcome.process, WindowsOpenProcessResult::Exited(0));
        assert!(outcome.ui_ready);
        assert_eq!(outcome.cleanup, WindowsCleanupResult::Removed);
        assert!(!session_root.exists());
        fs::remove_dir_all(root).expect("remove lifecycle fixture");
    }

    #[test]
    fn injection_failure_terminates_the_job_before_removing_the_session() {
        let (root, plan) = plan();
        let session_root = plan.session.root.clone();

        let outcome = execute_windows_open_with(
            plan,
            launch_fixture,
            |_port, _options, _alive: Arc<AtomicBool>, _close_requested, _cdp_failed| {
                Err("fixture injection refused".to_string())
            },
        );

        assert!(matches!(
            outcome.process,
            WindowsOpenProcessResult::InjectionFailed(ref error)
                if error == "fixture injection refused"
        ));
        assert!(!outcome.ui_ready);
        assert_eq!(outcome.cleanup, WindowsCleanupResult::Removed);
        assert!(!session_root.exists());
        fs::remove_dir_all(root).expect("remove lifecycle fixture");
    }

    #[test]
    fn persistent_cdp_loss_is_not_reported_as_a_normal_close() {
        let (root, plan) = plan();
        let session_root = plan.session.root.clone();

        let outcome = execute_windows_open_with(
            plan,
            launch_fixture,
            |_port, _options, _alive, _close_requested, cdp_failed| {
                cdp_failed.store(true, Ordering::Release);
                Ok(())
            },
        );

        assert!(matches!(
            outcome.process,
            WindowsOpenProcessResult::InjectionFailed(ref error)
                if error.contains("CDP lifecycle")
        ));
        assert!(outcome.ui_ready);
        assert_eq!(outcome.cleanup, WindowsCleanupResult::Removed);
        assert!(!session_root.exists());
        fs::remove_dir_all(root).expect("remove lifecycle fixture");
    }

    #[test]
    fn unrelated_debug_listener_is_rejected_before_injection() {
        let (root, plan) = plan();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, plan.debug_port))
            .expect("occupy planned debug port");
        let injected = Arc::new(AtomicBool::new(false));
        let injection_probe = injected.clone();

        let outcome = execute_windows_open_with(
            plan,
            launch_fixture,
            move |_port, _options, _alive, _close_requested, _cdp_failed| {
                injection_probe.store(true, Ordering::Release);
                Ok(())
            },
        );

        assert!(matches!(
            outcome.process,
            WindowsOpenProcessResult::ListenerOwnershipFailed(_)
        ));
        assert!(!injected.load(Ordering::Acquire));
        assert_eq!(outcome.cleanup, WindowsCleanupResult::Removed);
        drop(listener);
        fs::remove_dir_all(root).expect("remove lifecycle fixture");
    }

    #[test]
    fn listener_replacement_after_initial_proof_is_rejected() {
        let (root, mut plan) = plan();
        plan.env_flags.insert(
            "INCODEX_WINDOWS_OPEN_DROP_LISTENER".to_string(),
            "1".to_string(),
        );
        let port = plan.debug_port;
        let (listener_tx, listener_rx) = mpsc::channel();

        let outcome = execute_windows_open_with(
            plan,
            launch_fixture,
            move |_port, _options, alive, _close_requested, _cdp_failed| {
                let deadline = Instant::now() + Duration::from_secs(2);
                let listener = loop {
                    match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
                        Ok(listener) => break listener,
                        Err(_) if Instant::now() < deadline => {
                            thread::sleep(Duration::from_millis(25));
                        }
                        Err(error) => panic!("cannot replace fixture listener: {error}"),
                    }
                };
                listener_tx
                    .send(listener)
                    .expect("hold replacement listener");
                while alive.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(5));
                }
                Err("fixture stopped after listener ownership loss".to_string())
            },
        );
        let replacement = listener_rx.recv().expect("replacement listener");

        assert!(matches!(
            outcome.process,
            WindowsOpenProcessResult::ListenerOwnershipFailed(_)
        ));
        assert!(!outcome.ui_ready);
        assert_eq!(outcome.cleanup, WindowsCleanupResult::Removed);
        drop(replacement);
        fs::remove_dir_all(root).expect("remove lifecycle fixture");
    }
}
