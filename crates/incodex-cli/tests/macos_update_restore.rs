//! [INPUT]: 依赖 macOS 更新恢复解析器与原生 Coordinator 源码契约
//! [OUTPUT]: 验证重启恢复入口、Sparkle 退出转发及后台更新后的静默恢复行为
//! [POS]: incodex-cli 的 macOS 更新状态机回归边界，约束原生桥接不丢失任何更新模式
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

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
    assert!(
        !termination_handler.contains("if (self.terminationPending) return"),
        "Sparkle may retry its quit event after the host delays or cancels termination, so the Coordinator must forward every retry"
    );
}

#[test]
fn coordinator_repairs_a_background_update_when_the_host_later_exits() {
    let source = include_str!("../native/macos_update_coordinator.m");
    let host_exit_handler = source
        .split("- (void)armHostExitObservation")
        .nth(1)
        .expect("coordinator observes the host process")
        .split("- (BOOL)requestHostTermination")
        .next()
        .expect("host exit observation ends before quit forwarding");

    assert!(
        host_exit_handler.contains("hostBundleChanged"),
        "a background Sparkle install replaces the app before the user later quits, so host exit must distinguish an updated bundle from an ordinary close"
    );
    assert!(
        host_exit_handler.contains("recoverAfterUpdate")
            && host_exit_handler.contains("relaunchHost:NO"),
        "background-update recovery must reuse the registered helper without reopening an app the user chose to close"
    );
    assert!(
        host_exit_handler.contains("removeItemAtPath:self.pendingPath"),
        "an ordinary close with no replacement must clear its launch-scoped pending marker"
    );
}
