/*
 * [INPUT]: 依赖共用无窗 guide 夹具与真实更新状态机。
 * [OUTPUT]: 验证更新不唤起 App、unknown 不弹卡及进程换代拒绝。
 * [POS]: 自动更新权限分支的红绿回归，不访问实际宿主或 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;
use std::cell::Cell;

// ---- 更新后的非干扰检查 ----
struct UpdateOps {
    states: VecDeque<(AccessibilityStatus, ScreenPermissionStatus)>,
    launches: usize,
    waits: usize,
    invalidated: Option<Rc<Cell<bool>>>,
}
impl GuideOps for UpdateOps {
    fn launch(&mut self) -> Result<(), String> {
        self.launches += 1;
        Ok(())
    }
    fn probe(&mut self) -> AccessibilityStatus {
        self.states.front().unwrap().0
    }
    fn probe_screen(&mut self) -> Option<ScreenPermissionStatus> {
        let value = self.states.front().unwrap().1;
        if value == ScreenPermissionStatus::Granted {
            if let Some(flag) = &self.invalidated {
                flag.set(true);
            }
        }
        if self.states.len() > 1 {
            self.states.pop_front();
        }
        Some(value)
    }
    fn wait_for_window(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn reset(&mut self) -> Result<(), String> {
        panic!("更新检查不得自动 reset")
    }
    fn open_settings(&mut self) -> Result<(), String> {
        panic!("更新检查不得自动开设置")
    }
    fn wait(&mut self, _: Duration) {
        self.waits += 1;
    }
}
fn after_update(
    states: &[(AccessibilityStatus, ScreenPermissionStatus)],
) -> (Outcome, UpdateOps, Factory) {
    let mut ops = UpdateOps {
        states: states.iter().cloned().collect(),
        launches: 0,
        waits: 0,
        invalidated: None,
    };
    let mut factory = Factory {
        events: [HostEvent::Ready, HostEvent::Later].into(),
        log: Rc::default(),
        starts: 0,
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
        flow::GuideMode::AfterUpdate {
            check_timeout: Duration::from_millis(2),
        },
    )
    .unwrap();
    (result, ops, factory)
}
#[test]
fn update_both_granted_does_not_launch_prompt_or_reset() {
    let (result, ops, factory) = after_update(&[(
        AccessibilityStatus::Granted,
        ScreenPermissionStatus::Granted,
    )]);
    assert_eq!(result, Outcome::Granted);
    assert_eq!((ops.launches, factory.starts), (0, 0));
}
#[test]
fn update_confirmed_missing_opens_shared_card_without_launch_or_reset() {
    let (result, ops, factory) = after_update(&[(
        AccessibilityStatus::Granted,
        ScreenPermissionStatus::NotGranted,
    )]);
    assert_eq!(result, Outcome::Pending);
    assert_eq!(factory.starts, 1);
    assert_eq!(ops.launches, 0);
    assert!(factory
        .log
        .borrow()
        .iter()
        .any(|line| line.contains("accessibility: Granted")
            && line.contains("screen: Some(NotGranted)")));
}
#[test]
fn update_unknown_screen_is_pending_without_an_authorization_card() {
    let (result, ops, factory) = after_update(&[(
        AccessibilityStatus::Granted,
        ScreenPermissionStatus::Unknown,
    )]);
    assert_eq!(result, Outcome::AwaitingEvidence);
    assert_eq!((ops.launches, factory.starts), (0, 0));
    assert!(ops.waits > 0);
}
#[test]
fn update_delayed_grant_does_not_flash_a_card() {
    let (result, ops, factory) = after_update(&[
        (
            AccessibilityStatus::Granted,
            ScreenPermissionStatus::Unknown,
        ),
        (
            AccessibilityStatus::Granted,
            ScreenPermissionStatus::Granted,
        ),
    ]);
    assert_eq!(result, Outcome::Granted);
    assert_eq!((ops.launches, factory.starts), (0, 0));
}
#[test]
fn update_rechecks_missing_permission_before_presenting() {
    let (result, ops, factory) = after_update(&[
        (
            AccessibilityStatus::Granted,
            ScreenPermissionStatus::NotGranted,
        ),
        (
            AccessibilityStatus::Granted,
            ScreenPermissionStatus::Granted,
        ),
    ]);
    assert_eq!(result, Outcome::Granted);
    assert_eq!((ops.launches, factory.starts), (0, 0));
}
#[test]
fn update_withdraws_card_if_fresh_probe_becomes_unknown() {
    let (result, ops, factory) = after_update(&[
        (
            AccessibilityStatus::Granted,
            ScreenPermissionStatus::NotGranted,
        ),
        (
            AccessibilityStatus::Granted,
            ScreenPermissionStatus::Unknown,
        ),
    ]);
    assert_eq!(result, Outcome::AwaitingEvidence);
    assert_eq!((ops.launches, factory.starts), (0, 0));
}

#[test]
fn update_rejects_a_grant_read_from_a_replaced_process() {
    let invalidated = Rc::new(Cell::new(false));
    let mut ops = UpdateOps {
        states: [
            (
                AccessibilityStatus::Granted,
                ScreenPermissionStatus::NotGranted,
            ),
            (
                AccessibilityStatus::Granted,
                ScreenPermissionStatus::NotGranted,
            ),
            (
                AccessibilityStatus::Granted,
                ScreenPermissionStatus::Granted,
            ),
        ]
        .into(),
        launches: 0,
        waits: 0,
        invalidated: Some(invalidated.clone()),
    };
    let mut factory = Factory {
        events: [HostEvent::Ready, HostEvent::Timeout].into(),
        log: Rc::default(),
        starts: 0,
    };
    let result = flow::run_permission_guide_with_mode(
        &mut ops,
        Path::new("/fixture"),
        Path::new(OFFICIAL_APP_PATH),
        &mut || {
            if invalidated.get() {
                Err("host generation changed".into())
            } else {
                Ok(())
            }
        },
        &mut factory,
        GuideTimeouts {
            ready: Duration::from_secs(1),
            choice: Some(Duration::from_secs(1)),
            guide: Some(Duration::from_secs(1)),
        },
        flow::GuideMode::AfterUpdate {
            check_timeout: Duration::from_secs(1),
        },
    );
    assert_eq!(result, Err("host generation changed".into()));
    assert!(!factory
        .log
        .borrow()
        .iter()
        .any(|line| line == "state:Granted"));
}

#[test]
fn explicit_rejects_a_grant_after_permission_identity_retires() {
    let invalidated = Rc::new(Cell::new(false));
    let mut ops = UpdateOps {
        states: [
            (
                AccessibilityStatus::Denied,
                ScreenPermissionStatus::NotGranted,
            ),
            (
                AccessibilityStatus::Granted,
                ScreenPermissionStatus::Granted,
            ),
        ]
        .into(),
        launches: 0,
        waits: 0,
        invalidated: Some(invalidated.clone()),
    };
    let mut factory = Factory {
        events: [HostEvent::Ready, HostEvent::Later].into(),
        log: Rc::default(),
        starts: 0,
    };
    let result = flow::run_permission_guide_with_mode(
        &mut ops,
        Path::new("/fixture"),
        Path::new(OFFICIAL_APP_PATH),
        &mut || {
            if invalidated.get() {
                Err("permission target identity retired".into())
            } else {
                Ok(())
            }
        },
        &mut factory,
        GuideTimeouts::for_user(),
        flow::GuideMode::Explicit,
    );

    assert_eq!(
        result,
        Err("permission target identity retired".into()),
        "a grant snapshot must not escape after install/signing identity retires"
    );
    assert_eq!((ops.launches, factory.starts), (1, 0));
    assert!(!factory
        .log
        .borrow()
        .iter()
        .any(|line| line == "state:Granted"));
}

#[test]
fn shipped_human_wait_policy_has_no_authorization_countdown() {
    let limits = flow::GuideTimeouts::for_user();
    assert!(
        limits.choice.is_none(),
        "human choices cannot expire after five minutes"
    );
    assert!(
        limits.guide.is_none(),
        "System Settings and passwords cannot expire after two minutes"
    );
    assert!(
        limits.ready > Duration::ZERO,
        "machine startup must remain bounded"
    );
}

#[test]
fn reset_holds_the_app_lock_but_settings_handoff_releases_it() {
    use std::path::PathBuf;
    struct Root(PathBuf);
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let root = Root(std::env::temp_dir().canonicalize().unwrap().join(format!(
        "incodex-permission-short-lock-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())));
    let app = root.0.join("ChatGPT.app");
    struct LockedOps<'a> {
        root: &'a Path,
        app: &'a Path,
        reset: bool,
        handoff: bool,
    }
    impl GuideOps for LockedOps<'_> {
        fn launch(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn probe(&mut self) -> AccessibilityStatus {
            if self.reset {
                AccessibilityStatus::Granted
            } else {
                AccessibilityStatus::Denied
            }
        }
        fn probe_screen(&mut self) -> Option<ScreenPermissionStatus> {
            Some(ScreenPermissionStatus::Granted)
        }
        fn wait_for_window(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn lock_target(&mut self) -> Result<Option<incodex_transaction::TargetLock>, String> {
            incodex_transaction::acquire_target_lock(self.root, self.app, "permission-reset", None)
                .map(Some)
        }
        fn reset(&mut self) -> Result<(), String> {
            assert!(
                incodex_transaction::acquire_target_lock(
                    self.root,
                    self.app,
                    "concurrent-update",
                    None
                )
                .is_err(),
                "reset must own the App transaction lock"
            );
            self.reset = true;
            Ok(())
        }
        fn open_settings(&mut self) -> Result<(), String> {
            assert!(
                incodex_transaction::acquire_target_lock(
                    self.root,
                    self.app,
                    "concurrent-update",
                    None
                )
                .is_ok(),
                "waiting for a human must release the App transaction lock"
            );
            self.handoff = true;
            Ok(())
        }
        fn wait(&mut self, _: Duration) {}
    }
    let mut ops = LockedOps {
        root: &root.0,
        app: &app,
        reset: false,
        handoff: false,
    };
    let mut factory = Factory {
        events: [HostEvent::Ready, HostEvent::Allow, HostEvent::Later].into(),
        log: Rc::default(),
        starts: 0,
    };
    let result = flow::run_permission_guide_with_mode(
        &mut ops,
        &root.0,
        &app,
        &mut || Ok(()),
        &mut factory,
        flow::GuideTimeouts::for_user(),
        flow::GuideMode::Explicit,
    )
    .unwrap();
    assert_eq!(result, Outcome::Granted);
    assert!(ops.reset && ops.handoff);
}
