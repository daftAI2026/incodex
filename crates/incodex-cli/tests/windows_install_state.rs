#![cfg(target_os = "windows")]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_cli::windows_activation::WindowsInstalledRuntimeRegistration;
use incodex_cli::windows_helper::publish_windows_transient_helper;
use incodex_cli::windows_install::{
    capture_windows_uninstall_approval, install_windows_runtime_with,
    uninstall_windows_runtime_approved_with, uninstall_windows_runtime_with,
    uninstall_windows_runtime_with_restore, WindowsUninstallOutcome,
};
use incodex_cli::windows_install_state::{
    acquire_windows_install_state, read_windows_install_state, stage_windows_install_state,
    synchronize_windows_install_runtime_release, transition_windows_install_state,
    WindowsInstallPhase, WindowsInstallStateGuard,
};
use incodex_cli::windows_registration::{
    recover_transient_windows_debug_registration_with,
    recover_transient_windows_debug_registration_with_restore,
    stage_transient_windows_debug_registration,
};
use incodex_cli::windows_runtime::publish_windows_runtime;
use incodex_core::windows_session::verify_private_acl;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[test]
fn exposes_one_cross_process_gate_for_launch_install_and_uninstall() {
    let _gate: fn() -> Result<WindowsInstallStateGuard, String> = acquire_windows_install_state;
}

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
        |_| Ok(false),
        |_| Ok(()),
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
        |_| Ok(false),
        |_| Ok(()),
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
        |_| Ok(false),
        |_| Ok(()),
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
fn installed_registration_keeps_separate_transient_open_recovery_evidence() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    install_windows_runtime_with(
        &user_root,
        package,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install fixture Runtime");
    let transient = publish_windows_transient_helper(&user_root, &helper)
        .expect("publish transient open helper");

    stage_transient_windows_debug_registration(&user_root, package, &transient.executable)
        .expect("stage transient open beside installed registration");

    assert!(user_root.join("windows-registration.json").is_file());
    assert!(user_root
        .join("windows-transient-registration.json")
        .is_file());
    fs::remove_dir_all(user_root).expect("remove concurrent registration fixture");
}

#[test]
fn abandoned_transient_open_restores_the_installed_registration() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    install_windows_runtime_with(
        &user_root,
        package,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install fixture Runtime");
    let transient = publish_windows_transient_helper(&user_root, &helper)
        .expect("publish transient open helper");
    stage_transient_windows_debug_registration(&user_root, package, &transient.executable)
        .expect("stage abandoned transient open");

    let mut restored = false;
    assert!(recover_transient_windows_debug_registration_with_restore(
        &user_root,
        |_| Ok(Vec::new()),
        |_| Ok(true),
        |_| panic!("an installed registration must be restored, not disabled"),
        |state| {
            restored = true;
            assert_eq!(state.package_full_name, package);
            Ok(())
        },
    )
    .expect("restore installed registration after abandoned open"));

    assert!(restored);
    assert!(user_root.join("windows-registration.json").is_file());
    assert!(!user_root
        .join("windows-transient-registration.json")
        .exists());
    fs::remove_dir_all(user_root).expect("remove abandoned override fixture");
}

#[test]
fn uninstall_restores_an_abandoned_open_before_removing_the_installation() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    install_windows_runtime_with(
        &user_root,
        package,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install fixture Runtime");
    let transient = publish_windows_transient_helper(&user_root, &helper)
        .expect("publish transient open helper");
    stage_transient_windows_debug_registration(&user_root, package, &transient.executable)
        .expect("stage abandoned transient open");

    let mut restore_calls = 0;
    let mut disable_calls = 0;
    let outcome = uninstall_windows_runtime_with_restore(
        &user_root,
        |_| Ok(Vec::new()),
        |_| Ok(true),
        |_| {
            disable_calls += 1;
            Ok(())
        },
        |_| {
            restore_calls += 1;
            Ok(())
        },
    )
    .expect("recover abandoned open, then uninstall");

    assert_eq!(outcome, WindowsUninstallOutcome::Removed);
    assert_eq!(restore_calls, 1);
    assert_eq!(disable_calls, 1);
    assert!(!user_root.join("windows-registration.json").exists());
    assert!(!user_root
        .join("windows-transient-registration.json")
        .exists());
    fs::remove_dir_all(user_root).expect("remove recovered uninstall fixture");
}

