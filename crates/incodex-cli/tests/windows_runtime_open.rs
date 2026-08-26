#![cfg(target_os = "windows")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_cli::windows_app::WindowsCodexApp;
use incodex_cli::windows_runtime_open::{
    parse_windows_runtime_open, prepare_windows_runtime_open, validate_windows_runtime_ready,
    windows_runtime_ready_for_handshake,
};
use incodex_core::windows_session::{
    burn_windows_session, verify_private_acl, WindowsCleanupResult,
};

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
fn guardian_protects_the_shared_runtime_ready_marker_before_trusting_it() {
    let root = scratch();
    let install = root.join("package");
    let profile = root.join("profile");
    let source = profile.join(".codex");
    fs::create_dir_all(install.join("app")).expect("create package fixture");
    fs::create_dir_all(&source).expect("create source home");
    let plan =
        prepare_windows_runtime_open(&app_at(&install), &profile.join(".incodex"), &source, None)
            .expect("prepare installed Runtime open");
    let ready = plan.session.root.join("ready");
    fs::write(&ready, b"1787760000000\n").expect("write inherited ready marker");

    assert!(validate_windows_runtime_ready(&plan.session).expect("validate ready marker"));
    verify_private_acl(&ready).expect("guardian protected ready marker ACL");

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
