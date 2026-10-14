/*
 * [INPUT]: 依赖共用 GuideOps、双行状态机与无窗传输夹具。
 * [OUTPUT]: 验证安装前置检查、缺权呈现与用户终态。
 * [POS]: 安装权限模式回归；不启动宿主或访问真实 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;

struct InstallOps {
    states: VecDeque<PermissionSnapshot>,
    reads: usize,
}

impl GuideOps for InstallOps {
    fn launch(&mut self) -> Result<(), String> {
        panic!("安装权限状态机不能提前打开工作宿主")
    }
    fn probe(&mut self) -> AccessibilityStatus {
        self.states.front().unwrap().accessibility
    }
    fn snapshot(&mut self) -> PermissionSnapshot {
        self.reads += 1;
        if self.states.len() > 1 {
            self.states.pop_front().unwrap()
        } else {
            self.states.front().unwrap().clone()
        }
    }
    fn wait_for_window(&mut self) -> Result<(), String> {
        panic!("安装权限卡不能依赖尚未打开的工作窗口")
    }
    fn reset(&mut self) -> Result<(), String> {
        panic!("只读检查和用户终态不能重置权限")
    }
    fn open_settings(&mut self) -> Result<(), String> {
        panic!("无 Allow 动作不能打开设置")
    }
    fn wait(&mut self, _: Duration) {}
}

fn snapshot(ax: AccessibilityStatus, screen: ScreenPermissionStatus) -> PermissionSnapshot {
    PermissionSnapshot {
        accessibility: ax,
        screen: Some(screen),
    }
}

fn before_launch(
    states: Vec<PermissionSnapshot>,
    events: Vec<HostEvent>,
) -> (Outcome, InstallOps, Factory) {
    let mut ops = InstallOps {
        states: states.into(),
        reads: 0,
    };
    let mut factory = Factory {
        events: events.into(),
        log: Rc::default(),
        starts: 0,
    };
    let result = flow::run_permission_guide_with_mode(
        &mut ops,
        Path::new("/fixture"),
        Path::new(OFFICIAL_APP_PATH),
        &mut || Ok(()),
        &mut factory,
        GuideTimeouts::for_user(),
        flow::GuideMode::BeforeLaunch,
    );
    (result.unwrap(), ops, factory)
}

#[test]
fn install_dual_granted_needs_neither_card_nor_work_window() {
    let (outcome, ops, factory) = before_launch(
        vec![snapshot(
            AccessibilityStatus::Granted,
            ScreenPermissionStatus::Granted,
        )],
        vec![],
    );
    assert_eq!(outcome, Outcome::Granted);
    assert_eq!(ops.reads, 1);
    assert_eq!(factory.starts, 0);
}

#[test]
fn install_unknown_does_not_open_card_or_poll_until_a_guess() {
    for ax in [
        AccessibilityStatus::Unknown,
        AccessibilityStatus::NotRunning,
        AccessibilityStatus::Granted,
    ] {
        let (outcome, ops, factory) =
            before_launch(vec![snapshot(ax, ScreenPermissionStatus::Unknown)], vec![]);
        assert_eq!(outcome, Outcome::AwaitingEvidence);
        assert_eq!(ops.reads, 1);
        assert_eq!(factory.starts, 0);
    }
}

#[test]
fn install_unavailable_screen_adapter_is_unknown_not_a_single_row_grant() {
    let (outcome, _, factory) = before_launch(
        vec![PermissionSnapshot {
            accessibility: AccessibilityStatus::Granted,
            screen: None,
        }],
        vec![],
    );
    assert_eq!(outcome, Outcome::AwaitingEvidence);
    assert_eq!(factory.starts, 0);
}

#[test]
fn install_known_missing_can_present_original_card_without_work_window() {
    let missing = snapshot(
        AccessibilityStatus::Granted,
        ScreenPermissionStatus::NotGranted,
    );
    let (outcome, _, factory) = before_launch(
        vec![
            missing.clone(),
            missing,
            snapshot(
                AccessibilityStatus::Granted,
                ScreenPermissionStatus::Granted,
            ),
        ],
        vec![HostEvent::Ready, HostEvent::Timeout],
    );
    assert_eq!(outcome, Outcome::Granted);
    assert_eq!(factory.starts, 1);
    assert!(factory.log.borrow().contains(&"state:Granted".into()));
}

#[test]
fn install_skip_close_and_transport_loss_retain_original_distinct_outcomes() {
    for (event, expected) in [
        (HostEvent::Later, Outcome::Pending),
        (HostEvent::Close, Outcome::Pending),
        (HostEvent::Eof, Outcome::AwaitingEvidence),
    ] {
        let (outcome, _, factory) = before_launch(
            vec![snapshot(
                AccessibilityStatus::Denied,
                ScreenPermissionStatus::Unknown,
            )],
            vec![HostEvent::Ready, event],
        );
        assert_eq!(outcome, expected);
        assert_eq!(factory.starts, 1);
    }
}