#[test]
fn uninstall_disables_an_abandoned_open_when_the_installed_helper_is_missing() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    let installed = install_windows_runtime_with(
        &user_root,
        package,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install fixture Runtime");
    let transient = publish_windows_transient_helper(&user_root, &helper)
        .expect("publish transient open helper");
    stage_transient_windows_debug_registration(&user_root, package, &transient.executable)
        .expect("stage abandoned transient open");
    fs::remove_file(&installed.helper_path).expect("remove installed helper");

    let mut disable_calls = 0;
    let outcome = uninstall_windows_runtime_with_restore(
        &user_root,
        |_| Ok(Vec::new()),
        |_| Ok(true),
        |_| {
            disable_calls += 1;
            Ok(())
        },
        |_| panic!("a missing installed helper cannot be restored"),
    )
    .expect("disable transient registration and recover uninstall");

    assert_eq!(outcome, WindowsUninstallOutcome::Removed);
    assert_eq!(disable_calls, 2);
    assert!(!user_root
        .join("windows-transient-registration.json")
        .exists());
    assert!(!user_root.join("windows-install.json").exists());
    fs::remove_dir_all(user_root).expect("remove missing-helper fixture");
}

#[test]
fn uninstall_refuses_a_transient_registration_replaced_after_approval() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    let transient = publish_windows_transient_helper(&user_root, &helper)
        .expect("publish transient open helper");
    stage_transient_windows_debug_registration(&user_root, package, &transient.executable)
        .expect("stage first transient registration");
    let approved =
        capture_windows_uninstall_approval(&user_root).expect("capture displayed transient target");
    recover_transient_windows_debug_registration_with_restore(
        &user_root,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| panic!("an absent package has no debugger registration to disable"),
        |_| panic!("a transient-only registration has no installed state to restore"),
    )
    .expect("retire first transient registration");
    stage_transient_windows_debug_registration(&user_root, package, &transient.executable)
        .expect("stage replacement transient registration");

    let error = uninstall_windows_runtime_approved_with(
        &user_root,
        &approved,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
    )
    .expect_err("confirmation must be bound to the transient registration identity");

    assert!(error.contains("changed since confirmation"), "{error}");
    assert!(user_root
        .join("windows-transient-registration.json")
        .is_file());
    fs::remove_dir_all(user_root).expect("remove transient approval fixture");
}

