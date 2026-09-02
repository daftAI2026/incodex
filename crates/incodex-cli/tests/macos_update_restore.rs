//! [INPUT]: 依赖 macOS 更新恢复解析器与原生 Coordinator 源码契约
//! [OUTPUT]: 验证重启恢复入口、Sparkle 退出转发、handoff 所有权及后台更新后的静默恢复行为
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
        host_exit_handler.contains("removePendingIfOwned"),
        "an ordinary close with no replacement must clear only its own launch-scoped pending marker"
    );
}

#[test]
fn coordinator_resolves_the_current_helper_generation_before_recovery() {
    let source = include_str!("../native/macos_update_coordinator.m");
    let recovery = source
        .split("- (void)launchRecovery:")
        .nth(1)
        .expect("coordinator has a recovery path")
        .split("- (void)recoverAfterUpdate:")
        .next()
        .expect("generation-aware launch ends before the recovery entry point");

    assert!(
        recovery.contains("currentRegistrationForPending"),
        "recovery must resolve the current content-addressed Helper generation instead of trusting the launch-time pending path"
    );
    assert!(
        recovery.contains("registration[@\"helperPath\"]"),
        "the Helper executable must come from the current verified registration"
    );
    assert!(
        !recovery.contains("pending[@\"helperPath\"]"),
        "a stale pending file must not launch its obsolete Helper after Runtime refreshes registration"
    );
}

#[test]
fn coordinator_executes_only_a_verified_content_addressed_helper() {
    let source = include_str!("../native/macos_update_coordinator.m");

    assert!(
        source.contains("helperSha256") && source.contains("CC_SHA256"),
        "the Coordinator must hash the Helper itself before NSTask crosses the execution boundary"
    );
    assert!(
        source.contains("helpers/macos-update") && source.contains("O_NOFOLLOW"),
        "the Helper must remain a no-follow file under the private content-addressed root"
    );
}

#[test]
fn coordinator_never_clears_another_handoff_or_reports_a_failed_helper_as_success() {
    let source = include_str!("../native/macos_update_coordinator.m");
    let recovery = source
        .split("- (void)launchRecovery:")
        .nth(1)
        .expect("coordinator has a recovery path")
        .split("- (void)recoverAfterUpdate:")
        .next()
        .expect("generation-aware launch ends before the recovery entry point");

    assert!(
        source.contains("handoffId") && source.contains("removePendingIfOwned"),
        "pending cleanup must be conditional on the Coordinator's unique handoff owner"
    );
    assert!(
        recovery.contains("terminationStatus") && recovery.contains("retry"),
        "a Helper generation race must be retried and a nonzero exit must not masquerade as recovery success"
    );
}

#[test]
fn recovery_helper_identity_is_stable_across_registration_refresh() {
    let coordinator = include_str!("../native/macos_update_coordinator.m");
    let restore = include_str!("../src/macos_update_restore.rs");

    assert!(
        coordinator.contains("INCODEX_MACOS_UPDATE_HELPER_SHA256")
            && coordinator.contains("registration[@\"helperSha256\"]"),
        "the verified Helper generation must cross the Coordinator-to-Helper process boundary"
    );
    assert!(
        restore.contains("INCODEX_MACOS_UPDATE_HELPER_SHA256")
            && restore.contains("expected_helper_sha256")
            && restore.contains("registration.helper_sha256 != expected_helper_sha256"),
        "the running Helper and the mutable registration must agree on one generation before mutation"
    );
}

#[test]
fn pending_handoff_ownership_is_atomic_and_adopted_by_the_recovery_coordinator() {
    let coordinator = include_str!("../native/macos_update_coordinator.m");
    let assets = include_str!("../src/macos_update_assets.rs");

    assert!(
        coordinator.contains("flock") && coordinator.contains("withPendingLock"),
        "pending read-check-write/delete must share one OS lock across Coordinator processes"
    );
    assert!(
        coordinator.contains("coordinatorPid")
            && coordinator.contains("loadAndAdoptPending")
            && coordinator.contains("pendingIsOwned"),
        "the post-update Coordinator must atomically adopt the handoff before it launches a Helper"
    );
    assert!(
        assets.contains("pending_control_lock") && assets.contains(".pending.lock"),
        "Rust removal must participate in the same pending lock as native Coordinator writers"
    );
}

#[test]
fn invalid_registration_cannot_relaunch_an_untrusted_or_missing_app_path() {
    let source = include_str!("../native/macos_update_coordinator.m");
    let finish = source
        .split("- (void)finishRecovery:")
        .nth(1)
        .expect("Coordinator has one recovery completion path")
        .split("- (void)launchRecovery:")
        .next()
        .expect("completion ends before Helper launch");

    assert!(
        finish.contains("relaunchHost && self.appPath.length > 0"),
        "relaunch is allowed only after registration has established a trusted application path"
    );
}
