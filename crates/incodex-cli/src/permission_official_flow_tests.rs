/*
 * [INPUT]: 依赖权限卡协议与无窗宿主、权限操作夹具。
 * [OUTPUT]: 验证官方原版拖放收据仅结束引导，不生成录屏授权。
 * [POS]: 卸载后引导的状态机回归；不调用真实 TCC 或系统设置。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

const RECEIPT: &[u8] = br#"{"nonce":"n","type":"settings-window-drop-received"}"#;
fn receipt() -> HostEvent {
    decode_host_event(RECEIPT, "n").unwrap_or_else(HostEvent::Error)
}
struct Ops {
    ax: AccessibilityStatus,
    grant_at: usize,
    reads: usize,
    trace: Vec<GuideTraceEvent>,
    resets: usize,
    settings: usize,
    fail_settings: bool,
}
impl GuideOps for Ops {
    fn launch(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn probe(&mut self) -> AccessibilityStatus {
        self.reads += 1;
        if self.reads >= self.grant_at {
            self.ax = AccessibilityStatus::Granted;
        }
        self.ax
    }
    fn observe(&mut self, event: GuideTraceEvent) {
        self.trace.push(event);
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
        self.settings += 1;
        if self.fail_settings {
            Err("fixture settings failed".into())
        } else {
            Ok(())
        }
    }
    fn wait(&mut self, _: Duration) {}
}
struct Host {
    events: VecDeque<HostEvent>,
    log: Rc<RefCell<Vec<String>>>,
}
impl GuideHost for Host {
    fn send_state(&mut self, state: HostState) -> Result<(), String> {
        self.log.borrow_mut().push(format!("state:{state:?}"));
        Ok(())
    }
    fn send_permissions(&mut self, snapshot: &PermissionSnapshot) -> Result<(), String> {
        self.log
            .borrow_mut()
            .push(format!("permissions:{snapshot:?}"));
        Ok(())
    }
    fn send_permission_state(&mut self, permission: &str, state: HostState) -> Result<(), String> {
        self.log
            .borrow_mut()
            .push(format!("{permission}:{state:?}"));
        Ok(())
    }
    fn poll(&mut self, _: Duration) -> Result<HostEvent, String> {
        Ok(self.events.pop_front().unwrap_or(HostEvent::Later))
    }
    fn close(&mut self) {
        self.log.borrow_mut().push("close".into());
    }
}
struct Factory {
    host: Option<Host>,
    manual: bool,
}
impl GuideHostFactory for Factory {
    fn screen_verification(&self) -> ScreenVerification {
        if self.manual {
            ScreenVerification::Manual
        } else {
            ScreenVerification::Probe
        }
    }
    fn start(&mut self, _: &Path, _: &Path) -> Result<Box<dyn GuideHost>, String> {
        Ok(Box::new(self.host.take().unwrap()))
    }
}
fn run(
    grant_at: usize,
    events: Vec<HostEvent>,
    manual: bool,
    before_launch: bool,
    fail_settings: bool,
) -> (Result<Outcome, String>, Ops, Vec<String>) {
    let mut ops = Ops {
        ax: AccessibilityStatus::Denied,
        grant_at,
        reads: 0,
        trace: vec![],
        resets: 0,
        settings: 0,
        fail_settings,
    };
    let log = Rc::new(RefCell::new(vec![]));
    let mut factory = Factory {
        manual,
        host: Some(Host {
            events: events.into(),
            log: log.clone(),
        }),
    };
    let result = flow::run_permission_guide_with_mode(
        &mut ops,
        Path::new("/fixture"),
        Path::new(OFFICIAL_APP_PATH),
        &mut || Ok(()),
        &mut factory,
        GuideTimeouts {
            ready: Duration::from_secs(1),
            choice: Some(Duration::from_secs(1)),
            guide: Some(Duration::from_secs(1)),
        },
        if before_launch {
            flow::GuideMode::BeforeLaunch
        } else {
            flow::GuideMode::Explicit
        },
    );
    let output = log.borrow().clone();
    (result, ops, output)
}
fn assert_unverified(result: Result<Outcome, String>, ops: &Ops, log: &[String]) {
    assert_eq!(result, Ok(Outcome::AwaitingEvidence));
    assert_eq!(ops.resets, 0);
    assert!(!ops.trace.contains(&GuideTraceEvent::CardCompleted));
    assert!(!log
        .iter()
        .any(|v| v == "state:Granted" || v == "screen:Granted"));
    assert!(
        log.iter().any(|v| v.contains("screen: Some(Unknown)")),
        "{log:?}"
    );
    assert_eq!(log.last().map(String::as_str), Some("close"));
}
#[test]
fn official_receipt_is_nonce_bound_and_has_no_extra_fields() {
    assert!(decode_host_event(RECEIPT, "n").is_ok());
    assert!(decode_host_event(RECEIPT, "other").is_err());
    assert!(decode_host_event(
        br#"{"nonce":"n","type":"settings-window-drop-received","granted":true}"#,
        "n"
    )
    .is_err());
}
#[test]
fn official_drop_after_ax_grant_closes_without_claiming_screen_grant() {
    let (result, ops, log) = run(
        1,
        vec![HostEvent::Ready, HostEvent::AllowScreen, receipt()],
        true,
        false,
        false,
    );
    assert_unverified(result, &ops, &log);
    assert_eq!(ops.settings, 1);
}
#[test]
fn official_drop_before_ax_grant_retracts_helper_and_keeps_ax_live() {
    let (result, ops, log) = run(
        6,
        vec![
            HostEvent::Ready,
            HostEvent::AllowScreen,
            receipt(),
            HostEvent::Timeout,
            HostEvent::Timeout,
        ],
        true,
        false,
        false,
    );
    assert_unverified(result, &ops, &log);
    assert!(log.iter().any(|v| v == "screen:ManualSubmitted"), "{log:?}");
    assert!(ops.reads >= 6);
}
#[test]
fn receipt_without_screen_settings_request_is_rejected() {
    let (result, ops, _) = run(1, vec![HostEvent::Ready, receipt()], true, false, false);
    assert!(result.is_err());
    assert_eq!(ops.settings, 0);
    assert_eq!(ops.resets, 0);
}
#[test]
fn installed_and_install_preflight_never_accept_manual_receipts() {
    for (manual, before_launch) in [(false, false), (true, true)] {
        let (result, ops, log) = run(
            usize::MAX,
            vec![HostEvent::Ready, HostEvent::AllowScreen, receipt()],
            manual,
            before_launch,
            false,
        );
        assert!(result.is_err(), "{manual} {before_launch}: {result:?}");
        assert!(!ops.trace.contains(&GuideTraceEvent::CardCompleted));
        assert!(!log.iter().any(|v| v == "screen:ManualSubmitted"));
    }
}
#[test]
fn failed_settings_launch_cannot_accept_a_drop_receipt() {
    let (result, ops, log) = run(
        1,
        vec![
            HostEvent::Ready,
            HostEvent::AllowScreen,
            receipt(),
            HostEvent::Later,
        ],
        true,
        false,
        true,
    );
    assert_eq!(result, Err("fixture settings failed".into()));
    assert!(!ops.trace.contains(&GuideTraceEvent::CardCompleted));
    assert!(!log.iter().any(|v| v == "screen:ManualSubmitted"));
}