#[test]
fn install_detects_a_package_started_during_registration() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    let mut probe_calls = 0;
    let mut disable_calls = 0;

    let error = install_windows_runtime_with(
        &user_root,
        package,
        &helper,
        |_| {
            probe_calls += 1;
            Ok(if probe_calls == 1 {
                Vec::new()
            } else {
                vec![42]
            })
        },
        |_| Ok(false),
        |registered_package| {
            disable_calls += 1;
            assert_eq!(registered_package, package);
            Ok(())
        },
        |_| Ok(()),
    )
    .expect_err("a package started during enable must prevent commit");

    assert!(error.contains("RecoveryRequired"), "{error}");
    assert!(error.contains("42"), "{error}");
    assert_eq!(probe_calls, 2);
    assert_eq!(disable_calls, 1);
    let retained = read_windows_install_state(&user_root)
        .expect("read raced install state")
        .expect("raced install state retained");
    assert_eq!(retained.phase, WindowsInstallPhase::RecoveryRequired);
    assert!(user_root.join("windows-registration.json").is_file());
    fs::remove_dir_all(user_root).expect("remove raced install fixture");
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
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install fixture Runtime");

    let waiting = uninstall_windows_runtime_with(
        &user_root,
        |_| Ok(vec![42]),
        |_| panic!("running package must block the generation query"),
        |_| panic!("running package must block disable"),
    )
    .expect("request uninstall");
    assert_eq!(
        waiting,
        WindowsUninstallOutcome::CloseRequired {
            process_ids: vec![42]
        }
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
        |_| Ok(true),
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
fn uninstall_detects_a_package_started_during_registration_removal() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    install_windows_runtime_with(
        &user_root,
        package,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install fixture Runtime");
    let mut probe_calls = 0;
    let mut disable_calls = 0;

    let outcome = uninstall_windows_runtime_with(
        &user_root,
        |_| {
            probe_calls += 1;
            Ok(if probe_calls == 1 {
                Vec::new()
            } else {
                vec![42]
            })
        },
        |_| Ok(true),
        |_| {
            disable_calls += 1;
            Ok(())
        },
    )
    .expect("detect package raced with debugger removal");

    assert_eq!(
        outcome,
        WindowsUninstallOutcome::CloseRequired {
            process_ids: vec![42]
        }
    );
    assert_eq!(probe_calls, 2);
    assert_eq!(disable_calls, 1);
    let retained = read_windows_install_state(&user_root)
        .expect("read raced uninstall state")
        .expect("raced uninstall state retained");
    assert_eq!(retained.phase, WindowsInstallPhase::RecoveryRequired);
    assert!(user_root.join("windows-registration.json").is_file());
    fs::remove_dir_all(user_root).expect("remove raced uninstall fixture");
}

#[test]
fn uninstall_disables_the_registered_package_when_the_helper_was_removed() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    let installed = install_windows_runtime_with(
        &user_root,
        package,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install fixture Runtime");
    fs::remove_file(&installed.helper_path).expect("remove published helper fixture");
    assert!(
        read_windows_install_state(&user_root).is_err(),
        "normal activation must still reject a missing helper"
    );

    let mut disabled = false;
    let outcome = uninstall_windows_runtime_with(
        &user_root,
        |_| Ok(Vec::new()),
        |_| Ok(true),
        |disabled_package| {
            disabled = true;
            assert_eq!(disabled_package, package);
            Ok(())
        },
    )
    .expect("uninstall without the published helper");

    assert_eq!(outcome, WindowsUninstallOutcome::Removed);
    assert!(
        disabled,
        "the durable debugger registration was not disabled"
    );
    assert!(
        !user_root.join("windows-install.json").exists(),
        "the disabled install generation was not retired"
    );
    fs::remove_dir_all(user_root).expect("remove missing-helper uninstall fixture");
}

#[test]
fn uninstall_uses_independent_registration_evidence_when_install_state_is_missing() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    install_windows_runtime_with(
        &user_root,
        package,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install fixture Runtime");
    assert!(
        user_root.join("windows-registration.json").is_file(),
        "install did not persist independent registration evidence"
    );
    fs::remove_file(user_root.join("windows-install.json")).expect("remove primary install state");

    let mut disabled = false;
    let outcome = uninstall_windows_runtime_with(
        &user_root,
        |_| Ok(Vec::new()),
        |_| Ok(true),
        |registered_package| {
            disabled = true;
            assert_eq!(registered_package, package);
            Ok(())
        },
    )
    .expect("recover uninstall from independent evidence");

    assert_eq!(outcome, WindowsUninstallOutcome::Removed);
    assert!(disabled);
    assert!(!user_root.join("windows-registration.json").exists());
    fs::remove_dir_all(user_root).expect("remove missing-state fixture");
}

#[test]
fn uninstall_refuses_a_registration_replaced_after_approval() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    let first = install_windows_runtime_with(
        &user_root,
        package,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install first registration");
    let approved =
        capture_windows_uninstall_approval(&user_root).expect("capture displayed target");
    uninstall_windows_runtime_with(&user_root, |_| Ok(Vec::new()), |_| Ok(true), |_| Ok(()))
        .expect("replace the approved registration");
    let replacement = install_windows_runtime_with(
        &user_root,
        package,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install replacement registration");
    assert_ne!(first.registration_id, replacement.registration_id);

    let error = uninstall_windows_runtime_approved_with(
        &user_root,
        &approved,
        |_| panic!("a replacement registration must not reach process inspection"),
        |_| panic!("a replacement registration must not reach package inspection"),
        |_| panic!("a replacement registration must not be disabled"),
    )
    .expect_err("confirmation must be bound to the displayed registration");
    assert!(error.contains("changed since confirmation"), "{error}");
    assert_eq!(
        read_windows_install_state(&user_root)
            .expect("read replacement state")
            .expect("replacement state remains")
            .registration_id,
        replacement.registration_id
    );

    uninstall_windows_runtime_with(&user_root, |_| Ok(Vec::new()), |_| Ok(true), |_| Ok(()))
        .expect("remove replacement registration");
    fs::remove_dir_all(user_root).expect("remove approval fixture");
}

#[test]
fn uninstall_removes_malformed_install_state_after_disabling_proven_registration() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    install_windows_runtime_with(
        &user_root,
        package,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install fixture Runtime");
    fs::write(user_root.join("windows-install.json"), b"{").expect("corrupt primary install state");

    let mut disabled = false;
    let outcome = uninstall_windows_runtime_with(
        &user_root,
        |_| Ok(Vec::new()),
        |_| Ok(true),
        |_| {
            disabled = true;
            Ok(())
        },
    )
    .expect("recover uninstall from malformed state");

    assert_eq!(outcome, WindowsUninstallOutcome::Removed);
    assert!(disabled);
    assert!(!user_root.join("windows-install.json").exists());
    fs::remove_dir_all(user_root).expect("remove malformed-state fixture");
}

#[test]
fn uninstall_recovers_after_the_store_replaces_the_registered_package() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let old_package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    let installed = install_windows_runtime_with(
        &user_root,
        old_package,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install old Store package fixture");
    transition_windows_install_state(
        &user_root,
        installed.epoch,
        WindowsInstallPhase::RecoveryRequired,
    )
    .expect("record the stale debugger recovery state");

    let mut package_checked = false;
    let removed = uninstall_windows_runtime_with(
        &user_root,
        |_| Ok(Vec::new()),
        |package| {
            package_checked = true;
            assert_eq!(package, old_package);
            Ok(false)
        },
        |_| panic!("a package proven absent has no debugger registration to disable"),
    )
    .expect("retire the registration for the replaced Store package");

    assert_eq!(removed, WindowsUninstallOutcome::Removed);
    assert!(package_checked);
    assert!(read_windows_install_state(&user_root)
        .expect("read retired state")
        .is_none());
    fs::remove_dir_all(user_root).expect("remove Store upgrade fixture");
}

#[test]
fn install_replaces_a_stale_store_generation() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let old_package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    let new_package = "OpenAI.Codex_1.2.3.5_x64__publisher";
    install_windows_runtime_with(
        &user_root,
        old_package,
        &helper,
        |_| Ok(Vec::new()),
        |package| {
            assert_eq!(package, old_package);
            Ok(false)
        },
        |_| panic!("an absent stale package must not be disabled"),
        |_| Ok(()),
    )
    .expect("install old Store generation");

    assert!(!recover_transient_windows_debug_registration_with(
        &user_root,
        |_| Ok(Vec::new()),
        |_| panic!("an installed registration is not transient recovery"),
        |_| panic!("an installed registration is not transient recovery"),
    )
    .expect("the public install gate must preserve matching installed evidence"));

    let replacement = install_windows_runtime_with(
        &user_root,
        new_package,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("replace stale Store generation");

    assert_eq!(replacement.package_full_name, new_package);
    assert_eq!(replacement.phase, WindowsInstallPhase::EnabledUnobserved);
    fs::remove_dir_all(user_root).expect("remove Store replacement fixture");
}

#[test]
fn install_keeps_the_old_generation_when_the_current_store_app_is_running() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let old_package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    let new_package = "OpenAI.Codex_1.2.3.5_x64__publisher";
    let old = install_windows_runtime_with(
        &user_root,
        old_package,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install old Store generation");

    let error = install_windows_runtime_with(
        &user_root,
        new_package,
        &helper,
        |package| {
            Ok(if package == new_package {
                vec![42]
            } else {
                Vec::new()
            })
        },
        |_| Ok(false),
        |_| panic!("a running current generation must preserve the old registration"),
        |_| panic!("a running current generation must block new enable"),
    )
    .expect_err("the current Store app must block replacement before old-state cleanup");

    assert!(error.contains("close Codex"), "{error}");
    let retained = read_windows_install_state(&user_root)
        .expect("read retained old state")
        .expect("old state remains");
    assert_eq!(retained.registration_id, old.registration_id);
    assert_eq!(retained.phase, WindowsInstallPhase::EnabledUnobserved);
    fs::remove_dir_all(user_root).expect("remove current-generation process fixture");
}

#[test]
fn install_disables_a_stale_generation_that_is_still_registered() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let old_package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    let new_package = "OpenAI.Codex_1.2.3.5_x64__publisher";
    install_windows_runtime_with(
        &user_root,
        old_package,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install old Store generation");

    let mut disabled = 0;
    let replacement = install_windows_runtime_with(
        &user_root,
        new_package,
        &helper,
        |_| Ok(Vec::new()),
        |package| {
            assert_eq!(package, old_package);
            Ok(true)
        },
        |package| {
            assert_eq!(package, old_package);
            disabled += 1;
            Ok(())
        },
        |registration| {
            assert_eq!(registration.package_full_name(), new_package);
            Ok(())
        },
    )
    .expect("disable old registration and install new generation");

    assert_eq!(disabled, 1);
    assert_eq!(replacement.package_full_name, new_package);
    fs::remove_dir_all(user_root).expect("remove registered Store replacement fixture");
}

#[test]
fn install_keeps_a_running_stale_store_generation() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let old_package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    let new_package = "OpenAI.Codex_1.2.3.5_x64__publisher";
    let old = install_windows_runtime_with(
        &user_root,
        old_package,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install old Store generation");

    let error = install_windows_runtime_with(
        &user_root,
        new_package,
        &helper,
        |package| {
            if package == new_package {
                Ok(Vec::new())
            } else {
                assert_eq!(package, old_package);
                Ok(vec![42])
            }
        },
        |_| panic!("running old package must block package inspection"),
        |_| panic!("running old package must block disable"),
        |_| panic!("running old package must block new enable"),
    )
    .expect_err("running old Store generation must block replacement");

    assert!(error.contains("previous Codex Store generation"), "{error}");
    let retained = read_windows_install_state(&user_root)
        .expect("read retained old state")
        .expect("old state remains");
    assert_eq!(retained.registration_id, old.registration_id);
    assert_eq!(retained.phase, WindowsInstallPhase::DisableRequested);
    fs::remove_dir_all(user_root).expect("remove running Store replacement fixture");
}

#[test]
fn uninstall_recovers_every_interrupted_install_transition() {
    let helper = std::env::current_exe().expect("test helper path");
    let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    let runtime_release = "0.5.0-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    for (phase, expected_disable_calls) in [
        (WindowsInstallPhase::Staged, 0),
        (WindowsInstallPhase::EnablePending, 1),
        (WindowsInstallPhase::DisablePending, 1),
    ] {
        let user_root = scratch_root();
        let staged = stage_windows_install_state(&user_root, package, &helper, runtime_release)
            .expect("stage interrupted install");
        let state = match phase {
            WindowsInstallPhase::Staged => staged,
            WindowsInstallPhase::EnablePending => transition_windows_install_state(
                &user_root,
                staged.epoch,
                WindowsInstallPhase::EnablePending,
            )
            .expect("record interrupted enable"),
            WindowsInstallPhase::DisablePending => {
                let enable_pending = transition_windows_install_state(
                    &user_root,
                    staged.epoch,
                    WindowsInstallPhase::EnablePending,
                )
                .expect("record enable pending");
                let enabled = transition_windows_install_state(
                    &user_root,
                    enable_pending.epoch,
                    WindowsInstallPhase::EnabledUnobserved,
                )
                .expect("record enabled");
                let disable_requested = transition_windows_install_state(
                    &user_root,
                    enabled.epoch,
                    WindowsInstallPhase::DisableRequested,
                )
                .expect("record disable requested");
                transition_windows_install_state(
                    &user_root,
                    disable_requested.epoch,
                    WindowsInstallPhase::DisablePending,
                )
                .expect("record interrupted disable")
            }
            _ => unreachable!(),
        };
        assert_eq!(state.phase, phase);

        let mut disable_calls = 0;
        let outcome = uninstall_windows_runtime_with(
            &user_root,
            |_| Ok(Vec::new()),
            |_| Ok(true),
            |_| {
                disable_calls += 1;
                Ok(())
            },
        )
        .expect("recover interrupted uninstall");
        assert_eq!(outcome, WindowsUninstallOutcome::Removed);
        assert_eq!(disable_calls, expected_disable_calls, "phase {phase:?}");
        assert!(read_windows_install_state(&user_root)
            .expect("read retired interrupted state")
            .is_none());
        fs::remove_dir_all(user_root).expect("remove interrupted install fixture");
    }
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

#[test]
fn runtime_sync_moves_durable_integration_without_rewriting_registration() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    let old_release = "0.4.0-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let published = publish_windows_runtime(&user_root).expect("publish replacement Runtime");
    let new_release = published
        .release_dir
        .file_name()
        .and_then(|name| name.to_str())
        .expect("replacement Runtime release name");
    let staged = stage_windows_install_state(&user_root, package, &helper, old_release)
        .expect("stage old Runtime integration");
    let pending = transition_windows_install_state(
        &user_root,
        staged.epoch,
        WindowsInstallPhase::EnablePending,
    )
    .expect("record enable pending");
    let enabled_unobserved = transition_windows_install_state(
        &user_root,
        pending.epoch,
        WindowsInstallPhase::EnabledUnobserved,
    )
    .expect("record enabled integration");
    let enabled = transition_windows_install_state(
        &user_root,
        enabled_unobserved.epoch,
        WindowsInstallPhase::EnabledObserved,
    )
    .expect("record observed integration");
    let registration_id = enabled.registration_id.clone();

    let synchronized = synchronize_windows_install_runtime_release(&user_root, new_release)
        .expect("synchronize installed Runtime")
        .expect("installed integration state");
    assert_eq!(synchronized.runtime_release, new_release);
    assert_eq!(synchronized.registration_id, registration_id);
    assert_eq!(synchronized.phase, WindowsInstallPhase::EnabledObserved);
    assert!(synchronized.epoch > enabled.epoch);

    let environment =
        WindowsInstalledRuntimeRegistration::environment_from_install_state(&synchronized)
            .expect("build stable debugger environment");
    let node_options = environment
        .get("NODE_OPTIONS")
        .expect("stable bootstrap option")
        .to_string_lossy();
    assert!(
        node_options.contains("/runtime/incodex-windows-bootstrap.cjs"),
        "{node_options}"
    );
    assert!(!node_options.contains("/releases/"), "{node_options}");

    fs::remove_dir_all(user_root).expect("remove Runtime synchronization fixture");
}
