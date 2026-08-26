#![cfg(target_os = "windows")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_core::windows_session::{
    burn_windows_session, copy_windows_settings, create_windows_session,
    sweep_orphan_windows_sessions, verify_private_acl, WindowsCleanupResult,
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn scratch(label: &str) -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "incodex-windows-session-{label}-{}-{sequence}",
        std::process::id()
    ))
}

fn create_junction(link: &Path, target: &Path) {
    let output = Command::new("cmd")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .expect("create junction fixture");
    assert!(
        output.status.success(),
        "mklink failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn creates_a_private_session_and_copies_only_safe_settings() {
    let root = scratch("copy");
    let user_root = root.join("用户 Profile").join(".incodex");
    let source = root.join("Codex source");
    fs::create_dir_all(user_root.parent().expect("profile parent")).expect("create profile");
    fs::create_dir_all(&source).expect("create source");
    fs::write(source.join("auth.json"), br#"{"token":"fixture"}"#).expect("write auth");
    fs::write(source.join("config.toml"), b"model = 'fixture'\n").expect("write config");
    fs::write(source.join("sessions.jsonl"), b"must not copy\n").expect("write history");

    let session = create_windows_session(&user_root).expect("create private session");
    let copied = copy_windows_settings(&session, &source).expect("copy safe settings");

    assert_eq!(copied, 2);
    assert_eq!(
        fs::read(session.home.join("auth.json")).expect("read copied auth"),
        br#"{"token":"fixture"}"#
    );
    assert!(session.home.join("config.toml").is_file());
    assert!(!session.home.join("sessions.jsonl").exists());
    for path in [
        &session.root,
        &session.home,
        &session.chromium,
        &session.home.join("auth.json"),
        &session.home.join("config.toml"),
    ] {
        verify_private_acl(path).expect("private directory ACL");
    }

    assert_eq!(
        burn_windows_session(&session),
        WindowsCleanupResult::Removed
    );
    assert!(!session.root.exists());
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn rejects_a_reparse_source_without_copying_through_it() {
    let root = scratch("source-junction");
    let user_root = root.join("profile").join(".incodex");
    let real_source = root.join("real-source");
    let source_link = root.join("source-link");
    fs::create_dir_all(user_root.parent().expect("profile parent")).expect("create profile");
    fs::create_dir_all(&real_source).expect("create real source");
    fs::write(real_source.join("auth.json"), b"secret").expect("write source");
    create_junction(&source_link, &real_source);
    let session = create_windows_session(&user_root).expect("create private session");

    let error = copy_windows_settings(&session, &source_link).unwrap_err();
    assert!(error.contains("reparse point"), "{error}");
    assert!(!session.home.join("auth.json").exists());

    fs::remove_dir(&source_link).expect("remove source junction");
    assert_eq!(
        burn_windows_session(&session),
        WindowsCleanupResult::Removed
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn retains_an_identity_changed_session_instead_of_deleting_its_target() {
    let root = scratch("identity");
    let user_root = root.join("profile").join(".incodex");
    let outside = root.join("outside");
    fs::create_dir_all(user_root.parent().expect("profile parent")).expect("create profile");
    fs::create_dir_all(&outside).expect("create outside");
    fs::write(outside.join("sentinel.txt"), b"keep").expect("write sentinel");
    let session = create_windows_session(&user_root).expect("create private session");
    fs::remove_dir_all(&session.root).expect("replace original session");
    create_junction(&session.root, &outside);

    let result = burn_windows_session(&session);

    assert!(matches!(result, WindowsCleanupResult::Retained { .. }));
    assert_eq!(
        fs::read(outside.join("sentinel.txt")).expect("read sentinel"),
        b"keep"
    );
    fs::remove_dir(&session.root).expect("remove replacement junction");
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn orphan_session_fixture() {
    let Some(user_root) = std::env::var_os("INCODEX_WINDOWS_ORPHAN_USER_ROOT") else {
        return;
    };
    let result_path = std::env::var_os("INCODEX_WINDOWS_ORPHAN_RESULT")
        .map(PathBuf::from)
        .expect("orphan result path");
    let session = create_windows_session(Path::new(&user_root)).expect("create orphan session");
    fs::write(result_path, session.root.to_string_lossy().as_bytes())
        .expect("publish orphan session path");
}

#[test]
fn sweeps_only_sessions_owned_by_dead_processes() {
    let root = scratch("orphan-sweep");
    let user_root = root.join("profile").join(".incodex");
    let result_path = root.join("orphan-path.txt");
    fs::create_dir_all(user_root.parent().expect("profile parent")).expect("create profile");
    let active = create_windows_session(&user_root).expect("create active session");

    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args(["orphan_session_fixture", "--exact", "--nocapture"])
        .env("INCODEX_WINDOWS_ORPHAN_USER_ROOT", &user_root)
        .env("INCODEX_WINDOWS_ORPHAN_RESULT", &result_path)
        .output()
        .expect("run orphan fixture");
    assert!(
        output.status.success(),
        "orphan fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let orphan = PathBuf::from(fs::read_to_string(&result_path).expect("read orphan path"));
    assert!(orphan.is_dir());

    assert_eq!(sweep_orphan_windows_sessions(&user_root), 1);
    assert!(!orphan.exists());
    assert!(active.root.is_dir());

    assert_eq!(burn_windows_session(&active), WindowsCleanupResult::Removed);
    fs::remove_dir_all(root).expect("remove fixture");
}
