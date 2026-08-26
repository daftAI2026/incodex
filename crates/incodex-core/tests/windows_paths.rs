#![cfg(target_os = "windows")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_core::windows_path::{require_local_disk_absolute, validate_existing_session_dir};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn scratch(label: &str) -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "incodex-windows-path-{label}-{}-{sequence}",
        std::process::id()
    ))
}

fn remove_tree(root: &Path) {
    if root.exists() {
        fs::remove_dir_all(root).expect("remove scratch tree");
    }
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
fn accepts_a_plain_unicode_tree_inside_the_trusted_root() {
    let root = scratch("plain");
    let trusted = root.join("用户 Profile").join(".incodex");
    let sessions = trusted.join("sessions with spaces");
    fs::create_dir_all(&sessions).expect("create plain session tree");

    let validated = validate_existing_session_dir(&trusted, &sessions).expect("validate tree");
    assert_eq!(
        validated,
        sessions.canonicalize().expect("canonical sessions")
    );

    remove_tree(&root);
}

#[test]
fn rejects_a_directory_outside_the_trusted_root() {
    let root = scratch("outside");
    let trusted = root.join("profile").join(".incodex");
    let outside = root.join("outside").join("sessions");
    fs::create_dir_all(&trusted).expect("create trusted root");
    fs::create_dir_all(&outside).expect("create outside directory");

    let error = validate_existing_session_dir(&trusted, &outside).unwrap_err();
    assert!(error.contains("outside trusted root"), "{error}");

    remove_tree(&root);
}

#[test]
fn rejects_a_junction_even_when_it_points_inside_the_trusted_root() {
    let root = scratch("junction");
    let trusted = root.join("profile").join(".incodex");
    let real_sessions = trusted.join("real-sessions");
    let junction = trusted.join("sessions");
    fs::create_dir_all(&real_sessions).expect("create junction target");
    create_junction(&junction, &real_sessions);

    let error = validate_existing_session_dir(&trusted, &junction).unwrap_err();
    assert!(error.contains("reparse point"), "{error}");

    fs::remove_dir(&junction).expect("remove junction fixture");
    remove_tree(&root);
}

#[test]
fn source_home_preflight_accepts_local_drives_and_rejects_network_or_device_namespaces() {
    for trusted in [r"C:\Users\fixture\.codex", r"\\?\C:\Users\fixture\.codex"] {
        require_local_disk_absolute(Path::new(trusted), "Windows Codex source home")
            .expect("local drive source");
    }

    for untrusted in [
        r"\\server\share\.codex",
        r"\\?\UNC\server\share\.codex",
        r"\\.\PIPE\incodex",
    ] {
        let error = require_local_disk_absolute(Path::new(untrusted), "Windows Codex source home")
            .expect_err("network and device paths must fail before filesystem access");
        assert!(error.contains("local disk"), "{error}");
    }
}
