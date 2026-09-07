#![cfg(target_os = "windows")]

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use incodex_cli::windows_self_uninstall::start_windows_self_uninstall_handoff;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
const LATE_MANAGED_FIXTURE: &str = "INCODEX_LATE_MANAGED_FIXTURE";

fn scratch() -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "incodex-windows-self-uninstall-{}-{sequence}",
        std::process::id()
    ));
    incodex_core::windows_session::ensure_private_windows_dir(&path)
        .expect("create private self-uninstall fixture")
}

fn blocking_owner() -> Child {
    Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 5",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start a blocking owner process")
}

fn wait_until_removed(paths: &[&PathBuf]) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if paths.iter().all(|path| !path.exists()) {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    paths.iter().all(|path| !path.exists())
}

#[test]
fn late_managed_process_fixture() {
    if std::env::var_os(LATE_MANAGED_FIXTURE).is_none() {
        return;
    }
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

#[test]
fn self_uninstall_handoff_waits_for_owner_and_preserves_unrelated_state() {
    let root = scratch();
    let user_root = root.join("user-root");
    let bin = user_root.join("bin");
    let package_root = user_root.join("packages/standalone");
    let runtime = user_root.join("runtime/keep.cjs");
    let session = user_root.join("sessions/keep/owner.json");

    incodex_core::windows_session::ensure_private_windows_dir(&user_root)
        .expect("create private user root");
    incodex_core::windows_session::ensure_private_windows_dir(&bin)
        .expect("create private bin directory");
    incodex_core::windows_session::ensure_private_windows_dir(&user_root.join("packages"))
        .expect("create private package parent");
    incodex_core::windows_session::ensure_private_windows_dir(&package_root)
        .expect("create private package root");
    incodex_core::windows_session::ensure_private_windows_dir(&runtime.parent().unwrap())
        .expect("create private Runtime fixture");
    incodex_core::windows_session::ensure_private_windows_dir(&user_root.join("sessions"))
        .expect("create private sessions parent");
    incodex_core::windows_session::ensure_private_windows_dir(&session.parent().unwrap())
        .expect("create private session fixture");

    let primary = bin.join("incodex.cmd");
    let alias = bin.join("inc.cmd");
    let release = package_root.join("releases/1.0.0");
    incodex_core::windows_session::ensure_private_windows_dir(&package_root.join("releases"))
        .expect("create releases fixture");
    incodex_core::windows_session::ensure_private_windows_dir(&release)
        .expect("create release fixture");
    let late_executable = release.join("late-managed.exe");
    fs::copy(
        std::env::current_exe().expect("current test executable"),
        &late_executable,
    )
    .expect("copy late managed process fixture");
    fs::write(&primary, b"primary launcher\n").expect("write primary launcher fixture");
    fs::write(&alias, b"alias launcher\n").expect("write alias launcher fixture");
    fs::write(package_root.join("current"), b"1.0.0\n").expect("write generation fixture");
    fs::write(&runtime, b"runtime fixture\n").expect("write Runtime fixture");
    fs::write(&session, b"session fixture\n").expect("write session fixture");

    let mut first_owner = blocking_owner();
    let mut second_owner = blocking_owner();
    start_windows_self_uninstall_handoff(
        &user_root,
        &package_root,
        &[first_owner.id(), second_owner.id()],
        false,
    )
    .expect("schedule external self-uninstall cleanup");

    assert!(
        incodex_cli::windows_update::acquire_windows_install_lock(&package_root).is_err(),
        "external cleanup did not retain the installer generation lock"
    );

    assert!(
        !primary.exists(),
        "primary launcher remained open for new work"
    );
    assert!(!alias.exists(), "alias launcher remained open for new work");
    assert!(package_root.exists(), "cleanup raced the owner process");
    assert!(runtime.exists(), "Runtime fixture was touched too early");
    assert!(session.exists(), "session fixture was touched too early");

    let mut late_managed = Command::new(&late_executable)
        .args(["late_managed_process_fixture", "--exact", "--nocapture"])
        .env(LATE_MANAGED_FIXTURE, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start late managed process");

    first_owner.kill().expect("stop first owner");
    first_owner.wait().expect("wait for first owner process");
    thread::sleep(Duration::from_millis(250));
    assert!(
        package_root.exists(),
        "cleanup ignored another managed CLI process"
    );

    second_owner.kill().expect("stop second owner");
    second_owner.wait().expect("wait for second owner process");
    thread::sleep(Duration::from_millis(250));
    assert!(
        package_root.exists(),
        "cleanup ignored a managed process started after the first snapshot"
    );

    late_managed.kill().expect("stop late managed process");
    late_managed.wait().expect("wait for late managed process");
    assert!(
        wait_until_removed(&[&primary, &alias, &package_root]),
        "external cleanup did not remove the owned CLI tree"
    );
    assert!(
        runtime.exists(),
        "default self-uninstall removed Runtime state"
    );
    assert!(
        session.exists(),
        "default self-uninstall removed session state"
    );

    fs::remove_dir_all(root).expect("remove private self-uninstall fixture");
}

#[test]
fn self_uninstall_path_cleanup_preserves_every_unrelated_entry_verbatim() {
    let source = include_str!("../src/windows_self_uninstall.rs");
    let cleanup = source
        .split_once("const CLEANUP_SCRIPT")
        .expect("cleanup script")
        .1
        .split_once("static SCRIPT_SEQUENCE")
        .expect("end of cleanup script")
        .0;

    assert!(!cleanup.contains("IsNullOrWhiteSpace"));
    assert!(cleanup.contains("$PathEntryRemoved = $false"));
    assert!(cleanup.contains("if ($PathEntryRemoved)"));
}

#[test]
fn restore_app_reinstates_an_abandoned_transient_registration_before_uninstalling() {
    let source = include_str!("../src/windows_self_uninstall.rs");
    let restore_app_path = source
        .split_once("if let Some(approval) = approval.as_ref()")
        .expect("restore-app uninstall path")
        .1
        .split_once("let managed_process_ids")
        .expect("end of restore-app uninstall path")
        .0;

    assert!(restore_app_path.contains("uninstall_windows_runtime_approved_with_restore"));
    assert!(restore_app_path.contains("WindowsInstalledRuntimeRegistration::from_install_state"));
    assert!(restore_app_path.contains("enable_installed_runtime"));
}
