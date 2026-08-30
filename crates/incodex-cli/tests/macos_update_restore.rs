#![cfg(target_os = "macos")]

use incodex_cli::macos_update_restore::{
    choose_sparkle_update, next_action, CoordinatorAction, CoordinatorPhase,
    CoordinatorSnapshot, SparkleDownload,
};

fn snapshot(
    phase: CoordinatorPhase,
    app_build: u64,
    app_running: bool,
    integration_installed: bool,
    registered: bool,
    downloaded_target: Option<u64>,
) -> CoordinatorSnapshot {
    CoordinatorSnapshot {
        phase,
        app_build,
        app_running,
        integration_installed,
        registered,
        downloaded_target,
    }
}

#[test]
fn newest_complete_delta_for_the_running_build_is_selected() {
    let downloads = [
        SparkleDownload {
            source_build: Some(7303),
            target_build: 7345,
            bytes: 12,
            complete: true,
        },
        SparkleDownload {
            source_build: Some(7303),
            target_build: 7377,
            bytes: 35_061_182,
            complete: true,
        },
        SparkleDownload {
            source_build: Some(6892),
            target_build: 6962,
            bytes: 9,
            complete: true,
        },
        SparkleDownload {
            source_build: Some(7303),
            target_build: 7400,
            bytes: 0,
            complete: false,
        },
    ];

    assert_eq!(choose_sparkle_update(7303, &downloads), Some(7377));
}

#[test]
fn ordinary_exit_without_update_evidence_does_not_reinstall() {
    let action = next_action(snapshot(
        CoordinatorPhase::Watching,
        7303,
        false,
        true,
        true,
        None,
    ));

    assert_eq!(action, CoordinatorAction::ExitNoUpdate);
}

#[test]
fn downloaded_update_waits_for_a_normal_exit_before_restoring_vendor_app() {
    let running = next_action(snapshot(
        CoordinatorPhase::Watching,
        7303,
        true,
        true,
        true,
        Some(7377),
    ));
    assert_eq!(
        running,
        CoordinatorAction::PersistIntent {
            source_build: 7303,
            target_build: 7377,
        }
    );

    let stopped = next_action(snapshot(
        CoordinatorPhase::AwaitingPatchedExit {
            source_build: 7303,
            target_build: 7377,
        },
        7303,
        false,
        true,
        true,
        Some(7377),
    ));
    assert_eq!(
        stopped,
        CoordinatorAction::RestoreVendorAndLaunch {
            source_build: 7303,
            target_build: 7377,
        }
    );
}

#[test]
fn new_official_generation_waits_for_its_process_before_reinstalling() {
    let running = next_action(snapshot(
        CoordinatorPhase::AwaitingOfficialUpdate {
            source_build: 7303,
            target_build: 7377,
        },
        7377,
        true,
        false,
        true,
        Some(7377),
    ));
    assert_eq!(running, CoordinatorAction::Wait);

    let stopped = next_action(snapshot(
        CoordinatorPhase::AwaitingOfficialUpdate {
            source_build: 7303,
            target_build: 7377,
        },
        7377,
        false,
        false,
        true,
        Some(7377),
    ));
    assert_eq!(
        stopped,
        CoordinatorAction::Reinstall {
            expected_build: 7377,
        }
    );
}

#[test]
fn registration_removal_cancels_every_pending_generation() {
    let action = next_action(snapshot(
        CoordinatorPhase::AwaitingOfficialUpdate {
            source_build: 7303,
            target_build: 7377,
        },
        7377,
        false,
        false,
        false,
        Some(7377),
    ));

    assert_eq!(action, CoordinatorAction::ExitCancelled);
}

#[test]
fn stale_or_same_build_downloads_never_start_a_repair() {
    let downloads = [
        SparkleDownload {
            source_build: Some(7303),
            target_build: 7303,
            bytes: 1,
            complete: true,
        },
        SparkleDownload {
            source_build: Some(6892),
            target_build: 6962,
            bytes: 1,
            complete: true,
        },
    ];

    assert_eq!(choose_sparkle_update(7303, &downloads), None);
}
