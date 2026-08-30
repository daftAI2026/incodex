#![cfg(target_os = "macos")]

use incodex_cli::macos_update_restore::{next_action, CoordinatorAction, CoordinatorSnapshot};

fn snapshot(
    observed_build: Option<u64>,
    parent_running: bool,
    app_running: bool,
    integration_installed: bool,
    registered: bool,
    grace_expired: bool,
) -> CoordinatorSnapshot {
    CoordinatorSnapshot {
        source_build: 7303,
        observed_build,
        parent_running,
        app_running,
        integration_installed,
        registered,
        grace_expired,
    }
}

#[test]
fn ordinary_exit_waits_for_the_replacement_grace_period() {
    assert_eq!(
        next_action(snapshot(Some(7303), false, false, true, true, false)),
        CoordinatorAction::Wait
    );
    assert_eq!(
        next_action(snapshot(Some(7303), false, false, true, true, true)),
        CoordinatorAction::ExitNoUpdate
    );
}

#[test]
fn a_temporary_bundle_replacement_gap_never_looks_like_an_ordinary_exit() {
    assert_eq!(
        next_action(snapshot(None, false, false, false, true, true)),
        CoordinatorAction::Wait
    );
}

#[test]
fn a_new_official_generation_waits_until_its_process_exits() {
    assert_eq!(
        next_action(snapshot(Some(7377), false, true, false, true, true)),
        CoordinatorAction::Wait
    );
    assert_eq!(
        next_action(snapshot(Some(7377), false, false, false, true, true)),
        CoordinatorAction::Reinstall {
            expected_build: 7377
        }
    );
}

#[test]
fn same_build_official_repair_is_restored_after_the_app_exits() {
    assert_eq!(
        next_action(snapshot(Some(7303), false, false, false, true, true)),
        CoordinatorAction::Reinstall {
            expected_build: 7303
        }
    );
}

#[test]
fn parent_and_registration_liveness_bound_the_worker() {
    assert_eq!(
        next_action(snapshot(Some(7377), true, false, false, true, true)),
        CoordinatorAction::Wait
    );
    assert_eq!(
        next_action(snapshot(Some(7377), false, false, false, false, true)),
        CoordinatorAction::ExitCancelled
    );
}
