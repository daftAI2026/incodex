#![cfg(target_os = "windows")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use incodex_cli::windows_app::WindowsCodexApp;
use incodex_cli::windows_runtime_open::{
    parse_windows_runtime_open, prepare_windows_runtime_open, windows_runtime_ready_for_handshake,
    windows_runtime_shutdown_authorized, windows_runtime_startup_action, WindowsRuntimeOwnerClaim,
    WindowsRuntimeReadinessDeadline, WindowsRuntimeReadyPipe, WindowsRuntimeStartupAction,
};
use incodex_core::windows_session::{burn_windows_session, WindowsCleanupResult};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    std::env::temp_dir().join(format!(
        "incodex-windows-runtime-open-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

fn app_at(install_location: &Path) -> WindowsCodexApp {
    WindowsCodexApp {
        package_full_name: "OpenAI.Codex_9.8.7.6_x64__2p2nqsd0c76g0".to_string(),
        app_user_model_id: "OpenAI.Codex_2p2nqsd0c76g0!App".to_string(),
        install_location: install_location.to_path_buf(),
        executable: install_location.join("app/ChatGPT.exe"),
        architecture: "X64".to_string(),
    }
}

#[test]
fn hidden_runtime_open_accepts_only_absolute_bounded_lifecycle_input() {
    let args = vec![
        "__incodex_windows_runtime_open".to_string(),
        "--source-home".to_string(),
        r"C:\Users\me\.codex".to_string(),
        "--source-bounds".to_string(),
        "10,20,1200,800".to_string(),
    ];
    let request = parse_windows_runtime_open(&args)
        .expect("recognize hidden mode")
        .expect("accept request");
    assert_eq!(request.source_home, PathBuf::from(r"C:\Users\me\.codex"));
    assert_eq!(request.source_bounds.as_deref(), Some("10,20,1200,800"));

    let relative = vec![
        "__incodex_windows_runtime_open".to_string(),
        "--source-home".to_string(),
        r"relative\.codex".to_string(),
        "--source-bounds".to_string(),
        String::new(),
    ];
    assert!(parse_windows_runtime_open(&relative)
        .expect("recognize hidden mode")
        .unwrap_err()
        .contains("absolute"));
    assert!(parse_windows_runtime_open(&["status".to_string()]).is_none());
}

#[test]
fn installed_runtime_plan_reuses_native_session_without_cdp_or_duplicate_ui() {
    let root = scratch();
    let install = root.join("package");
    let profile = root.join("profile");
    let source = profile.join(".codex");
    fs::create_dir_all(install.join("app")).expect("create package fixture");
    fs::create_dir_all(&source).expect("create source home");
    fs::write(source.join("auth.json"), b"fixture-auth").expect("write auth");
    fs::write(source.join("config.toml"), b"localeOverride = 'zh-CN'\n").expect("write config");

    let app = app_at(&install);
    let plan = prepare_windows_runtime_open(
        &app,
        &profile.join(".incodex"),
        &source,
        Some("10,20,1200,800"),
    )
    .expect("prepare installed Runtime open");

    assert_eq!(plan.bin, app.executable);
    assert_eq!(
        plan.args,
        vec![
            format!("--user-data-dir={}", plan.session.chromium.display()),
            "codex://new?mode=codex".to_string(),
        ]
    );
    assert!(!plan.args.iter().any(|arg| arg.contains("remote-debugging")));
    let activation = plan
        .activation_request()
        .expect("build installed Runtime activation");
    assert_eq!(activation.package_full_name(), app.package_full_name);
    assert_eq!(activation.app_user_model_id(), app.app_user_model_id);
    assert!(activation.arguments().contains("--user-data-dir="));
    assert!(!activation.arguments().contains("remote-debugging"));
    assert!(!plan.env_flags.contains_key("INCODEX_WINDOWS_BOOTSTRAPPED"));
    assert_eq!(plan.env.get("CODEX_HOME"), Some(&plan.session.home));
    assert_eq!(
        plan.env.get("CODEX_ELECTRON_USER_DATA_PATH"),
        Some(&plan.session.chromium)
    );
    assert_eq!(
        plan.env_flags
            .get("INCODEX_CLEANUP_OWNER")
            .map(String::as_str),
        Some("native")
    );
    assert_eq!(
        plan.env_flags
            .get("INCODEX_SOURCE_BOUNDS")
            .map(String::as_str),
        Some("10,20,1200,800")
    );
    assert_eq!(
        fs::read(plan.session.home.join("auth.json")).expect("copied auth"),
        b"fixture-auth"
    );
    assert_eq!(
        burn_windows_session(&plan.session),
        WindowsCleanupResult::Removed
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn guardian_reports_ready_only_after_runtime_acceptance_and_a_visible_window() {
    assert!(!windows_runtime_ready_for_handshake(false, false));
    assert!(!windows_runtime_ready_for_handshake(true, false));
    assert!(!windows_runtime_ready_for_handshake(false, true));
    assert!(windows_runtime_ready_for_handshake(true, true));
}

#[test]
fn visible_window_gets_its_own_runtime_acceptance_budget() {
    let started = Instant::now();
    let budget = Duration::from_secs(30);
    let mut readiness = WindowsRuntimeReadinessDeadline::new(started, budget);

    assert!(!readiness.timed_out(started + Duration::from_secs(29), false));
    assert!(!readiness.timed_out(started + budget, true));
    assert!(!readiness.timed_out(started + Duration::from_secs(59), true));
    assert!(readiness.timed_out(started + Duration::from_secs(60), true));

    let mut never_visible = WindowsRuntimeReadinessDeadline::new(started, budget);
    assert!(never_visible.timed_out(started + budget, false));
}

#[test]
fn guardian_destroys_a_ready_session_only_after_authenticated_close_or_job_exit() {
    assert!(!windows_runtime_shutdown_authorized(false, false));
    assert!(windows_runtime_shutdown_authorized(true, false));
    assert!(windows_runtime_shutdown_authorized(false, true));
}

#[test]
fn guardian_consumes_authenticated_close_before_runtime_readiness() {
    assert_eq!(
        windows_runtime_startup_action(true, false),
        WindowsRuntimeStartupAction::Finish
    );
    assert_eq!(
        windows_runtime_startup_action(true, true),
        WindowsRuntimeStartupAction::Finish
    );
    assert_eq!(
        windows_runtime_startup_action(false, true),
        WindowsRuntimeStartupAction::FailExited
    );
    assert_eq!(
        windows_runtime_startup_action(false, false),
        WindowsRuntimeStartupAction::Continue
    );
}

#[test]
fn runtime_acceptance_comes_from_the_connecting_named_pipe_process() {
    use std::io::Write;

    let pipe = WindowsRuntimeReadyPipe::create().expect("create ready pipe");
    let pipe_name = pipe.name().to_string();
    let writer = std::thread::spawn(move || {
        let mut client = fs::OpenOptions::new()
            .write(true)
            .open(pipe_name)
            .expect("connect ready pipe");
        client.write_all(b"accepted\n").expect("write acceptance");
    });
    let acceptance = pipe.accept().expect("accept Runtime readiness");
    writer.join().expect("join ready writer");

    assert_eq!(acceptance.process_id, std::process::id());
    assert_eq!(acceptance.message, "accepted");
}

#[test]
fn close_signal_uses_a_distinct_unpredictable_named_pipe() {
    let pipe = WindowsRuntimeReadyPipe::create_close().expect("create close pipe");
    assert!(pipe.name().starts_with(r"\\.\pipe\Incodex-Runtime-Closed-"));
    assert_eq!(
        pipe.name().len(),
        32 + r"\\.\pipe\Incodex-Runtime-Closed-".len()
    );
}

#[test]
fn one_native_owner_serializes_all_installed_runtime_clicks() {
    let first = WindowsRuntimeOwnerClaim::acquire().expect("acquire first owner");
    assert!(matches!(first, WindowsRuntimeOwnerClaim::Owned(_)));
    let second = WindowsRuntimeOwnerClaim::acquire().expect("inspect existing owner");
    assert!(matches!(second, WindowsRuntimeOwnerClaim::Existing));
    drop(first);
    let replacement = WindowsRuntimeOwnerClaim::acquire().expect("acquire replacement owner");
    assert!(matches!(replacement, WindowsRuntimeOwnerClaim::Owned(_)));
}
