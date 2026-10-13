/*
 * [INPUT]: 依赖 CLI 权限操作、原生传输、目标身份验证与转场观察器。
 * [OUTPUT]: 仅真实 Later/Close 返回 Pending；机器等待保留证据，非法序列返回错误。
 * [POS]: 共用权限状态机；AX reset 持短锁，系统设置等待不持锁。
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
        ops.snapshot_after_ax(accessibility)
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
        GuideTimeouts::for_user(),
        GuideMode::Explicit,
    )
}

#[cfg(test)]
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
            choice: Some(choice_timeout),
            guide: Some(guide_timeout),
        },
    )
}

pub(crate) struct GuideTimeouts {
    pub(crate) ready: Duration,
    pub(crate) choice: Option<Duration>,
    pub(crate) guide: Option<Duration>,
}

impl GuideTimeouts {
    pub(crate) fn for_user() -> Self {
        Self {
            ready: HOST_READY_TIMEOUT,
            choice: None,
            guide: None,
        }
    }
}

#[cfg(test)]
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
            if let Err(error) = ops.launch() {
                ops.observe(GuideTraceEvent::MachineInterrupted);
                return Err(error);
            }
            None
        }
        GuideMode::AfterUpdate { check_timeout } => Some(Instant::now() + check_timeout),
    };
    let read = |ops: &mut O, verify_target: &mut F| -> Result<Option<PermissionSnapshot>, String> {
        if let Some(deadline) = check_deadline {
            return update_permissions(ops, verify_target, deadline);
        }
        let ax = match wait_for_decision(ops) {
            Ok(status) => status,
            Err(error) => {
                ops.observe(GuideTraceEvent::PreflightUnknown);
                return Err(error);
            }
        };
        let snapshot = PermissionSnapshot::read(ops, ax);
        verify_target()?;
        Ok(Some(snapshot))
    };
    let Some(mut permissions) = read(ops, verify_target)? else {
        ops.observe(GuideTraceEvent::PreflightUnknown);
        return Ok(Outcome::AwaitingEvidence);
    };
    if permissions.granted() {
        ops.observe(GuideTraceEvent::PreflightGranted);
        return Ok(Outcome::Granted);
    }

    // 主进程存在不等于窗口可呈现。窗口就绪后再次读取，防止短暂未知或
    // 用户刚授予的权限导致闪卡。更新检查始终沿用同一个检查截止时间。
    if let Err(error) = ops.wait_for_window() {
        ops.observe(GuideTraceEvent::MachineInterrupted);
        return Err(error);
    }
    let Some(fresh) = read(ops, verify_target)? else {
        ops.observe(GuideTraceEvent::PreflightUnknown);
        return Ok(Outcome::AwaitingEvidence);
    };
    permissions = fresh;
    if permissions.granted() {
        ops.observe(GuideTraceEvent::PreflightGranted);
        return Ok(Outcome::Granted);
    }
    if check_deadline.is_some() {
        verify_target()?;
    }

    let mut host = match factory.start_with_permissions(root, app, &permissions) {
        Ok(host) => host,
        Err(error) => {
            ops.observe(GuideTraceEvent::MachineInterrupted);
            return Err(format!(
                "native Accessibility guide could not start: {error}"
            ));
        }
    };
    let ready_deadline = Instant::now() + limits.ready;
    let mut ready = false;
    while Instant::now() < ready_deadline {
        let event = match host.poll(Duration::from_millis(250)) {
            Ok(event) => event,
            Err(error) => {
                ops.observe(GuideTraceEvent::MachineInterrupted);
                return Err(error);
            }
        };
        // A ready event is useful only if the native window became ready
        // within its own bounded presentation deadline.
        if Instant::now() >= ready_deadline && matches!(&event, HostEvent::Ready) {
            break;
        }
        match event {
            HostEvent::AppLocale(_) => {
                ops.observe(GuideTraceEvent::MachineInterrupted);
                host.close();
                return Err(
                    "native Accessibility guide repeated its locale after configuration".into(),
                );
            }
            HostEvent::Ready => {
                ops.observe(GuideTraceEvent::CardReady);
                ready = true;
                break;
            }
            HostEvent::Timeout => {
                let permissions = PermissionSnapshot::probe(ops);
                verify_target()?;
                if Instant::now() >= ready_deadline {
                    break;
                }
                if permissions.granted() {
                    ops.observe(GuideTraceEvent::PreflightGranted);
                    let _ = observe_host_result(ops, host.send_state(HostState::Granted));
                    host.close();
                    return Ok(Outcome::Granted);
                }
            }
            HostEvent::Later => {
                ops.observe(GuideTraceEvent::UserSkip);
                host.close();
                return Ok(Outcome::Pending);
            }
            HostEvent::Close => {
                ops.observe(GuideTraceEvent::UserClose);
                host.close();
                return Ok(Outcome::Pending);
            }
            HostEvent::Eof => {
                ops.observe(GuideTraceEvent::HostEof);
                host.close();
                return Ok(Outcome::AwaitingEvidence);
            }
            HostEvent::Error(message) => {
                ops.observe(GuideTraceEvent::MachineInterrupted);
                host.close();
                return Err(format!(
                    "native Accessibility guide failed before becoming ready: {message}"
                ));
            }
            HostEvent::Allow
            | HostEvent::Retry
            | HostEvent::AllowScreen
            | HostEvent::RetryScreen => {
                ops.observe(GuideTraceEvent::MachineInterrupted);
                let _ = host.send_state(HostState::Error(
                    "native Accessibility guide sent a choice before ready".into(),
                ));
                host.close();
                return Err("native Accessibility guide sent a choice before ready".into());
            }
        }
    }
    if !ready {
        ops.observe(GuideTraceEvent::MachineInterrupted);
        let _ = host.send_state(HostState::Error(
            "native Accessibility guide did not become ready in time".into(),
        ));
        host.close();
        return Ok(Outcome::AwaitingEvidence);
    }
    // ---- 人操作不计时；机器等待与有界夹具单独计时 ----
    // 正式流程由关闭、跳过或传输结束收尾。有限预算只供隔离测试。
    // Retry 不续期有限夹具，避免测试掩盖晚到读数。
    let choice_deadline = limits.choice.map(|timeout| Instant::now() + timeout);
    let mut guide_deadline = None;
    let mut guide_started = false;
    let mut reset_performed = false;
    let mut ax_granted_after_reset = false;
    let mut screen_requested = false;
    if let Err(error) = host.send_permissions(&permissions) {
        ops.observe(GuideTraceEvent::MachineInterrupted);
        return Err(error);
    }
    loop {
        let deadline = if guide_started {
            guide_deadline
        } else {
            choice_deadline
        };
        let now = Instant::now();
        if deadline.is_some_and(|deadline| now >= deadline) {
            break;
        }
        let poll_timeout = deadline.map_or(Duration::from_millis(250), |deadline| {
            (deadline - now).min(Duration::from_millis(250))
        });
        let event = match host.poll(poll_timeout) {
            Ok(event) => event,
            Err(error) => {
                ops.observe(GuideTraceEvent::MachineInterrupted);
                return Err(error);
            }
        };
        // A misbehaving host may return an event after the bounded poll
        // interval.  Do not accept a late Allow/Retry from the pre-choice
        // phase (or silently extend the post-Allow phase).
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            break;
        }
        match event {
            HostEvent::AppLocale(_) => {
                ops.observe(GuideTraceEvent::MachineInterrupted);
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
                observe_host_result(
                    ops,
                    host.send_permission_state("accessibility", HostState::Repairing),
                )?;
                let mutation = ops.lock_target()?;
                verify_target()?;
                // A user may have approved the system row while the guide was
                // animating.  Only an explicit denied probe authorizes reset;
                // unknown/not-running is an identity/readiness failure.
                // Keep this probe and reset adjacent: no UI handoff or slow
                // signature inventory belongs between the two operations.
                match ops.probe() {
                    AccessibilityStatus::Granted => {
                        permissions = PermissionSnapshot::read(ops, AccessibilityStatus::Granted);
                        verify_target()?;
                        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                            break;
                        }
                        observe_host_result(ops, host.send_permissions(&permissions))?;
                        if permissions.granted() {
                            ops.observe(GuideTraceEvent::CardCompleted);
                            observe_host_result(ops, host.send_state(HostState::Granted))?;
                            host.close();
                            return Ok(Outcome::Granted);
                        }
                        observe_host_result(
                            ops,
                            host.send_permission_state("accessibility", HostState::Granted),
                        )?;
                        continue;
                    }
                    AccessibilityStatus::Denied => {}
                    AccessibilityStatus::Unknown | AccessibilityStatus::NotRunning => {
                        ops.observe(GuideTraceEvent::MachineInterrupted);
                        let _ = observe_host_result(
                            ops,
                            host.send_state(HostState::Error(
                                "Accessibility status was not a confirmed denial; no reset was performed".into(),
                            )),
                        );
                        host.close();
                        return Ok(Outcome::AwaitingEvidence);
                    }
                }
                if let Err(error) = ops.reset() {
                    ops.observe(GuideTraceEvent::MachineInterrupted);
                    drop(mutation);
                    show_error_until_dismissed(&mut *host, error.clone(), limits.choice);
                    return Err(error);
                }
                reset_performed = true;
                drop(mutation);
                if let Err(error) = ops.open_settings() {
                    ops.observe(GuideTraceEvent::MachineInterrupted);
                    show_error_until_dismissed(&mut *host, error.clone(), limits.choice);
                    return Err(error);
                }
                // 正式流程等待用户完成或关闭，不给系统设置和密码倒计时。
                // 有限夹具在打开设置后才起算，不把验证和 reset 计入预算。
                if !guide_started {
                    guide_started = true;
                    guide_deadline = limits.guide.map(|timeout| Instant::now() + timeout);
                }
                observe_host_result(
                    ops,
                    host.send_permission_state("accessibility", HostState::AwaitingUser),
                )?;
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
                observe_host_result(
                    ops,
                    host.send_permission_state("accessibility", HostState::Repairing),
                )?;
                verify_target()?;
                if let Err(error) = ops.open_settings() {
                    ops.observe(GuideTraceEvent::MachineInterrupted);
                    show_error_until_dismissed(&mut *host, error.clone(), limits.choice);
                    return Err(error);
                }
                observe_host_result(
                    ops,
                    host.send_permission_state("accessibility", HostState::AwaitingUser),
                )?;
            }
            HostEvent::Allow => {
                ops.observe(GuideTraceEvent::MachineInterrupted);
                let _ = host.send_state(HostState::Error(
                    "native Accessibility guide sent an invalid Allow sequence".into(),
                ));
                host.close();
                return Err("native Accessibility guide sent an invalid Allow sequence".into());
            }
            HostEvent::Retry => {
                ops.observe(GuideTraceEvent::MachineInterrupted);
                let _ = host.send_state(HostState::Error(
                    "native Accessibility guide sent an invalid Retry sequence".into(),
                ));
                host.close();
                return Err("native Accessibility guide sent an invalid Retry sequence".into());
            }
            HostEvent::AllowScreen | HostEvent::RetryScreen => {
                if permissions.screen.is_none()
                    || (matches!(event, HostEvent::AllowScreen) && screen_requested)
                    || (matches!(event, HostEvent::RetryScreen) && !screen_requested)
                {
                    ops.observe(GuideTraceEvent::MachineInterrupted);
                    let _ = host.send_state(HostState::Error(
                        "invalid Screen Recording action sequence".into(),
                    ));
                    host.close();
                    return Err("invalid Screen Recording action sequence".into());
                }
                observe_host_result(
                    ops,
                    host.send_permission_state("screen", HostState::Repairing),
                )?;
                verify_target()?;
                if ops.probe_screen() != Some(ScreenPermissionStatus::Granted) {
                    // 未获准或未知只授权设置导航，不授权清除系统决定。
                    if let Err(error) = ops
                        .record_screen_intent()
                        .and_then(|_| ops.open_screen_settings())
                    {
                        ops.observe(GuideTraceEvent::MachineInterrupted);
                        show_error_until_dismissed(&mut *host, error.clone(), limits.choice);
                        return Err(error);
                    }
                    screen_requested = true;
                    if !guide_started {
                        guide_started = true;
                        guide_deadline = limits.guide.map(|timeout| Instant::now() + timeout);
                    }
                    observe_host_result(
                        ops,
                        host.send_permission_state("screen", HostState::AwaitingUser),
                    )?;
                }
            }
            HostEvent::Later => {
                ops.observe(GuideTraceEvent::UserSkip);
                host.close();
                return Ok(Outcome::Pending);
            }
            HostEvent::Close => {
                ops.observe(GuideTraceEvent::UserClose);
                host.close();
                return Ok(Outcome::Pending);
            }
            HostEvent::Eof => {
                ops.observe(GuideTraceEvent::HostEof);
                host.close();
                return Ok(Outcome::AwaitingEvidence);
            }
            HostEvent::Error(message) => {
                ops.observe(GuideTraceEvent::MachineInterrupted);
                host.close();
                return Err(format!(
                    "native Accessibility guide reported an error: {message}"
                ));
            }
        }

        permissions = PermissionSnapshot::probe(ops);
        verify_target()?;
        // 状态读取也占用本轮时限；晚到的读数不续期。
        if (if guide_started {
            guide_deadline
        } else {
            choice_deadline
        })
        .is_some_and(|deadline| Instant::now() >= deadline)
        {
            break;
        }
        if reset_performed && permissions.accessibility == AccessibilityStatus::Granted {
            ax_granted_after_reset = true;
        }
        if permissions.screen == Some(ScreenPermissionStatus::Granted) {
            // 原生行在获准后清除 Retry；撤销后允许新的设置导航。
            screen_requested = false;
        }
        observe_host_result(ops, host.send_permissions(&permissions))?;
        if permissions.granted() {
            ops.observe(GuideTraceEvent::CardCompleted);
            observe_host_result(ops, host.send_state(HostState::Granted))?;
            host.close();
            return Ok(Outcome::Granted);
        }
        ops.wait(if guide_started {
            POST_ALLOW_PROBE_INTERVAL
        } else {
            INITIAL_PROBE_INTERVAL
        });
    }

    let message = if guide_started {
        "Permission verification timed out. Any unverified permission remains pending; an unavailable Screen Recording probe is not a denial.".into()
    } else {
        "native Accessibility guide did not receive a user choice in time".into()
    };
    match show_error_until_dismissed(&mut *host, message, limits.choice) {
        ErrorPageExit::UserSkip => {
            ops.observe(GuideTraceEvent::UserSkip);
            Ok(Outcome::Pending)
        }
        ErrorPageExit::UserClose => {
            ops.observe(GuideTraceEvent::UserClose);
            Ok(Outcome::Pending)
        }
        ErrorPageExit::HostEof => {
            ops.observe(GuideTraceEvent::HostEof);
            Ok(Outcome::AwaitingEvidence)
        }
        ErrorPageExit::MachineInterrupted => {
            ops.observe(GuideTraceEvent::MachineInterrupted);
            Ok(Outcome::AwaitingEvidence)
        }
    }
}

fn observe_host_result<O: GuideOps>(ops: &mut O, result: Result<(), String>) -> Result<(), String> {
    result.map_err(|error| {
        ops.observe(GuideTraceEvent::MachineInterrupted);
        error
    })
}

/// Keep the native error page alive long enough for a user to read it or
/// dismiss it. Production waits for the user's close; finite test limits
/// still clean up. No further choice authorizes reset in this error state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorPageExit {
    UserSkip,
    UserClose,
    HostEof,
    MachineInterrupted,
}

fn show_error_until_dismissed(
    host: &mut dyn GuideHost,
    message: String,
    timeout: Option<Duration>,
) -> ErrorPageExit {
    if host.send_state(HostState::Error(message)).is_err() {
        host.close();
        return ErrorPageExit::MachineInterrupted;
    }

    let deadline = timeout.map(|timeout| Instant::now() + timeout);
    let exit = loop {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            break ErrorPageExit::MachineInterrupted;
        }
        let remaining = deadline.map_or(Duration::from_millis(250), |deadline| {
            deadline.saturating_duration_since(Instant::now())
        });
        match host.poll(remaining.min(Duration::from_millis(250))) {
            Ok(HostEvent::Later) => break ErrorPageExit::UserSkip,
            Ok(HostEvent::Close) => break ErrorPageExit::UserClose,
            Ok(HostEvent::Eof) => break ErrorPageExit::HostEof,
            Ok(HostEvent::Error(_)) | Err(_) => break ErrorPageExit::MachineInterrupted,
            // Error is a terminal presenter state. Ignore any stale or
            // malformed choice rather than starting another operation.
            Ok(_) => {}
        }
    };
    host.close();
    exit
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
