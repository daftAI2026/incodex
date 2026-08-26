use super::*;
use std::fs;
use std::net::{Ipv4Addr, TcpListener};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[test]
fn visible_window_loss_closes_background_electron_after_a_grace_period() {
    let grace = Duration::from_millis(250);
    let started = Instant::now();
    let mut lifecycle = VisibleWindowLifecycle::new(grace);

    assert!(!lifecycle.should_close(true, started));
    let missing = started + Duration::from_millis(1);
    assert!(!lifecycle.should_close(false, missing));
    assert!(!lifecycle.should_close(false, missing + grace - Duration::from_millis(1)));
    assert!(lifecycle.should_close(false, missing + grace));
}

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
        let delay = std::env::var("INCODEX_WINDOWS_OPEN_EXIT_AFTER_LISTENER_DROP_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_secs(2));
        thread::sleep(delay);
    }
}

fn launch_fixture(
    plan: &WindowsOpenPlan,
) -> Result<crate::windows_process::WindowsProcessTree, WindowsActivationFailure> {
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
    spawn_kill_on_drop(&mut command)
        .map_err(|error| WindowsActivationFailure::before_start(error.to_string()))
}

#[test]
fn successful_contained_process_exit_removes_the_session() {
    let (root, plan) = plan();
    let session_root = plan.session.root.clone();

    let outcome = execute_windows_open_with(
        plan,
        launch_fixture,
        |_port, _options, alive, _close_requested, _cdp_failed, _ownership_guard| {
            assert!(alive.load(Ordering::Acquire));
            Ok(Vec::new())
        },
    );

    assert_eq!(outcome.process, WindowsOpenProcessResult::Exited(0));
    assert!(outcome.ui_ready);
    assert_eq!(outcome.cleanup, WindowsCleanupResult::Removed);
    assert!(!session_root.exists());
    fs::remove_dir_all(root).expect("remove lifecycle fixture");
}

#[test]
fn launch_failure_after_activation_retains_the_session_when_shutdown_is_unproven() {
    let (root, plan) = plan();
    let session_root = plan.session.root.clone();

    let outcome = execute_windows_open_with(
        plan,
        |_plan| {
            Err(WindowsActivationFailure::after_start(
                "fixture activation failed",
                Err("fixture process shutdown is unproven".to_string()),
            ))
        },
        |_port, _options, _alive, _close_requested, _cdp_failed, _ownership_guard| {
            panic!("injection must not run after launch failure")
        },
    );

    assert!(matches!(
        outcome.process,
        WindowsOpenProcessResult::SpawnFailed(ref error)
            if error == "fixture activation failed"
    ));
    assert!(matches!(
        outcome.cleanup,
        WindowsCleanupResult::Unknown { ref reason }
            if reason.contains("shutdown is unproven")
    ));
    assert!(
        session_root.exists(),
        "an active writer may still own the session"
    );
    fs::remove_dir_all(root).expect("remove retained lifecycle fixture");
}

#[test]
fn closing_the_primary_window_terminates_background_electron_as_success() {
    let (root, plan) = plan();
    let session_root = plan.session.root.clone();
    let monitor_finished = Arc::new(AtomicBool::new(false));
    let monitor_finished_after_close = monitor_finished.clone();

    let outcome = execute_windows_open_with(
        plan,
        launch_fixture,
        move |_port, _options, alive, close_requested, cdp_failed, _ownership_guard| {
            close_requested.store(true, Ordering::Release);
            cdp_failed.store(true, Ordering::Release);
            Ok(vec![thread::spawn(move || {
                while alive.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(5));
                }
                thread::sleep(Duration::from_millis(100));
                monitor_finished_after_close.store(true, Ordering::Release);
            })])
        },
    );

    assert_eq!(outcome.process, WindowsOpenProcessResult::Exited(0));
    assert!(outcome.ui_ready);
    assert_eq!(outcome.cleanup, WindowsCleanupResult::Removed);
    assert!(monitor_finished.load(Ordering::Acquire));
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
        |_port,
         _options,
         _alive: Arc<AtomicBool>,
         _close_requested,
         _cdp_failed,
         _ownership_guard| { Err("fixture injection refused".to_string()) },
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
        |_port, _options, _alive, _close_requested, cdp_failed, _ownership_guard| {
            cdp_failed.store(true, Ordering::Release);
            Ok(Vec::new())
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
        move |_port, _options, _alive, _close_requested, _cdp_failed, _ownership_guard| {
            injection_probe.store(true, Ordering::Release);
            Ok(Vec::new())
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
        move |_port, _options, alive, _close_requested, _cdp_failed, _ownership_guard| {
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

#[test]
fn listener_shutdown_immediately_before_process_exit_is_normal() {
    let (root, mut plan) = plan();
    plan.env_flags.insert(
        "INCODEX_WINDOWS_OPEN_DROP_LISTENER".to_string(),
        "1".to_string(),
    );
    plan.env_flags.insert(
        "INCODEX_WINDOWS_OPEN_EXIT_AFTER_LISTENER_DROP_MS".to_string(),
        "75".to_string(),
    );
    let session_root = plan.session.root.clone();
    let outcome = execute_windows_open_with(
        plan,
        launch_fixture,
        |_port, _options, _alive, _close_requested, _cdp_failed, _ownership_guard| Ok(Vec::new()),
    );

    assert_eq!(outcome.process, WindowsOpenProcessResult::Exited(0));
    assert!(outcome.ui_ready);
    assert_eq!(outcome.cleanup, WindowsCleanupResult::Removed);
    assert!(!session_root.exists());
    fs::remove_dir_all(root).expect("remove lifecycle fixture");
}
