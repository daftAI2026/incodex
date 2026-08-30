#![cfg(target_os = "macos")]

use std::collections::VecDeque;
use std::process::Command;

use incodex_cli::macos_update_restore::{
    drive_coordinator, next_action, parse_worker_request, CoordinatorAction, CoordinatorOutcome,
    CoordinatorSnapshot, WorkerRequest,
};

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
fn ordinary_relaunch_does_not_keep_the_worker_alive_after_the_grace_period() {
    assert_eq!(
        next_action(snapshot(Some(7303), false, true, true, true, true)),
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

#[test]
fn coordinator_waits_then_reinstalls_exactly_once() {
    let mut observations = VecDeque::from([
        snapshot(Some(7303), true, false, true, true, false),
        snapshot(Some(7377), false, true, false, true, false),
        snapshot(Some(7377), false, false, false, true, false),
    ]);
    let mut waits = 0;
    let mut reinstalls = Vec::new();

    let outcome = drive_coordinator(
        || observations.pop_front().ok_or("fixture exhausted".into()),
        |build| {
            reinstalls.push(build);
            Ok(())
        },
        || {
            waits += 1;
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(outcome, CoordinatorOutcome::Reinstalled { build: 7377 });
    assert_eq!(waits, 2);
    assert_eq!(reinstalls, vec![7377]);
}

#[test]
fn coordinator_exits_without_mutation_when_registration_is_cancelled() {
    let mut reinstalled = false;

    let outcome = drive_coordinator(
        || Ok(snapshot(Some(7303), false, false, true, false, true)),
        |_| {
            reinstalled = true;
            Ok(())
        },
        || Err("must not wait".into()),
    )
    .unwrap();

    assert_eq!(outcome, CoordinatorOutcome::Cancelled);
    assert!(!reinstalled);
}

#[test]
fn worker_mode_requires_an_explicit_marker_epoch_and_parent_pid() {
    assert_eq!(parse_worker_request(None, None, None), None);
    assert!(parse_worker_request(Some("1"), None, Some("42"))
        .unwrap()
        .is_err());
    assert!(
        parse_worker_request(Some("1"), Some("install-a"), Some("0"))
            .unwrap()
            .is_err()
    );
    assert_eq!(
        parse_worker_request(Some("1"), Some("install-a"), Some("42")),
        Some(Ok(WorkerRequest {
            install_id: "install-a".into(),
            parent_pid: 42,
        }))
    );
}

#[test]
fn native_cli_routes_internal_worker_mode_before_public_parsing() {
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .env("INCODEX_MACOS_UPDATE_WORKER", "1")
        .env_remove("INCODEX_MACOS_UPDATE_INSTALL_ID")
        .env("INCODEX_MACOS_UPDATE_PARENT_PID", "42")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("install epoch"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
