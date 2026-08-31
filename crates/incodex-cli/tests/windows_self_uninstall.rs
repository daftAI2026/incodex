#![cfg(target_os = "windows")]

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use incodex_cli::windows_self_uninstall::start_windows_self_uninstall_handoff;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

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

    thread::sleep(Duration::from_millis(250));
    assert!(primary.exists(), "cleanup raced the owner process");
    assert!(alias.exists(), "cleanup raced the owner process");
    assert!(package_root.exists(), "cleanup raced the owner process");
    assert!(runtime.exists(), "Runtime fixture was touched too early");
    assert!(session.exists(), "session fixture was touched too early");

    first_owner.kill().expect("stop first owner");
    first_owner.wait().expect("wait for first owner process");
    thread::sleep(Duration::from_millis(250));
    assert!(
        package_root.exists(),
        "cleanup ignored another managed CLI process"
    );

    second_owner.kill().expect("stop second owner");
    second_owner.wait().expect("wait for second owner process");
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
