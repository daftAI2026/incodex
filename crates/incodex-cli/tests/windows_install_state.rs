#![cfg(target_os = "windows")]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_cli::windows_install::{
    install_windows_runtime_with, uninstall_windows_runtime_with, WindowsUninstallOutcome,
};
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
fn stages_and_enables_the_installed_runtime_only_after_proving_codex_is_closed() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    let mut enabled = false;
    let installed = install_windows_runtime_with(
        &user_root,
        package,
        &helper,
        |_| Ok(Vec::new()),
        |registration| {
            enabled = true;
            assert_eq!(registration.package_full_name(), package);
            assert!(registration
                .debugger_command_line()
                .contains("__incodex_windows_installed_debugger"));
            Ok(())
        },
    )
    .expect("enable installed Runtime");

    assert!(enabled);
    assert_eq!(installed.phase, WindowsInstallPhase::EnabledUnobserved);
    assert!(installed.desired_enabled());
    assert!(installed.helper_path.is_file());
    assert!(user_root.join("runtime/current.json").is_file());

    fs::remove_dir_all(user_root).expect("remove installed Runtime fixture");

    let blocked_root = scratch_root();
    let error = install_windows_runtime_with(
        &blocked_root,
        package,
        &helper,
        |_| Ok(vec![42]),
        |_| panic!("running package must block enable"),
    )
    .expect_err("running Codex must block install");
    assert!(error.contains("close Codex"), "{error}");
    assert!(!blocked_root.exists(), "blocked install created state");

    let uncertain_root = scratch_root();
    let error = install_windows_runtime_with(
        &uncertain_root,
        package,
        &helper,
        |_| Ok(Vec::new()),
        |_| Err("fixture enable uncertainty".to_string()),
    )
    .expect_err("uncertain enable must fail");
    assert!(error.contains("RecoveryRequired"), "{error}");
    let retained = read_windows_install_state(&uncertain_root)
        .expect("read retained state")
        .expect("uncertain state retained");
    assert_eq!(retained.phase, WindowsInstallPhase::RecoveryRequired);
    assert!(!retained.desired_enabled());
    fs::remove_dir_all(uncertain_root).expect("remove uncertain install fixture");
}

#[test]
fn uninstall_publishes_the_kill_switch_before_waiting_then_disables_once() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    install_windows_runtime_with(
        &user_root,
        package,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(()),
    )
    .expect("install fixture Runtime");

    let waiting = uninstall_windows_runtime_with(
        &user_root,
        |_| Ok(vec![42]),
        |_| panic!("running package must block disable"),
    )
    .expect("request uninstall");
    assert_eq!(
        waiting,
        WindowsUninstallOutcome::CloseRequired { process_ids: vec![42] }
    );
    let kill_switch = read_windows_install_state(&user_root)
        .expect("read kill switch")
        .expect("kill switch retained");
    assert_eq!(kill_switch.phase, WindowsInstallPhase::DisableRequested);
    assert!(!kill_switch.desired_enabled());

    let mut disabled = false;
    let removed = uninstall_windows_runtime_with(
        &user_root,
        |_| Ok(Vec::new()),
        |disabled_package| {
            disabled = true;
            assert_eq!(disabled_package, package);
            Ok(())
        },
    )
    .expect("finish uninstall");
    assert_eq!(removed, WindowsUninstallOutcome::Removed);
    assert!(disabled);
    assert!(
        read_windows_install_state(&user_root)
            .expect("read removed state")
            .is_none(),
        "disabled registration state was not retired"
    );

    fs::remove_dir_all(user_root).expect("remove uninstall fixture");
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
    assert!(staged
        .registration_id
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit()));
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
