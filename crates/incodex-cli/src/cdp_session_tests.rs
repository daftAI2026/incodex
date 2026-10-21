/*
 * [INPUT]: 依赖 cdp_mode 的模式快照与活动时钟。
 * [OUTPUT]: 验证长阻挡窗口与确认后断线的恢复边界。
 * [POS]: CDP会话生命周期回归；不启动窗口或访问权限服务。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

use std::time::{Duration, Instant};

use super::{CodexModeAction, CodexModePageState, CodexModeReadiness};

#[test]
fn readiness_preserves_long_official_pending_before_success() {
    let start = Instant::now();
    let mut readiness = CodexModeReadiness::new(start, Duration::from_secs(90));

    assert_eq!(
        readiness.observe_at(CodexModePageState::BlockedByOfficialUi, start),
        CodexModeAction::BlockedByOfficialUi
    );
    assert_eq!(
        readiness.observe_at(
            CodexModePageState::BlockedByOfficialUi,
            start + Duration::from_secs(10 * 60),
        ),
        CodexModeAction::BlockedByOfficialUi
    );
    assert_eq!(
        readiness.observe_at(
            CodexModePageState::Codex,
            start + Duration::from_secs(10 * 60),
        ),
        CodexModeAction::Confirmed
    );
}

#[test]
fn confirmed_session_reconnect_gets_a_fresh_clock_without_repeating_fallback() {
    let start = Instant::now();
    let mut readiness = CodexModeReadiness::new(start, Duration::from_secs(90));
    assert_eq!(
        readiness.observe_at(CodexModePageState::Other, start),
        CodexModeAction::Wait
    );
    assert_eq!(
        readiness.observe_at(CodexModePageState::Other, start + Duration::from_secs(1)),
        CodexModeAction::Wait
    );
    assert_eq!(
        readiness.observe_at(CodexModePageState::Other, start + Duration::from_secs(2)),
        CodexModeAction::SelectFallback
    );
    assert_eq!(
        readiness.observe_at(CodexModePageState::Codex, start + Duration::from_secs(100)),
        CodexModeAction::Confirmed
    );

    let reconnect = start + Duration::from_secs(1_000);
    readiness.restart_after_confirmed_session_disconnect_at(reconnect);
    assert_eq!(
        readiness.observe_at(
            CodexModePageState::NotReady,
            reconnect + Duration::from_secs(1)
        ),
        CodexModeAction::Wait
    );
    for seconds in [2, 3, 4] {
        assert_eq!(
            readiness.observe_at(
                CodexModePageState::Other,
                reconnect + Duration::from_secs(seconds),
            ),
            CodexModeAction::Wait,
            "a recovered session must not select fallback a second time"
        );
    }
    assert_eq!(
        readiness.observe_at(
            CodexModePageState::NotReady,
            reconnect + Duration::from_secs(91)
        ),
        CodexModeAction::Unresolved
    );
}

#[test]
fn reconnect_after_direct_codex_confirmation_does_not_start_fallback() {
    let start = Instant::now();
    let mut readiness = CodexModeReadiness::new(start, Duration::from_secs(90));
    assert_eq!(
        readiness.observe_at(CodexModePageState::Codex, start),
        CodexModeAction::Confirmed
    );

    let reconnect = start + Duration::from_secs(120);
    readiness.restart_after_confirmed_session_disconnect_at(reconnect);
    for seconds in [0, 1, 2] {
        assert_eq!(
            readiness.observe_at(
                CodexModePageState::Other,
                reconnect + Duration::from_secs(seconds),
            ),
            CodexModeAction::Wait,
            "a confirmed session must not select a fallback after reconnect"
        );
    }
}
