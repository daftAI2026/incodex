//! [INPUT]: 依赖父级 CDP 共享类型与已验证目标、profile mask 与既有窗口存活策略。
//! [OUTPUT]: 原样提供既有lifecycle函数；保持父级导出和调用契约。
//! [POS]: CDP 的lifecycle职责边界；不复制 Runtime 或建立新入口。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use super::*;

pub fn start_primary_lifecycle_monitor(
    debug_port: u16,
    process_alive: Arc<AtomicBool>,
    on_close: impl FnMut() -> bool + Send + 'static,
) -> Result<(), String> {
    let targets = list_targets(debug_port)?;
    let target_id = pick_codex_page_target(&targets)
        .ok_or("no Codex page target")?
        .id
        .clone();
    start_lifecycle_monitor(debug_port, target_id, process_alive, on_close);
    Ok(())
}

pub fn start_lifecycle_monitor(
    debug_port: u16,
    primary_target_id: String,
    process_alive: Arc<AtomicBool>,
    on_close: impl FnMut() -> bool + Send + 'static,
) {
    thread::spawn(move || {
        monitor_primary_target(debug_port, &primary_target_id, &process_alive, on_close)
    });
}

pub fn start_lifecycle_signal_monitor(
    debug_port: u16,
    primary_target_id: String,
    process_alive: Arc<AtomicBool>,
    close_requested: Arc<AtomicBool>,
    cdp_failed: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        monitor_primary_target_with_failure_limit(
            debug_port,
            &primary_target_id,
            &process_alive,
            LifecyclePolicy {
                max_consecutive_errors: Some(WINDOWS_CDP_FAILURE_POLLS),
                adopt_replacement: false,
                cdp_timeout: Some(WINDOWS_LIFECYCLE_CDP_TIMEOUT),
            },
            || {
                close_requested.store(true, Ordering::Release);
                true
            },
            || cdp_failed.store(true, Ordering::Release),
        )
    })
}

