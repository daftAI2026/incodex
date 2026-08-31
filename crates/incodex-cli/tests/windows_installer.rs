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
    run_installer_mode(download_dir, user_root, true)
}

fn run_installer_from_wow64(download_dir: &Path, user_root: &Path) -> std::process::Output {
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
        .env("INCODEX_TEST_MODE", "1")
        .env("INCODEX_ARCH", "x86")
        .env("PROCESSOR_ARCHITEW6432", "AMD64")
        .output()
        .expect("run installer through a simulated WOW64 shell")
}

fn run_installer_without_file_hash_command(
    download_dir: &Path,
    user_root: &Path,
) -> std::process::Output {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("install.ps1");
    let escaped_script = script.to_string_lossy().replace('\'', "''");
    Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"])
        .arg(format!(
            "function Invoke-BlockedFileHash {{ throw 'Get-FileHash is unavailable' }}; Set-Alias Get-FileHash Invoke-BlockedFileHash; . '{escaped_script}'"
        ))
        .env("INCODEX_DOWNLOAD_DIR", download_dir)
        .env("INCODEX_USER_ROOT", user_root)
        .env("INCODEX_EXPECTED_VERSION", env!("CARGO_PKG_VERSION"))
        .env("INCODEX_SKIP_PATH", "1")
        .env("INCODEX_TEST_MODE", "1")
        .output()
        .expect("run installer without Get-FileHash")
}

