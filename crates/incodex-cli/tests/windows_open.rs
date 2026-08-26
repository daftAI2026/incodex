#![cfg(target_os = "windows")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_cli::windows_app::WindowsCodexApp;
use incodex_cli::windows_open::prepare_windows_open;
use incodex_core::windows_session::{burn_windows_session, WindowsCleanupResult};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "incodex-windows-open-{}-{sequence}",
        std::process::id()
    ))
}

fn app_at(install_location: &Path) -> WindowsCodexApp {
    WindowsCodexApp {
        package_full_name: "OpenAI.Codex_9.8.7.6_x64__2p2nqsd0c76g0".to_string(),
        app_user_model_id: "OpenAI.Codex_2p2nqsd0c76g0!App".to_string(),
        install_location: install_location.to_path_buf(),
        executable: install_location.join("app/ChatGPT.exe"),
        asar: install_location.join("app/resources/app.asar"),
        asar_unpacked: install_location.join("app/resources/app.asar.unpacked"),
        architecture: "X64".to_string(),
    }
}

#[test]
fn prepares_open_from_the_discovered_users_package_without_hardcoded_install_paths() {
    let root = scratch();
    let install = root.join("每个用户不同").join("OpenAI.Codex_9.8.7.6");
    let profile = root.join("用户 Profile");
    let user_root = profile.join(".incodex");
    let source = profile.join(".codex");
    fs::create_dir_all(&install).expect("create package fixture");
    fs::create_dir_all(&source).expect("create source home");
    fs::write(source.join("auth.json"), b"fixture-auth").expect("write auth");
    fs::write(source.join("config.toml"), b"localeOverride = 'zh-CN'\n").expect("write config");

    let app = app_at(&install);
    let plan = prepare_windows_open(&app, &user_root, &source, None).expect("prepare open");

    assert_eq!(plan.bin, app.executable);
    assert!(plan.args.contains(&format!(
        "--user-data-dir={}",
        plan.session.chromium.display()
    )));
    assert!(plan
        .args
        .contains(&"--remote-debugging-address=127.0.0.1".to_string()));
    assert_eq!(plan.env.get("CODEX_HOME"), Some(&plan.session.home));
    assert_eq!(
        plan.env.get("CODEX_ELECTRON_USER_DATA_PATH"),
        Some(&plan.session.chromium)
    );
    assert_eq!(
        fs::read(plan.session.home.join("auth.json")).expect("copied auth"),
        b"fixture-auth"
    );
    assert!(!plan.session.home.join("sessions.jsonl").exists());

    assert_eq!(
        burn_windows_session(&plan.session),
        WindowsCleanupResult::Removed
    );
    fs::remove_dir_all(root).expect("remove fixture");
}