#[cfg(all(not(target_os = "windows"), test))]
pub(crate) fn monitor_profile_mask_health<F>(
    debug_port: u16,
    process_alive: &AtomicBool,
    on_failure: F,
) -> Result<(), String>
where
    F: FnMut(&str),
{
    masked_lifecycle::monitor(debug_port, process_alive, on_failure)
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn start_profile_mask_signal_monitor<G>(
    debug_port: u16,
    process_alive: Arc<AtomicBool>,
    cdp_failed: Arc<AtomicBool>,
    connection_guard: G,
) -> thread::JoinHandle<()>
where
    G: Fn(&TcpStream) -> Result<(), String> + Send + 'static,
{
    thread::spawn(move || {
        let _ = monitor_profile_mask_health_with_guard(
            debug_port,
            &process_alive,
            |_| cdp_failed.store(true, Ordering::Release),
            &connection_guard,
            true,
        );
    })
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn monitor_profile_mask_health_with_guard<F, G>(
    debug_port: u16,
    process_alive: &AtomicBool,
    mut on_failure: F,
    connection_guard: &G,
    defer_missing_target: bool,
) -> Result<(), String>
where
    F: FnMut(&str),
    G: Fn(&TcpStream) -> Result<(), String>,
{
    let mut failures = ProfileMaskFailureCounters::for_platform(cfg!(target_os = "windows"));
    while process_alive.load(Ordering::Acquire) {
        thread::sleep(LIFECYCLE_POLL_INTERVAL);
        if !process_alive.load(Ordering::Acquire) {
            return Ok(());
        }
        let (error, failure_kind, failure_limit) =
            match probe_profile_mask_health(debug_port, process_alive, connection_guard) {
                Ok(true) => {
                    failures.clear();
                    continue;
                }
                Ok(false) => (
                    "Incodex profile mask could not be restored".to_string(),
                    ProfileMaskFailureKind::Unhealthy,
                    PROFILE_MASK_FAILURE_POLLS,
                ),
                Err(ProfileMaskProbeError::TargetMissing) if defer_missing_target => continue,
                Err(ProfileMaskProbeError::TargetMissing) => (
                    "no Codex page target".to_string(),
                    ProfileMaskFailureKind::Unhealthy,
                    PROFILE_MASK_FAILURE_POLLS,
                ),
                Err(ProfileMaskProbeError::ProbeFailed(error)) => (
                    error,
                    ProfileMaskFailureKind::Transport,
                    profile_mask_transport_failure_polls(cfg!(target_os = "windows")),
                ),
            };
        if failures.record(failure_kind, failure_limit) {
            on_failure(&error);
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(any(target_os = "windows", test))]
pub(super) enum ProfileMaskFailureCounters {
    Consecutive(u8),
    Independent { unhealthy: u8, transport: u8 },
}

#[cfg(any(target_os = "windows", test))]
pub(super) enum ProfileMaskFailureKind {
    Unhealthy,
    Transport,
}

#[cfg(any(target_os = "windows", test))]
impl ProfileMaskFailureCounters {
    pub(super) fn for_platform(windows: bool) -> Self {
        if windows {
            Self::Independent {
                unhealthy: 0,
                transport: 0,
            }
        } else {
            Self::Consecutive(0)
        }
    }

    fn clear(&mut self) {
        match self {
            Self::Consecutive(failures) => *failures = 0,
            Self::Independent {
                unhealthy,
                transport,
            } => {
                *unhealthy = 0;
                *transport = 0;
            }
        }
    }

    pub(super) fn record(&mut self, kind: ProfileMaskFailureKind, limit: u8) -> bool {
        let counter = match self {
            Self::Consecutive(failures) => failures,
            Self::Independent {
                unhealthy,
                transport,
            } => match kind {
                ProfileMaskFailureKind::Unhealthy => unhealthy,
                ProfileMaskFailureKind::Transport => transport,
            },
        };
        *counter = counter.saturating_add(1);
        *counter >= limit
    }
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn profile_mask_transport_failure_polls(windows: bool) -> u8 {
    if windows {
        WINDOWS_PROFILE_MASK_TRANSPORT_FAILURE_POLLS
    } else {
        PROFILE_MASK_FAILURE_POLLS
    }
}

#[cfg(any(target_os = "windows", test))]
pub(super) enum ProfileMaskProbeError {
    TargetMissing,
    ProbeFailed(String),
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn probe_profile_mask_health<G>(
    debug_port: u16,
    process_alive: &AtomicBool,
    connection_guard: &G,
) -> Result<bool, ProfileMaskProbeError>
where
    G: Fn(&TcpStream) -> Result<(), String>,
{
    probe_profile_mask_snapshot(debug_port, process_alive, connection_guard)
        .map(|(_, healthy)| healthy)
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn probe_profile_mask_snapshot<G>(
    debug_port: u16,
    process_alive: &AtomicBool,
    connection_guard: &G,
) -> Result<(String, bool), ProfileMaskProbeError>
where
    G: Fn(&TcpStream) -> Result<(), String>,
{
    ensure_injection_active(process_alive).map_err(ProfileMaskProbeError::ProbeFailed)?;
    let targets = list_targets(debug_port).map_err(ProfileMaskProbeError::ProbeFailed)?;
    let page = pick_codex_page_target(&targets).ok_or(ProfileMaskProbeError::TargetMissing)?;
    let mut socket =
        connect_cdp_websocket(&page.ws, debug_port).map_err(ProfileMaskProbeError::ProbeFailed)?;
    let response = send_guarded_cdp(
        &mut socket,
        1,
        "Runtime.evaluate",
        json!({
            "expression": profile_mask_health_expression(),
            "returnByValue": true
        }),
        connection_guard,
    )
    .map_err(ProfileMaskProbeError::ProbeFailed)?;
    let healthy = response
        .pointer("/result/result/value")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            ProfileMaskProbeError::ProbeFailed("malformed profile mask health result".to_string())
        })?;
    Ok((page.id.clone(), healthy))
}

pub(super) fn profile_mask_health_expression() -> &'static str {
    "window.__incodexRefreshProfileMaskHealth?.() === true"
}

pub(super) fn monitor_primary_target<F>(
    debug_port: u16,
    primary_target_id: &str,
    process_alive: &AtomicBool,
    on_close: F,
) where
    F: FnMut() -> bool,
{
    monitor_primary_target_with_failure_limit(
        debug_port,
        primary_target_id,
        process_alive,
        LifecyclePolicy {
            max_consecutive_errors: None,
            adopt_replacement: true,
            cdp_timeout: None,
        },
        on_close,
        || {},
    );
}

pub(super) fn monitor_primary_target_with_failure_limit<F>(
    debug_port: u16,
    primary_target_id: &str,
    process_alive: &AtomicBool,
    policy: LifecyclePolicy,
    mut on_close: F,
    mut on_cdp_failure: impl FnMut(),
) where
    F: FnMut() -> bool,
{
    let mut primary_target_id = primary_target_id.to_string();
    let mut missing_polls = 0u8;
    let mut consecutive_errors = 0u8;
    while process_alive.load(Ordering::Acquire) {
        thread::sleep(LIFECYCLE_POLL_INTERVAL);
        if !process_alive.load(Ordering::Acquire) {
            return;
        }
        let targets = match policy
            .cdp_timeout
            .map(|timeout| list_targets_with_timeout(debug_port, timeout))
            .unwrap_or_else(|| list_targets(debug_port))
        {
            Ok(targets) => {
                consecutive_errors = 0;
                targets
            }
            Err(_) => {
                if !process_alive.load(Ordering::Acquire) {
                    return;
                }
                consecutive_errors = consecutive_errors.saturating_add(1);
                if policy
                    .max_consecutive_errors
                    .is_some_and(|limit| consecutive_errors >= limit)
                {
                    on_cdp_failure();
                    return;
                }
                continue;
            }
        };
        // The supervisor may have observed the native window close while this
        // bounded CDP request was in flight. Recheck before treating target
        // absence as a lifecycle event.
        if !process_alive.load(Ordering::Acquire) {
            return;
        }
        if targets.iter().any(|target| target.id == primary_target_id) {
            missing_polls = 0;
            continue;
        }
        if let Some(replacement) = pick_codex_page_target(&targets) {
            if policy.adopt_replacement {
                primary_target_id.clone_from(&replacement.id);
                missing_polls = 0;
                continue;
            }
        }

        missing_polls = missing_polls.saturating_add(1);
        if missing_polls < PRIMARY_TARGET_MISSING_POLLS {
            continue;
        }
        if !process_alive.load(Ordering::Acquire) {
            return;
        }
        if on_close() {
            return;
        }
        missing_polls = 0;
    }
}

#[cfg(test)]
pub(super) fn browser_close_message() -> Value {
    json!({ "id": 1, "method": "Browser.close", "params": {} })
}

#[cfg(test)]
pub(super) fn close_browser(debug_port: u16) -> Result<(), String> {
    let version = http_get_json(debug_port, "/json/version")?;
    let websocket = version
        .get("webSocketDebuggerUrl")
        .and_then(Value::as_str)
        .ok_or("CDP browser target has no WebSocket URL")?;
    let mut socket = connect_cdp_websocket(websocket, debug_port)?;
    socket
        .send(Message::Text(browser_close_message().to_string().into()))
        .map_err(|err| err.to_string())
}

#[cfg(test)]
pub(crate) fn close_browser_with_retries(debug_port: u16) -> Result<(), String> {
    let mut last_error = "Browser.close was not attempted".to_string();
    for attempt in 0..BROWSER_CLOSE_ATTEMPTS {
        match close_browser(debug_port) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = error,
        }
        if attempt + 1 < BROWSER_CLOSE_ATTEMPTS {
            thread::sleep(LIFECYCLE_POLL_INTERVAL);
        }
    }
    Err(last_error)
}
