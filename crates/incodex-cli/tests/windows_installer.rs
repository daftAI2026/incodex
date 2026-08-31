#![cfg(target_os = "windows")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn scratch(label: &str) -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "incodex-windows-installer-{label}-{}-{sequence}",
        std::process::id()
    ));
    fs::create_dir_all(&path).expect("create scratch directory");
    path
}

fn sha256(path: &Path) -> String {
    let bytes = fs::read(path).expect("read asset");
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn run_installer(download_dir: &Path, user_root: &Path) -> std::process::Output {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("install.ps1");
    Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script)
        .env("INCODEX_DOWNLOAD_DIR", download_dir)
        .env("INCODEX_USER_ROOT", user_root)
        .env("INCODEX_EXPECTED_VERSION", env!("CARGO_PKG_VERSION"))
        .env("INCODEX_SKIP_PATH", "1")
        .output()
        .expect("run PowerShell installer")
}

fn release_fixture(root: &Path, digest: &str) -> PathBuf {
    let release = root.join("release");
    fs::create_dir_all(&release).expect("create release fixture");
    let asset = release.join("incodex-windows-x64.exe");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &asset).expect("copy release fixture");
    fs::write(
        release.join("SHA256SUMS"),
        format!("{digest}  incodex-windows-x64.exe\n"),
    )
    .expect("write checksum fixture");
    release
}

#[test]
fn installs_a_verified_versioned_release_and_two_launchers() {
    let root = scratch("success");
    let user_root = root.join("user-root");
    let source = Path::new(env!("CARGO_BIN_EXE_incodex"));
    let release = release_fixture(&root, &sha256(source));

    let output = run_installer(&release, &user_root);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let installed = user_root
        .join("packages/standalone/releases")
        .join(env!("CARGO_PKG_VERSION"))
        .join("incodex.exe");
    assert_eq!(sha256(&installed), sha256(source));
    let primary = user_root.join("bin/incodex.cmd");
    let alias = user_root.join("bin/inc.cmd");
    let primary_body = fs::read_to_string(&primary).expect("read primary launcher");
    let alias_body = fs::read_to_string(&alias).expect("read alias launcher");
    assert!(primary_body.contains("INCODEX_MANAGED_BY_STANDALONE=1"));
    assert!(primary_body.contains("INCODEX_MANAGED_PACKAGE_ROOT="));
    assert!(primary_body.contains(&format!(
        r"packages\standalone\releases\{}\incodex.exe",
        env!("CARGO_PKG_VERSION")
    )));
    assert!(alias_body.contains("incodex.cmd"));

    let launched = Command::new("cmd.exe")
        .args(["/d", "/c"])
        .arg(&primary)
        .arg("--version")
        .output()
        .expect("run installed launcher");
    assert!(
        launched.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&launched.stdout),
        String::from_utf8_lossy(&launched.stderr)
    );
    assert!(String::from_utf8_lossy(&launched.stdout)
        .contains(&format!("Incodex version {}", env!("CARGO_PKG_VERSION"))));

    fs::remove_dir_all(root).expect("remove scratch directory");
}

#[test]
fn checksum_failure_does_not_publish_a_launcher_or_release() {
    let root = scratch("checksum");
    let user_root = root.join("user-root");
    let release = release_fixture(&root, &"00".repeat(32));

    let output = run_installer(&release, &user_root);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("checksum mismatch"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!user_root.join("bin/incodex.cmd").exists());
    assert!(!user_root.join("packages/standalone/releases").exists());

    fs::remove_dir_all(root).expect("remove scratch directory");
}
