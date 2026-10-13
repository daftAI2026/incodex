/*
 * [INPUT]: 依赖权限 guide 夹具、有限宿主预算与闭集转场。
 * [OUTPUT]: 验证机器等待、协议错误与错误页退出语义。
 * [POS]: 权限引导机器终态回归；不启动宿主或触碰 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;
use std::thread;

struct FlowOps {
    probes: VecDeque<AccessibilityStatus>,
    last_probe: AccessibilityStatus,
    screens: VecDeque<ScreenPermissionStatus>,
    last_screen: ScreenPermissionStatus,
    resets: usize,
    screen_settings: usize,
    traces: Vec<&'static str>,
}

impl FlowOps {
    fn new(probes: &[AccessibilityStatus], screen: ScreenPermissionStatus) -> Self {
        Self {
            probes: probes.iter().copied().collect(),
            last_probe: probes
                .last()
                .copied()
                .unwrap_or(AccessibilityStatus::Unknown),
            screens: [screen].into(),
            last_screen: screen,
            resets: 0,
            screen_settings: 0,
            traces: Vec::new(),
        }
    }
}

impl GuideOps for FlowOps {
    fn launch(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn probe(&mut self) -> AccessibilityStatus {
        if let Some(status) = self.probes.pop_front() {
            self.last_probe = status;
        }
        self.last_probe
    }

    fn probe_screen(&mut self) -> Option<ScreenPermissionStatus> {
        if let Some(screen) = self.screens.pop_front() {
            self.last_screen = screen;
        }
        Some(self.last_screen)
    }

    fn observe(&mut self, event: GuideTraceEvent) {
        self.traces.push(event.log_event());
    }

    fn wait_for_window(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn reset(&mut self) -> Result<(), String> {
        self.resets += 1;
        Ok(())
    }

    fn open_settings(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn open_screen_settings(&mut self) -> Result<(), String> {
        self.screen_settings += 1;
        Ok(())
    }

    fn wait(&mut self, _: Duration) {}
}

fn explicit_with_events(
    probes: &[AccessibilityStatus],
    events: &[HostEvent],
    limits: GuideTimeouts,
) -> (Result<Outcome, String>, FlowOps, Factory) {
    let mut ops = FlowOps::new(probes, ScreenPermissionStatus::NotGranted);
    let mut factory = Factory {
        events: events.iter().cloned().collect(),
        log: Rc::default(),
        starts: 0,
    };
    let result = flow::run_permission_guide_with_limits(
        &mut ops,
        Path::new("/fixture"),
        Path::new(OFFICIAL_APP_PATH),
        &mut || Ok(()),
        &mut factory,
        limits,
    );
    (result, ops, factory)
}

#[test]
fn machine_waits_and_protocol_violations_are_not_user_skips() {
    let (result, ops, _) = explicit_with_events(
        &[AccessibilityStatus::Granted],
        &[],
        GuideTimeouts {
            ready: Duration::ZERO,
            choice: Some(Duration::from_secs(1)),
            guide: Some(Duration::from_secs(1)),
        },
    );
    assert_eq!(result.unwrap(), Outcome::AwaitingEvidence);
    assert_eq!(ops.traces, ["permission-guide machine-interrupted"]);

    for action in [
        HostEvent::Allow,
        HostEvent::Retry,
        HostEvent::AllowScreen,
        HostEvent::RetryScreen,
    ] {
        let (result, ops, _) = explicit_with_events(
            &[AccessibilityStatus::Granted],
            std::slice::from_ref(&action),
            GuideTimeouts::for_user(),
        );
        assert!(
            result.is_err(),
            "{action:?} before Ready is a protocol failure"
        );
        assert_eq!(ops.traces, ["permission-guide machine-interrupted"]);
    }

    for (event, trace) in [
        (HostEvent::Later, "permission-guide user-skip"),
        (HostEvent::Close, "permission-guide user-close"),
    ] {
        let (result, ops, _) = explicit_with_events(
            &[AccessibilityStatus::Granted],
            &[event],
            GuideTimeouts::for_user(),
        );
        assert_eq!(result.unwrap(), Outcome::Pending);
        assert_eq!(ops.traces, [trace]);
    }

    for unavailable in [
        AccessibilityStatus::Unknown,
        AccessibilityStatus::NotRunning,
    ] {
        let (result, ops, _) = explicit_with_events(
            &[
                AccessibilityStatus::Granted,
                AccessibilityStatus::Granted,
                unavailable,
            ],
            &[HostEvent::Ready, HostEvent::Allow],
            GuideTimeouts::for_user(),
        );
        assert_eq!(result.unwrap(), Outcome::AwaitingEvidence);
        assert_eq!(
            ops.traces,
            [
                "permission-guide card-ready",
                "permission-guide machine-interrupted"
            ]
        );
    }
}

#[test]
fn late_screen_allow_after_grant_does_not_open_settings() {
    let mut ops = FlowOps::new(
        &[
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
        ],
        ScreenPermissionStatus::NotGranted,
    );
    ops.screens = [
        ScreenPermissionStatus::NotGranted,
        ScreenPermissionStatus::NotGranted,
        ScreenPermissionStatus::Granted,
        ScreenPermissionStatus::Granted,
    ]
    .into();
    let mut factory = Factory {
        events: [
            HostEvent::Ready,
            HostEvent::Timeout,
            HostEvent::AllowScreen,
            HostEvent::Later,
        ]
        .into(),
        log: Rc::default(),
        starts: 0,
    };

    let result = flow::run_permission_guide_with_limits(
        &mut ops,
        Path::new("/fixture"),
        Path::new(OFFICIAL_APP_PATH),
        &mut || Ok(()),
        &mut factory,
        GuideTimeouts::for_user(),
    );

    assert_eq!(result.unwrap(), Outcome::Pending);
    assert_eq!((ops.resets, ops.screen_settings), (0, 0));
    assert_eq!(
        ops.traces,
        ["permission-guide card-ready", "permission-guide user-skip"]
    );
}

#[test]
fn illegal_accessibility_and_screen_action_sequences_are_errors() {
    for (probes, events) in [
        (
            vec![AccessibilityStatus::Denied, AccessibilityStatus::Denied],
            vec![HostEvent::Ready, HostEvent::Retry],
        ),
        (
            vec![AccessibilityStatus::Denied, AccessibilityStatus::Denied],
            vec![HostEvent::Ready, HostEvent::RetryScreen],
        ),
        (
            vec![
                AccessibilityStatus::Denied,
                AccessibilityStatus::Denied,
                AccessibilityStatus::Denied,
                AccessibilityStatus::Granted,
            ],
            vec![HostEvent::Ready, HostEvent::Allow, HostEvent::Allow],
        ),
        (
            vec![
                AccessibilityStatus::Granted,
                AccessibilityStatus::Granted,
                AccessibilityStatus::Granted,
            ],
            vec![
                HostEvent::Ready,
                HostEvent::AllowScreen,
                HostEvent::AllowScreen,
            ],
        ),
    ] {
        let (result, ops, _) = explicit_with_events(&probes, &events, GuideTimeouts::for_user());
        assert!(
            result.is_err(),
            "illegal sequence {events:?} must fail closed"
        );
        assert_eq!(
            ops.traces.last(),
            Some(&"permission-guide machine-interrupted")
        );
        assert!(!ops.traces.iter().any(|event| {
            matches!(
                *event,
                "permission-guide user-skip" | "permission-guide user-close"
            )
        }));
    }
}

#[derive(Clone, Copy)]
enum ErrorPageExit {
    Later,
    Close,
    Eof,
    Budget,
    TransportFailure,
}

struct ErrorPageFactory {
    exit: ErrorPageExit,
    begin_guide: bool,
}

struct ErrorPageHost {
    exit: ErrorPageExit,
    begin_guide: bool,
    ready_sent: bool,
    action_sent: bool,
    error_page: bool,
}

impl GuideHost for ErrorPageHost {
    fn send_state(&mut self, state: HostState) -> Result<(), String> {
        if matches!(state, HostState::Error(_)) {
            self.error_page = true;
        }
        Ok(())
    }

    fn send_permission_state(&mut self, _: &str, _: HostState) -> Result<(), String> {
        Ok(())
    }

    fn poll(&mut self, timeout: Duration) -> Result<HostEvent, String> {
        if !self.ready_sent {
            self.ready_sent = true;
            return Ok(HostEvent::Ready);
        }
        if !self.error_page {
            if self.begin_guide && !self.action_sent {
                self.action_sent = true;
                return Ok(HostEvent::AllowScreen);
            }
            thread::sleep(timeout);
            return Ok(HostEvent::Timeout);
        }
        match self.exit {
            ErrorPageExit::Later => Ok(HostEvent::Later),
            ErrorPageExit::Close => Ok(HostEvent::Close),
            ErrorPageExit::Eof => Ok(HostEvent::Eof),
            ErrorPageExit::Budget => {
                thread::sleep(timeout);
                Ok(HostEvent::Timeout)
            }
            ErrorPageExit::TransportFailure => Err("private transport detail".into()),
        }
    }

    fn close(&mut self) {}
}

impl GuideHostFactory for ErrorPageFactory {
    fn start(&mut self, _: &Path, _: &Path) -> Result<Box<dyn GuideHost>, String> {
        Ok(Box::new(ErrorPageHost {
            exit: self.exit,
            begin_guide: self.begin_guide,
            ready_sent: false,
            action_sent: false,
            error_page: false,
        }))
    }
}

#[test]
fn finite_error_page_budget_defers_only_after_a_real_user_dismissal() {
    for begin_guide in [false, true] {
        for (exit, expected, trace) in [
            (
                ErrorPageExit::Later,
                Outcome::Pending,
                "permission-guide user-skip",
            ),
            (
                ErrorPageExit::Close,
                Outcome::Pending,
                "permission-guide user-close",
            ),
            (
                ErrorPageExit::Eof,
                Outcome::AwaitingEvidence,
                "permission-guide host-eof",
            ),
            (
                ErrorPageExit::Budget,
                Outcome::AwaitingEvidence,
                "permission-guide machine-interrupted",
            ),
            (
                ErrorPageExit::TransportFailure,
                Outcome::AwaitingEvidence,
                "permission-guide machine-interrupted",
            ),
        ] {
            let mut ops = FlowOps::new(
                &[AccessibilityStatus::Granted],
                ScreenPermissionStatus::NotGranted,
            );
            let mut factory = ErrorPageFactory { exit, begin_guide };
            let result = flow::run_permission_guide_with_limits(
                &mut ops,
                Path::new("/fixture"),
                Path::new(OFFICIAL_APP_PATH),
                &mut || Ok(()),
                &mut factory,
                GuideTimeouts {
                    ready: Duration::from_secs(1),
                    choice: Some(Duration::from_millis(2)),
                    guide: Some(Duration::from_millis(2)),
                },
            );
            assert_eq!(result.unwrap(), expected);
            assert_eq!(
                ops.traces,
                ["permission-guide card-ready", trace],
                "error-page exit {trace} after guide-start={begin_guide} must match the returned outcome"
            );
        }
    }
}
