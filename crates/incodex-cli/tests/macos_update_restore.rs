#![cfg(target_os = "macos")]

use std::process::Command;

use incodex_cli::macos_update_restore::parse_relaunch_request;

#[test]
fn relaunch_mode_requires_an_explicit_marker_and_install_epoch() {
    assert_eq!(parse_relaunch_request(None, None), None);
    assert!(parse_relaunch_request(Some("1"), None).unwrap().is_err());
    assert_eq!(
        parse_relaunch_request(Some("1"), Some("install-a")),
        Some(Ok("install-a".into()))
    );
}

#[test]
fn native_cli_routes_relaunch_recovery_before_public_parsing() {
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .env("INCODEX_MACOS_UPDATE_RELAUNCH", "1")
        .env_remove("INCODEX_MACOS_UPDATE_INSTALL_ID")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("install epoch"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn coordinator_forwards_sparkle_quit_to_the_running_host() {
    let source = include_str!("../native/macos_update_coordinator.m");
    let termination_handler = source
        .split("- (NSApplicationTerminateReply)applicationShouldTerminate:")
        .nth(1)
        .expect("coordinator has an application termination handler")
        .split("- (void)launchHostAndExit")
        .next()
        .expect("termination handler ends before relaunch code");

    assert!(
        termination_handler.contains("[self requestHostTermination]"),
        "Sparkle quits the Coordinator application, so the Coordinator must forward that normal quit request to the still-running host before waiting for its exit"
    );
    assert!(
        !termination_handler.contains("SIGKILL") && !termination_handler.contains("SIGTERM"),
        "the update handoff must preserve the official app's normal task-confirmation exit path"
    );
}