fn run_installer_mode(
    download_dir: &Path,
    user_root: &Path,
    test_mode: bool,
) -> std::process::Output {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("install.ps1");
    let mut command = Command::new("powershell.exe");
    command
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
        .env("INCODEX_SKIP_PATH", "1");
    if test_mode {
        command.env("INCODEX_TEST_MODE", "1");
    }
    command.output().expect("run PowerShell installer")
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
    let user_root = root.join("用户 root with spaces");
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
    assert_eq!(
        fs::read_to_string(user_root.join("packages/standalone/current"))
            .expect("read current generation"),
        format!("{}\n", env!("CARGO_PKG_VERSION"))
    );
    let primary = user_root.join("bin/incodex.cmd");
    let alias = user_root.join("bin/inc.cmd");
    let primary_body = fs::read_to_string(&primary).expect("read primary launcher");
    let alias_body = fs::read_to_string(&alias).expect("read alias launcher");
    assert!(primary_body.contains("INCODEX_MANAGED_BY_STANDALONE=1"));
    assert!(primary_body.contains("INCODEX_MANAGED_PACKAGE_ROOT="));
    assert!(primary_body.contains("INCODEX_VERSION"));
    assert!(primary_body.contains(r"packages\standalone\current"));
    assert!(primary_body.contains("DisableDelayedExpansion"));
    assert!(
        !primary_body.contains(&format!(r"releases\{}", env!("CARGO_PKG_VERSION"))),
        "launcher must not be rewritten for each release"
    );
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
fn accepts_a_32_bit_powershell_on_64_bit_windows() {
    let root = scratch("wow64");
    let user_root = root.join("user-root");
    let source = Path::new(env!("CARGO_BIN_EXE_incodex"));
    let release = release_fixture(&root, &sha256(source));

    let output = run_installer_from_wow64(&release, &user_root);
    let launcher_was_published = user_root.join("bin/incodex.cmd").exists();
    fs::remove_dir_all(&root).expect("remove WOW64 fixture");

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(launcher_was_published);
}

#[test]
fn installer_hashes_without_get_file_hash() {
    let root = scratch("no-get-file-hash");
    let user_root = root.join("user-root");
    let source = Path::new(env!("CARGO_BIN_EXE_incodex"));
    let release = release_fixture(&root, &sha256(source));

    let output = run_installer_without_file_hash_command(&release, &user_root);
    let launcher_was_published = user_root.join("bin/incodex.cmd").exists();
    fs::remove_dir_all(&root).expect("remove Get-FileHash fixture");

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(launcher_was_published);
}

#[test]
fn installer_tightens_an_existing_generation_lock_acl() {
    let root = scratch("existing-lock-acl");
    let user_root = root.join("user-root");
    let package_root = user_root.join("packages/standalone");
    fs::create_dir_all(&package_root).expect("create legacy package root");
    let lock = package_root.join("install.lock");
    fs::write(&lock, b"").expect("create legacy generation lock");
    let source = Path::new(env!("CARGO_BIN_EXE_incodex"));
    let release = release_fixture(&root, &sha256(source));

    let output = run_installer(&release, &user_root);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    incodex_core::windows_session::verify_private_acl(&lock)
        .expect("installer must migrate the lock to a protected current-user DACL");

    fs::remove_dir_all(root).expect("remove scratch directory");
}

#[test]
fn malformed_duplicate_checksum_fails_closed() {
    let root = scratch("malformed-checksum");
    let user_root = root.join("user-root");
    let source = Path::new(env!("CARGO_BIN_EXE_incodex"));
    let release = release_fixture(&root, &sha256(source));
    fs::OpenOptions::new()
        .append(true)
        .open(release.join("SHA256SUMS"))
        .and_then(|mut file| {
            use std::io::Write;
            writeln!(file, "not-a-digest  incodex-windows-x64.exe")
        })
        .expect("append malformed checksum entry");

    let output = run_installer(&release, &user_root);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("SHA256SUMS"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!user_root.join("bin/incodex.cmd").exists());

    fs::remove_dir_all(root).expect("remove scratch directory");
}

#[test]
fn existing_release_junction_fails_closed() {
    let root = scratch("release-junction");
    let user_root = root.join("user-root");
    let release_fixture_root =
        release_fixture(&root, &sha256(Path::new(env!("CARGO_BIN_EXE_incodex"))));
    let external = root.join("external-release");
    fs::create_dir_all(&external).expect("create external release");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), external.join("incodex.exe"))
        .expect("copy external CLI");
    let releases = user_root
        .join("packages")
        .join("standalone")
        .join("releases");
    fs::create_dir_all(&releases).expect("create releases parent");
    let junction = fs::canonicalize(&releases)
        .expect("resolve releases parent")
        .join(env!("CARGO_PKG_VERSION"));
    let external = fs::canonicalize(&external).expect("resolve external release");
    let linked = Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&external)
        .output()
        .expect("create release junction");
    assert!(
        linked.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&linked.stdout),
        String::from_utf8_lossy(&linked.stderr)
    );

    let output = run_installer(&release_fixture_root, &user_root);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("reparse point"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    Command::new("cmd.exe")
        .args(["/d", "/c", "rmdir"])
        .arg(&junction)
        .status()
        .expect("remove release junction");
    fs::remove_dir_all(root).expect("remove scratch directory");
}

#[test]
fn production_installer_rejects_test_only_path_overrides() {
    let root = scratch("production-overrides");
    let user_root = root.join("user-root");
    let source = Path::new(env!("CARGO_BIN_EXE_incodex"));
    let release = release_fixture(&root, &sha256(source));

    let output = run_installer_mode(&release, &user_root, false);
    let launcher_was_published = user_root.join("bin/incodex.cmd").exists();
    fs::remove_dir_all(&root).expect("remove scratch directory");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("test-only"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!launcher_was_published);
}

#[test]
fn oversized_checksum_manifest_fails_closed() {
    let root = scratch("oversized-checksum");
    let user_root = root.join("user-root");
    let source = Path::new(env!("CARGO_BIN_EXE_incodex"));
    let release = release_fixture(&root, &sha256(source));
    fs::OpenOptions::new()
        .append(true)
        .open(release.join("SHA256SUMS"))
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(&vec![b'#'; 300 * 1024])
        })
        .expect("inflate checksum manifest");

    let output = run_installer(&release, &user_root);
    let launcher_was_published = user_root.join("bin/incodex.cmd").exists();
    fs::remove_dir_all(&root).expect("remove scratch directory");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("too large"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!launcher_was_published);
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
