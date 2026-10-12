/*
 * [INPUT]: 依赖 CLI 权限操作与有界原生传输。
 * [OUTPUT]: 驱动逐行引导；更新后缺失才弹卡，双项均通过才完成。
 * [POS]: 共用引导状态机；AX 保留既有 reset，录屏只打开设置。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScreenPermissionStatus {
    Granted,
    NotGranted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PermissionSnapshot {
    pub(crate) accessibility: AccessibilityStatus,
    pub(crate) screen: Option<ScreenPermissionStatus>,
}
impl PermissionSnapshot {
    fn read(ops: &mut impl GuideOps, accessibility: AccessibilityStatus) -> Self {
        Self {
            accessibility,
            screen: ops.probe_screen(),
        }
    }
    fn probe(ops: &mut impl GuideOps) -> Self {
        ops.snapshot()
    }
    fn granted(&self) -> bool {
        self.accessibility == AccessibilityStatus::Granted
            && self
                .screen
                .is_none_or(|state| state == ScreenPermissionStatus::Granted)
    }
    pub(super) fn json(&self) -> Value {
        let status = |granted, unknown| {
            if granted {
                "granted"
            } else if unknown {
                "unknown"
            } else {
                "not-granted"
            }
        };
        json!({"accessibility": status(self.accessibility == AccessibilityStatus::Granted,
            matches!(self.accessibility, AccessibilityStatus::Unknown | AccessibilityStatus::NotRunning)),
            "screen": status(self.screen == Some(ScreenPermissionStatus::Granted),
                self.screen == Some(ScreenPermissionStatus::Unknown) || self.screen.is_none())})
    }
}

pub(crate) fn run_permission_guide_with<O, F, H>(
    ops: &mut O,
    root: &Path,
    app: &Path,
    verify_target: &mut F,
    factory: &mut H,
    guide_timeout: Duration,
) -> Result<Outcome, String>
where
    O: GuideOps,
    F: FnMut() -> Result<(), String>,
    H: GuideHostFactory,
{
    run_permission_guide_with_timeouts(
        ops,
        root,
        app,
        verify_target,
        factory,
        HOST_CHOICE_TIMEOUT,
        guide_timeout,
    )
}

pub(crate) fn run_permission_guide_with_timeouts<O, F, H>(
    ops: &mut O,
    root: &Path,
    app: &Path,
    verify_target: &mut F,
    factory: &mut H,
    choice_timeout: Duration,
    guide_timeout: Duration,
) -> Result<Outcome, String>
where
    O: GuideOps,
    F: FnMut() -> Result<(), String>,
    H: GuideHostFactory,
{
    run_permission_guide_with_limits(
        ops,
        root,
        app,
        verify_target,
        factory,
        GuideTimeouts {
            ready: HOST_READY_TIMEOUT,
            choice: choice_timeout,
            guide: guide_timeout,
        },
    )
}

pub(crate) struct GuideTimeouts {
    pub(crate) ready: Duration,
    pub(crate) choice: Duration,
    pub(crate) guide: Duration,
}

pub(crate) fn run_permission_guide_with_limits<O, F, H>(
    ops: &mut O,
    root: &Path,
    app: &Path,
    verify_target: &mut F,
    factory: &mut H,
    limits: GuideTimeouts,
) -> Result<Outcome, String>
where
    O: GuideOps,
    F: FnMut() -> Result<(), String>,
    H: GuideHostFactory,
{
    run_permission_guide_with_mode(
        ops,
        root,
        app,
        verify_target,
        factory,
        limits,
        GuideMode::Explicit,
    )
}

pub(crate) enum GuideMode {
    Explicit,
    AfterUpdate { check_timeout: Duration },
}

pub(crate) fn run_permission_guide_with_mode<O, F, H>(
    ops: &mut O,
    root: &Path,
    app: &Path,
    verify_target: &mut F,
    factory: &mut H,
    limits: GuideTimeouts,
    mode: GuideMode,
) -> Result<Outcome, String>
where
    O: GuideOps,
    F: FnMut() -> Result<(), String>,
    H: GuideHostFactory,
{
    verify_target()?;
    // ---- 只读更新检查与显式引导共用后续状态机 ----
    let check_deadline = match mode {
        GuideMode::Explicit => {
            ops.launch()?;
            None
        }
        GuideMode::AfterUpdate { check_timeout } => Some(Instant::now() + check_timeout),
    };
    let read = |ops: &mut O, verify_target: &mut F| -> Result<Option<PermissionSnapshot>, String> {
        if let Some(deadline) = check_deadline {
            return update_permissions(ops, verify_target, deadline);
        }
        let ax = wait_for_decision(ops)?;
        Ok(Some(PermissionSnapshot::read(ops, ax)))
    };
    let Some(mut permissions) = read(ops, verify_target)? else {
        return Ok(Outcome::Pending);
    };
    if permissions.granted() {
        return Ok(Outcome::Granted);
    }

    // 主进程存在不等于窗口可呈现。窗口就绪后再次读取，防止短暂未知或
    // 用户刚授予的权限导致闪卡。更新检查始终沿用同一个检查截止时间。
    ops.wait_for_window()?;
    let Some(fresh) = read(ops, verify_target)? else {
        return Ok(Outcome::Pending);
    };
    permissions = fresh;
    if permissions.granted() {
        return Ok(Outcome::Granted);
    }
    if check_deadline.is_some() {
        verify_target()?;
    }

    let mut host = factory
        .start_with_permissions(root, app, &permissions)
        .map_err(|error| format!("native Accessibility guide could not start: {error}"))?;
    let ready_deadline = Instant::now() + limits.ready;
    let mut ready = false;
    while Instant::now() < ready_deadline {
        let event = host.poll(Duration::from_millis(250))?;
        // A ready event is useful only if the native window became ready
        // within its own bounded presentation deadline.
        if Instant::now() >= ready_deadline && matches!(&event, HostEvent::Ready) {
            break;
        }
        match event {
            HostEvent::AppLocale(_) => {
                host.close();
                return Err(
                    "native Accessibility guide repeated its locale after configuration".into(),
                );
            }
            HostEvent::Ready => {
                ready = true;
                break;
            }
            HostEvent::Timeout => {
                let permissions = PermissionSnapshot::probe(ops);
                if check_deadline.is_some() {
                    verify_target()?;
                }
                if Instant::now() >= ready_deadline {
                    break;
                }
                if permissions.granted() {
                    let _ = host.send_state(HostState::Granted);
                    host.close();
                    return Ok(Outcome::Granted);
                }
            }
            HostEvent::Later | HostEvent::Close | HostEvent::Eof => {
                host.close();
                return Ok(Outcome::Pending);
            }
            HostEvent::Error(message) => {
                host.close();
                return Err(format!(
                    "native Accessibility guide failed before becoming ready: {message}"
                ));
            }
            HostEvent::Allow
            | HostEvent::Retry
            | HostEvent::AllowScreen
            | HostEvent::RetryScreen => {
                let _ = host.send_state(HostState::Error(
                    "native Accessibility guide sent a choice before ready".into(),
                ));
                host.close();
                return Ok(Outcome::Pending);
            }
        }
    }
    if !ready {
        let _ = host.send_state(HostState::Error(
            "native Accessibility guide did not become ready in time".into(),
        ));
        host.close();
        return Ok(Outcome::Pending);
    }
    // Do not charge authentication/native-window presentation time against
    // the post-Allow Accessibility polling window.  A separate choice
    // deadline prevents a native view that never receives a decision from
    // living forever, while a later Retry does not renew the post-Allow
    // deadline.
    let choice_deadline = Instant::now() + limits.choice;
    let mut guide_deadline = None;
    let mut reset_performed = false;
    let mut ax_granted_after_reset = false;
    let mut screen_requested = false;
    host.send_permissions(&permissions)?;
    loop {
        let deadline = guide_deadline.unwrap_or(choice_deadline);
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let poll_timeout = (deadline - now).min(Duration::from_millis(250));
        let event = host.poll(poll_timeout)?;
        // A misbehaving host may return an event after the bounded poll
        // interval.  Do not accept a late Allow/Retry from the pre-choice
        // phase (or silently extend the post-Allow phase).
        if Instant::now() >= deadline {
            break;
        }
        match event {
            HostEvent::AppLocale(_) => {
                host.close();
                return Err(
                    "native Accessibility guide repeated its locale after configuration".into(),
                );
            }
            HostEvent::Ready | HostEvent::Timeout => {}
            HostEvent::Allow if !reset_performed => {
                // The native guide only expresses user intent.  Revalidate
                // the target in the CLI immediately before the destructive
                // operation, then perform that operation exactly once.  Tell
                // the user that verification is in progress first. Local
                // APFS uses the session's verified-target continuity guard;
                // unsupported targets retain the full verifier fallback.
                host.send_permission_state("accessibility", HostState::Repairing)?;
                verify_target()?;
                // A user may have approved the system row while the guide was
                // animating.  Only an explicit denied probe authorizes reset;
                // unknown/not-running is an identity/readiness failure.
                // Keep this probe and reset adjacent: no UI handoff or slow
                // signature inventory belongs between the two operations.
                match ops.probe() {
                    AccessibilityStatus::Granted => {
                        permissions = PermissionSnapshot::read(ops, AccessibilityStatus::Granted);
                        if check_deadline.is_some() {
                            verify_target()?;
                        }
                        if Instant::now() >= deadline {
                            break;
                        }
                        host.send_permissions(&permissions)?;
                        if permissions.granted() {
                            host.send_state(HostState::Granted)?;
                            host.close();
                            return Ok(Outcome::Granted);
                        }
                        host.send_permission_state("accessibility", HostState::Granted)?;
                        continue;
                    }
                    AccessibilityStatus::Denied => {}
                    AccessibilityStatus::Unknown | AccessibilityStatus::NotRunning => {
                        let _ = host.send_state(HostState::Error(
                            "Accessibility status was not a confirmed denial; no reset was performed".into(),
                        ));
                        host.close();
                        return Ok(Outcome::Pending);
                    }
                }
                if let Err(error) = ops.reset() {
                    show_error_until_dismissed(&mut *host, error.clone(), limits.choice);
                    return Err(error);
                }
                reset_performed = true;
                if let Err(error) = ops.open_settings() {
                    show_error_until_dismissed(&mut *host, error.clone(), limits.choice);
                    return Err(error);
                }
                // Only charge the two-minute handoff window once Settings
                // has actually opened.  Target revalidation and TCC reset
                // are synchronous CLI work after the user's choice and must
                // not consume the system-approval polling budget.
                if guide_deadline.is_none() {
                    guide_deadline = Some(Instant::now() + limits.guide);
                }
                host.send_permission_state("accessibility", HostState::AwaitingUser)?;
            }
            HostEvent::Allow | HostEvent::Retry
                if reset_performed
                    && (matches!(event, HostEvent::Retry)
                        || (ax_granted_after_reset
                            && permissions.accessibility != AccessibilityStatus::Granted)) =>
            {
                // 撤销后的 Allow 与 Back 后的 Retry 只重开设置。
                // 已执行的 reset 永不复位，重复 Allow 仍不能重置。
                ax_granted_after_reset = false;
                host.send_permission_state("accessibility", HostState::Repairing)?;
                verify_target()?;
                if let Err(error) = ops.open_settings() {
                    show_error_until_dismissed(&mut *host, error.clone(), limits.choice);
                    return Err(error);
                }
                host.send_permission_state("accessibility", HostState::AwaitingUser)?;
            }
            HostEvent::Allow => {
                let _ = host.send_state(HostState::Error(
                    "native Accessibility guide repeated Allow after reset".into(),
                ));
                host.close();
                return Ok(Outcome::Pending);
            }
            HostEvent::Retry => {
                let _ = host.send_state(HostState::Error(
                    "native Accessibility guide sent Retry before initial Allow".into(),
                ));
                host.close();
                return Ok(Outcome::Pending);
            }
            HostEvent::AllowScreen | HostEvent::RetryScreen => {
                if permissions.screen.is_none()
                    || (matches!(event, HostEvent::AllowScreen) && screen_requested)
                    || (matches!(event, HostEvent::RetryScreen) && !screen_requested)
                {
                    let _ = host.send_state(HostState::Error(
                        "invalid Screen Recording action sequence".into(),
                    ));
                    host.close();
                    return Ok(Outcome::Pending);
                }
                host.send_permission_state("screen", HostState::Repairing)?;
                verify_target()?;
                if ops.probe_screen() != Some(ScreenPermissionStatus::Granted) {
                    // 未获准或未知只授权设置导航，不授权清除系统决定。
                    if let Err(error) = ops.open_screen_settings() {
                        show_error_until_dismissed(&mut *host, error.clone(), limits.choice);
                        return Err(error);
                    }
                    screen_requested = true;
                    if guide_deadline.is_none() {
                        guide_deadline = Some(Instant::now() + limits.guide);
                    }
                    host.send_permission_state("screen", HostState::AwaitingUser)?;
                }
            }
            HostEvent::Later | HostEvent::Close | HostEvent::Eof => {
                host.close();
                return Ok(Outcome::Pending);
            }
            HostEvent::Error(message) => {
                host.close();
                return Err(format!(
                    "native Accessibility guide reported an error: {message}"
                ));
            }
        }

        permissions = PermissionSnapshot::probe(ops);
        if check_deadline.is_some() {
            verify_target()?;
        }
        // 状态读取也占用本轮时限；晚到的读数不续期。
        if Instant::now() >= guide_deadline.unwrap_or(choice_deadline) {
            break;
        }
        if reset_performed && permissions.accessibility == AccessibilityStatus::Granted {
            ax_granted_after_reset = true;
        }
        if permissions.screen == Some(ScreenPermissionStatus::Granted) {
            // 原生行在获准后清除 Retry；撤销后允许新的设置导航。
            screen_requested = false;
        }
        host.send_permissions(&permissions)?;
        if permissions.granted() {
            host.send_state(HostState::Granted)?;
            host.close();
            return Ok(Outcome::Granted);
        }
        ops.wait(if guide_deadline.is_some() {
            POST_ALLOW_PROBE_INTERVAL
        } else {
            INITIAL_PROBE_INTERVAL
        });
    }

    let message = if guide_deadline.is_some() {
        "Permission verification timed out. Any unverified permission remains pending; an unavailable Screen Recording probe is not a denial.".into()
    } else {
        "native Accessibility guide did not receive a user choice in time".into()
    };
    show_error_until_dismissed(&mut *host, message, limits.choice);
    Ok(Outcome::Pending)
}

/// Keep the native error page alive long enough for a user to read it or
/// dismiss it.  The timeout remains bounded, and no further choices can
/// authorize a reset while the presenter is in its non-retryable error state.
fn show_error_until_dismissed(host: &mut dyn GuideHost, message: String, timeout: Duration) {
    if host.send_state(HostState::Error(message)).is_err() {
        host.close();
        return;
    }

    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match host.poll(remaining.min(Duration::from_millis(250))) {
            Ok(HostEvent::Later | HostEvent::Close | HostEvent::Eof | HostEvent::Error(_))
            | Err(_) => break,
            // Error is a terminal presenter state. Ignore any stale or
            // malformed choice rather than starting another operation.
            Ok(_) => {}
        }
    }
    host.close();
}

pub(super) fn wait_for_decision(ops: &mut impl GuideOps) -> Result<AccessibilityStatus, String> {
    for _ in 0..INITIAL_PROBE_ATTEMPTS {
        match ops.probe() {
            status @ (AccessibilityStatus::Granted | AccessibilityStatus::Denied) => {
                return Ok(status)
            }
            AccessibilityStatus::Unknown | AccessibilityStatus::NotRunning => {
                ops.wait(INITIAL_PROBE_INTERVAL)
            }
        }
    }
    Err("The target app's identity or Accessibility permission remained unavailable; no reset was performed.".into())
}

// ---- 更新后只读预检 ----
fn update_permissions<O: GuideOps, F: FnMut() -> Result<(), String>>(
    ops: &mut O,
    verify_target: &mut F,
    deadline: Instant,
) -> Result<Option<PermissionSnapshot>, String> {
    loop {
        if Instant::now() >= deadline {
            return Ok(None);
        }
        verify_target()?;
        let snapshot = PermissionSnapshot::probe(ops);
        verify_target()?;
        if Instant::now() >= deadline {
            return Ok(None);
        }
        let missing = snapshot.accessibility == AccessibilityStatus::Denied
            || snapshot.screen == Some(ScreenPermissionStatus::NotGranted);
        if snapshot.granted() || missing {
            return Ok(Some(snapshot));
        }
        ops.wait(INITIAL_PROBE_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
    }
}
