/*
 * [INPUT]: 依赖双行权限状态机的注入操作、目标锁与无窗传输夹具。
 * [OUTPUT]: 验证意图先锁、锁内迟到 grant、缺权 reset 与权限隔离。
 * [POS]: 原生双权限卡的无窗回归；不调用真实 TCC 或宿主。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

struct Ops {
    ax: AccessibilityStatus,
    screen: ScreenPermissionStatus,
    ax_resets: usize,
    screen_resets: usize,
    ax_settings: usize,
    screen_settings: usize,
    screen_intents: usize,
    screen_reads: usize,
    grant_screen_at_read: Option<usize>,
    target_lock_requests: usize,
    action_order: Vec<&'static str>,
    grant_screen_on_open: bool,
    revoke_ax_on_wait: bool,
    revoke_screen_on_wait: bool,
}

impl GuideOps for Ops {
    fn launch(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn probe(&mut self) -> AccessibilityStatus {
        self.ax
    }
    fn probe_screen(&mut self) -> Option<ScreenPermissionStatus> {
        self.screen_reads += 1;
        self.action_order.push("screen-probe");
        if self.grant_screen_at_read == Some(self.screen_reads) {
            self.screen = ScreenPermissionStatus::Granted;
        }
        Some(self.screen)
    }
    fn wait_for_window(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn reset(&mut self) -> Result<(), String> {
        self.ax_resets += 1;
        self.ax = AccessibilityStatus::Granted;
        Ok(())
    }
    fn open_settings(&mut self) -> Result<(), String> {
        self.ax_settings += 1;
        Ok(())
    }
    fn record_screen_intent(&mut self) -> Result<(), String> {
        self.screen_intents += 1;
        self.action_order.push("screen-intent");
        Ok(())
    }
    fn reset_screen(&mut self) -> Result<(), String> {
        self.screen_resets += 1;
        self.action_order.push("screen-reset");
        Ok(())
    }
    fn open_screen_settings(&mut self) -> Result<(), String> {
        assert!(
            self.screen_intents > self.screen_settings,
            "persist intent before Settings can restart the host"
        );
        self.screen_settings += 1;
        self.action_order.push("screen-settings");
        if self.grant_screen_on_open {
            self.screen = ScreenPermissionStatus::Granted;
        }
        Ok(())
    }
    fn lock_target(&mut self) -> Result<Option<incodex_transaction::TargetLock>, String> {
        self.target_lock_requests += 1;
        self.action_order.push("target-lock");
        Ok(None)
    }
    fn wait(&mut self, _: Duration) {
        if std::mem::take(&mut self.revoke_ax_on_wait) {
            self.ax = AccessibilityStatus::Denied;
        }
        if std::mem::take(&mut self.revoke_screen_on_wait) {
            self.screen = ScreenPermissionStatus::NotGranted;
        }
    }
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
    events: VecDeque<HostEvent>,
    log: Rc<RefCell<Vec<String>>>,
    starts: usize,
}
impl GuideHostFactory for Factory {
    fn start(&mut self, _: &Path, _: &Path) -> Result<Box<dyn GuideHost>, String> {
        self.starts += 1;
        Ok(Box::new(Host {
            events: self.events.clone(),
            log: self.log.clone(),
        }))
    }
}

fn run(
    ax: AccessibilityStatus,
    screen: ScreenPermissionStatus,
    events: &[HostEvent],
    grant: bool,
) -> (Outcome, Ops, Factory) {
    run_configured(ax, screen, events, grant, None)
}

fn run_configured(
    ax: AccessibilityStatus,
    screen: ScreenPermissionStatus,
    events: &[HostEvent],
    grant: bool,
    grant_screen_at_read: Option<usize>,
) -> (Outcome, Ops, Factory) {
    let mut ops = Ops {
        ax,
        screen,
        ax_resets: 0,
        screen_resets: 0,
        ax_settings: 0,
        screen_settings: 0,
        screen_intents: 0,
        screen_reads: 0,
        grant_screen_at_read,
        target_lock_requests: 0,
        action_order: Vec::new(),
        grant_screen_on_open: grant,
        revoke_ax_on_wait: false,
        revoke_screen_on_wait: false,
    };
    let mut factory = Factory {
        events: events.iter().cloned().collect(),
        log: Rc::default(),
        starts: 0,
    };
    let result = run_permission_guide_with_timeouts(
        &mut ops,
        Path::new("/fixture"),
        Path::new(OFFICIAL_APP_PATH),
        &mut || Ok(()),
        &mut factory,
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .unwrap();
    (result, ops, factory)
}

#[test]
fn ax_granted_screen_pending_does_not_skip_card_or_report_success() {
    let (result, ops, factory) = run(
        AccessibilityStatus::Granted,
        ScreenPermissionStatus::NotGranted,
        &[HostEvent::Ready, HostEvent::Later],
        false,
    );
    assert_eq!(result, Outcome::Pending);
    assert_eq!(factory.starts, 1);
    assert_eq!(ops.ax_resets, 0);
}

#[test]
fn both_granted_skip_card_and_all_mutations() {
    let (result, ops, factory) = run(
        AccessibilityStatus::Granted,
        ScreenPermissionStatus::Granted,
        &[],
        false,
    );
    assert_eq!(result, Outcome::Granted);
    assert_eq!(factory.starts, 0);
    assert_eq!(
        (ops.ax_resets, ops.ax_settings, ops.screen_settings),
        (0, 0, 0)
    );
}

#[test]
fn explicit_screen_allow_resets_missing_screen_once_before_settings_without_resetting_ax() {
    let (result, ops, _) = run(
        AccessibilityStatus::Granted,
        ScreenPermissionStatus::NotGranted,
        &[HostEvent::Ready, HostEvent::AllowScreen],
        true,
    );
    assert_eq!(result, Outcome::Granted);
    assert_eq!(
        (
            ops.ax_resets,
            ops.screen_resets,
            ops.ax_settings,
            ops.screen_settings,
            ops.screen_intents,
            ops.target_lock_requests,
        ),
        (0, 1, 0, 1, 1, 1)
    );
    let lock = ops
        .action_order
        .iter()
        .position(|action| *action == "target-lock")
        .unwrap();
    let fresh_probe = ops.action_order[lock + 1..]
        .iter()
        .position(|action| *action == "screen-probe")
        .map(|offset| lock + 1 + offset)
        .unwrap();
    let intent = ops
        .action_order
        .iter()
        .position(|action| *action == "screen-intent")
        .unwrap();
    let reset = ops
        .action_order
        .iter()
        .position(|action| *action == "screen-reset")
        .unwrap();
    let settings = ops
        .action_order
        .iter()
        .position(|action| *action == "screen-settings")
        .unwrap();
    assert!(
        intent < lock,
        "publish intent before acquiring the reset lock"
    );
    assert!(lock < fresh_probe && fresh_probe < reset);
    assert!(intent < reset && reset < settings);
}

#[test]
fn late_screen_grant_skips_reset_and_settings() {
    let (result, ops, _) = run_configured(
        AccessibilityStatus::Granted,
        ScreenPermissionStatus::NotGranted,
        &[HostEvent::Ready, HostEvent::AllowScreen],
        false,
        Some(3),
    );
    assert_eq!(result, Outcome::Granted);
    assert_eq!(
        (ops.screen_resets, ops.screen_intents, ops.screen_settings),
        (0, 0, 0)
    );
}

#[test]
fn late_screen_grant_on_locked_recheck_skips_reset_and_settings() {
    use std::path::PathBuf;

    struct Root(PathBuf);
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let root = Root(std::env::temp_dir().canonicalize().unwrap().join(format!(
        "incodex-screen-locked-recheck-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )));
    let app = root.0.join("ChatGPT.app");

    struct LockedRecheckOps<'a> {
        root: &'a Path,
        app: &'a Path,
        screen_reads: usize,
        screen_intents: usize,
        ax_resets: usize,
        screen_resets: usize,
        screen_settings: usize,
        lock_held_during_recheck: bool,
    }
    impl GuideOps for LockedRecheckOps<'_> {
        fn launch(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn probe(&mut self) -> AccessibilityStatus {
            AccessibilityStatus::Granted
        }
        fn probe_screen(&mut self) -> Option<ScreenPermissionStatus> {
            self.screen_reads += 1;
            if self.screen_reads == 4 {
                self.lock_held_during_recheck = incodex_transaction::acquire_target_lock(
                    self.root,
                    self.app,
                    "concurrent-screen-probe",
                    None,
                )
                .is_err();
                assert!(
                    self.lock_held_during_recheck,
                    "the fourth Screen read must run under the target lock"
                );
            }
            Some(if self.screen_reads >= 4 {
                ScreenPermissionStatus::Granted
            } else {
                ScreenPermissionStatus::NotGranted
            })
        }
        fn wait_for_window(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn lock_target(&mut self) -> Result<Option<incodex_transaction::TargetLock>, String> {
            incodex_transaction::acquire_target_lock(self.root, self.app, "permission-reset", None)
                .map(Some)
        }
        fn record_screen_intent(&mut self) -> Result<(), String> {
            assert!(
                incodex_transaction::acquire_target_lock(
                    self.root,
                    self.app,
                    "screen-intent",
                    None
                )
                .is_ok(),
                "intent publication must precede the target lock"
            );
            self.screen_intents += 1;
            Ok(())
        }
        fn reset(&mut self) -> Result<(), String> {
            self.ax_resets += 1;
            Ok(())
        }
        fn reset_screen(&mut self) -> Result<(), String> {
            self.screen_resets += 1;
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

    let mut ops = LockedRecheckOps {
        root: &root.0,
        app: &app,
        screen_reads: 0,
        screen_intents: 0,
        ax_resets: 0,
        screen_resets: 0,
        screen_settings: 0,
        lock_held_during_recheck: false,
    };
    let mut factory = Factory {
        events: [HostEvent::Ready, HostEvent::AllowScreen].into(),
        log: Rc::default(),
        starts: 0,
    };

    let result = run_permission_guide_with_timeouts(
        &mut ops,
        &root.0,
        &app,
        &mut || Ok(()),
        &mut factory,
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .unwrap();

    assert_eq!(result, Outcome::Granted);
    assert!(ops.lock_held_during_recheck);
    assert_eq!(ops.screen_intents, 1);
    assert_eq!(ops.screen_resets, 0);
    assert_eq!(ops.screen_settings, 0);
    assert_eq!(ops.ax_resets, 0);
    assert!(incodex_transaction::acquire_target_lock(&root.0, &app, "after-guide", None).is_ok());
}

#[test]
fn screen_retry_does_not_repeat_a_reset_within_one_guide() {
    let (result, ops, _) = run(
        AccessibilityStatus::Granted,
        ScreenPermissionStatus::NotGranted,
        &[
            HostEvent::Ready,
            HostEvent::AllowScreen,
            HostEvent::RetryScreen,
            HostEvent::Later,
        ],
        false,
    );
    assert_eq!(result, Outcome::Pending);
    assert_eq!(
        (ops.screen_resets, ops.screen_intents, ops.screen_settings),
        (1, 2, 2)
    );
    assert_eq!(ops.ax_resets, 0);
}

#[test]
fn screen_grant_alone_does_not_close_while_ax_is_pending() {
    let (result, ops, factory) = run(
        AccessibilityStatus::Denied,
        ScreenPermissionStatus::NotGranted,
        &[HostEvent::Ready, HostEvent::AllowScreen, HostEvent::Later],
        true,
    );
    assert_eq!(result, Outcome::Pending);
    assert_eq!(ops.ax_resets, 0);
    assert!(!factory
        .log
        .borrow()
        .iter()
        .any(|line| line == "state:Granted"));
}

#[test]
fn screen_allow_rechecks_ax_before_any_completion_notification() {
    struct ChangedAx {
        screen_reads: usize,
        ax: AccessibilityStatus,
    }
    impl GuideOps for ChangedAx {
        fn launch(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn probe(&mut self) -> AccessibilityStatus {
            self.ax
        }
        fn probe_screen(&mut self) -> Option<ScreenPermissionStatus> {
            self.screen_reads += 1;
            if self.screen_reads >= 3 {
                self.ax = AccessibilityStatus::Denied;
                Some(ScreenPermissionStatus::Granted)
            } else {
                Some(ScreenPermissionStatus::NotGranted)
            }
        }
        fn wait_for_window(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn reset(&mut self) -> Result<(), String> {
            panic!("Screen action must not reset AX")
        }
        fn open_settings(&mut self) -> Result<(), String> {
            panic!("Screen action must not open AX")
        }
        fn open_screen_settings(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn wait(&mut self, _: Duration) {}
    }
    let mut ops = ChangedAx {
        screen_reads: 0,
        ax: AccessibilityStatus::Granted,
    };
    let mut factory = Factory {
        events: [HostEvent::Ready, HostEvent::AllowScreen, HostEvent::Later].into(),
        log: Rc::default(),
        starts: 0,
    };
    let result = run_permission_guide_with_timeouts(
        &mut ops,
        Path::new("/fixture"),
        Path::new(OFFICIAL_APP_PATH),
        &mut || Ok(()),
        &mut factory,
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(result, Outcome::Pending);
    let log = factory.log.borrow();
    assert!(
        !log.iter().any(|line| line == "screen:Granted"),
        "a partial grant must not be merged with a stale AX grant: {log:?}"
    );
    assert!(!log.iter().any(|line| line == "state:Granted"));
    assert!(log.iter().any(
        |line| line.contains("accessibility: Denied") && line.contains("screen: Some(Granted)")
    ));
}

#[test]
fn ax_action_retains_existing_screen_grant_and_existing_ax_reset_contract() {
    let (result, ops, _) = run(
        AccessibilityStatus::Denied,
        ScreenPermissionStatus::Granted,
        &[HostEvent::Ready, HostEvent::Allow],
        false,
    );
    assert_eq!(result, Outcome::Granted);
    assert_eq!(
        (
            ops.ax_resets,
            ops.screen_resets,
            ops.ax_settings,
            ops.screen_settings,
        ),
        (1, 0, 1, 0)
    );
}

fn reopen_after_revoke(screen: bool) -> (Ops, Vec<String>) {
    let mut ops = Ops {
        ax: AccessibilityStatus::Denied,
        screen: ScreenPermissionStatus::NotGranted,
        ax_resets: 0,
        screen_resets: 0,
        ax_settings: 0,
        screen_settings: 0,
        screen_intents: 0,
        screen_reads: 0,
        grant_screen_at_read: None,
        target_lock_requests: 0,
        action_order: Vec::new(),
        grant_screen_on_open: screen,
        revoke_ax_on_wait: !screen,
        revoke_screen_on_wait: screen,
    };
    let action = if screen {
        HostEvent::AllowScreen
    } else {
        HostEvent::Allow
    };
    let mut factory = Factory {
        events: [
            HostEvent::Ready,
            action.clone(),
            HostEvent::Timeout,
            action,
            HostEvent::Later,
        ]
        .into(),
        log: Rc::default(),
        starts: 0,
    };
    assert_eq!(
        run_permission_guide_with_timeouts(
            &mut ops,
            Path::new("/fixture"),
            Path::new(OFFICIAL_APP_PATH),
            &mut || Ok(()),
            &mut factory,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .unwrap(),
        Outcome::Pending
    );
    let log = factory.log.borrow().clone();
    (ops, log)
}

#[test]
fn revoked_ax_row_can_allow_again_without_a_second_reset() {
    let (ops, log) = reopen_after_revoke(false);
    assert_eq!(
        (ops.ax_resets, ops.ax_settings, ops.screen_settings),
        (1, 2, 0)
    );
    assert!(log
        .iter()
        .any(|line| line.contains("accessibility: Granted")));
    assert!(log
        .iter()
        .any(|line| line.contains("accessibility: Denied")));
    assert!(!log.iter().any(|line| line.contains("Error")), "{log:?}");
}

#[test]
fn revoked_screen_row_can_allow_again_without_a_second_reset() {
    let (ops, log) = reopen_after_revoke(true);
    assert_eq!(
        (
            ops.ax_resets,
            ops.screen_resets,
            ops.ax_settings,
            ops.screen_settings,
        ),
        (0, 1, 0, 2)
    );
    assert!(log
        .iter()
        .any(|line| line.contains("screen: Some(Granted)")));
    assert!(log
        .iter()
        .any(|line| line.contains("screen: Some(NotGranted)")));
    assert!(!log.iter().any(|line| line.contains("Error")), "{log:?}");
}

#[test]
fn official_or_unavailable_runtime_screen_unknown_is_never_success() {
    let (result, ops, _) = run(
        AccessibilityStatus::Granted,
        ScreenPermissionStatus::Unknown,
        &[
            HostEvent::Ready,
            HostEvent::AllowScreen,
            HostEvent::RetryScreen,
            HostEvent::Later,
        ],
        false,
    );
    assert_eq!(result, Outcome::Pending);
    assert_eq!(ops.ax_resets, 0);
    assert_eq!(ops.screen_resets, 0);
    assert_eq!(ops.screen_settings, 2);
}

#[test]
fn an_adapter_without_a_screen_probe_defaults_to_unknown_not_not_required() {
    struct NoScreenAdapter;
    impl GuideOps for NoScreenAdapter {
        fn launch(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn probe(&mut self) -> AccessibilityStatus {
            AccessibilityStatus::Granted
        }
        fn wait_for_window(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn reset(&mut self) -> Result<(), String> {
            panic!("not used")
        }
        fn open_settings(&mut self) -> Result<(), String> {
            panic!("not used")
        }
        fn wait(&mut self, _: Duration) {}
    }
    assert_eq!(
        NoScreenAdapter.probe_screen(),
        Some(ScreenPermissionStatus::Unknown)
    );
}

#[test]
fn screen_events_are_bounded_and_cannot_smuggle_extra_arguments() {
    assert_eq!(
        decode_host_event(br#"{"nonce":"n","type":"allow-screen"}"#, "n").unwrap(),
        HostEvent::AllowScreen
    );
    assert!(decode_host_event(
        br#"{"nonce":"n","type":"allow-screen","command":"reset"}"#,
        "n"
    )
    .is_err());
}

#[test]
fn screen_response_requires_exact_request_process_and_raw_status() {
    use crate::permission_screen_probe::{decode_response, ScreenRequest};
    let request = ScreenRequest {
        schema_version: 1,
        nonce: "a".repeat(32),
        install_id: "install".into(),
        request_id: "request".into(),
        app_path: OFFICIAL_APP_PATH.into(),
        pid: 123,
        process_start_identity: "Mon Oct 12 01:00:00 2026".into(),
        requested_at_ms: 2000,
    };
    let mut value = serde_json::to_value(&request).unwrap();
    value["status"] = "granted".into();
    value["rawStatus"] = "granted".into();
    value["observedAtMs"] = 2001.into();
    value["versions"] = json!({"electron":"42.0.0","chrome":"155.0.0.0"});
    assert_eq!(
        decode_response(&value, &request),
        ScreenPermissionStatus::Granted
    );
    for (field, replacement) in [
        ("nonce", json!("b".repeat(32))),
        ("pid", json!(124)),
        ("processStartIdentity", json!("changed")),
        ("installId", json!("old")),
        ("requestId", json!("old")),
        ("rawStatus", json!("denied")),
        ("observedAtMs", json!(1999)),
    ] {
        let mut stale = value.clone();
        stale[field] = replacement;
        assert_eq!(
            decode_response(&stale, &request),
            ScreenPermissionStatus::Unknown,
            "{field}"
        );
    }
    value["rawStatus"] = "denied".into();
    value["status"] = "not-granted".into();
    assert_eq!(
        decode_response(&value, &request),
        ScreenPermissionStatus::NotGranted
    );
}

#[path = "permission_update_flow_tests.rs"]
mod after_update;

#[test]
fn screen_intent_is_recorded_before_settings_and_eof_is_not_skip() {
    let (outcome, ops, _) = run(
        AccessibilityStatus::Granted,
        ScreenPermissionStatus::NotGranted,
        &[HostEvent::Ready, HostEvent::AllowScreen, HostEvent::Eof],
        false,
    );
    assert_eq!(
        ops.screen_intents, 1,
        "persist the human intent before Settings can restart the host"
    );
    assert_eq!(ops.screen_settings, 1);
    assert_eq!(
        outcome,
        Outcome::AwaitingEvidence,
        "transport EOF is not a human Skip"
    );
    let (dismissed, _, _) = run(
        AccessibilityStatus::Granted,
        ScreenPermissionStatus::NotGranted,
        &[HostEvent::Ready, HostEvent::AllowScreen, HostEvent::Later],
        false,
    );
    assert_eq!(dismissed, Outcome::Pending);
}

#[test]
fn presenter_transport_eof_is_awaiting_evidence_not_user_deferred() {
    let (outcome, _, _) = run(
        AccessibilityStatus::Granted,
        ScreenPermissionStatus::NotGranted,
        &[HostEvent::Ready, HostEvent::Eof],
        false,
    );
    assert_eq!(outcome, Outcome::AwaitingEvidence);
    let (dismissed, _, _) = run(
        AccessibilityStatus::Granted,
        ScreenPermissionStatus::NotGranted,
        &[HostEvent::Ready, HostEvent::Close],
        false,
    );
    assert_eq!(dismissed, Outcome::Pending);
}
