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
