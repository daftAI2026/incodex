#![cfg(target_os = "windows")]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_cli::windows_install_state::{
    read_windows_install_state, stage_windows_install_state, transition_windows_install_state,
    WindowsInstallPhase,
};
use incodex_core::windows_session::verify_private_acl;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn scratch_root() -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "incodex-windows-install-state-{}-{sequence}",
        std::process::id()
    ))
}

#[test]
fn persists_owned_install_transitions_with_epoch_cas_and_an_early_disable_kill_switch() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    let runtime_release = "0.5.0-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    let staged = stage_windows_install_state(&user_root, package, &helper, runtime_release)
        .expect("stage install state");
    assert_eq!(staged.epoch, 1);
    assert_eq!(staged.phase, WindowsInstallPhase::Staged);
    assert!(staged.desired_enabled());
    assert_eq!(staged.package_full_name, package);
    assert_eq!(staged.runtime_release, runtime_release);
    assert_eq!(staged.registration_id.len(), 32);
    assert!(staged.registration_id.bytes().all(|byte| byte.is_ascii_hexdigit()));
    verify_private_acl(&staged.state_path).expect("private install state ACL");

    let pending = transition_windows_install_state(
        &user_root,
        staged.epoch,
        WindowsInstallPhase::EnablePending,
    )
    .expect("record enable intent");
    assert_eq!(pending.epoch, 2);
    assert!(transition_windows_install_state(
        &user_root,
        staged.epoch,
        WindowsInstallPhase::EnabledUnobserved,
    )
    .is_err());

    let enabled = transition_windows_install_state(
        &user_root,
        pending.epoch,
        WindowsInstallPhase::EnabledUnobserved,
    )
    .expect("record successful enable");
    let disabled_requested = transition_windows_install_state(
        &user_root,
        enabled.epoch,
        WindowsInstallPhase::DisableRequested,
    )
    .expect("publish uninstall kill switch");
    assert!(!disabled_requested.desired_enabled());

    let reread = read_windows_install_state(&user_root)
        .expect("read install state")
        .expect("install state exists");
    assert_eq!(reread, disabled_requested);

    fs::remove_dir_all(user_root).expect("remove install state fixture");
}
