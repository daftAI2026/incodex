//! [INPUT]: 依赖CDP注入内核、共享Runtime、模式状态与文档epoch探针。
//! [OUTPUT]: 持有Ready CDP session；Windows把原socket交接给既有生命周期监督。
//! [POS]: CDP会话边界；Blocked只修当前页，断线重连才注册新文档脚本。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

#[cfg(not(target_os = "windows"))]
use std::collections::HashSet;

use super::*;

pub(crate) struct InjectionSession {
    pub(crate) target_id: String,
    pub(super) socket: WebSocket<TcpStream>,
    #[cfg(not(target_os = "windows"))]
    pub(super) health_expression: Option<String>,
    #[cfg(not(target_os = "windows"))]
    pub(super) document_epoch: Option<f64>,
}

#[cfg(not(target_os = "windows"))]
pub(crate) enum SessionEnd {
    Disconnected,
    ProbeFailed(String),
    RuntimeFailed(String),
}

impl InjectionSession {
    #[cfg(target_os = "windows")]
    pub(crate) fn into_registration(self) -> WebSocket<TcpStream> {
        self.socket
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn wait_for_disconnect(
        &mut self,
        process_alive: &AtomicBool,
    ) -> Result<SessionEnd, String> {
        let health_expression = self.health_expression.clone();
        let mut readiness = match (health_expression.is_some(), self.document_epoch) {
            (true, Some(epoch)) => Some(super::super::masked_lifecycle::DocumentReadiness::new(
                epoch,
            )),
            (false, None) => None,
            _ => return Err("incomplete masked CDP session identity".into()),
        };
        if readiness.is_some() {
            self.socket
                .get_mut()
                .set_read_timeout(Some(LIFECYCLE_POLL_INTERVAL))
                .map_err(|error| format!("CDP session timeout setup failed: {error}"))?;
        }
        let mut next_probe = Instant::now();
        while process_alive.load(Ordering::Acquire) {
            if let (Some(expression), Some(readiness)) =
                (health_expression.as_deref(), readiness.as_mut())
            {
                if Instant::now() >= next_probe {
                    ensure_injection_active(process_alive)?;
                    let response = match send_cdp(
                        &mut self.socket,
                        7,
                        "Runtime.evaluate",
                        json!({ "expression": expression, "returnByValue": true }),
                    ) {
                        Ok(response) => response,
                        Err(error) if error == TARGET_CRASHED_ERROR => {
                            return Ok(SessionEnd::RuntimeFailed(
                                "page crashed during runtime".into(),
                            ));
                        }
                        Err(_) if !process_alive.load(Ordering::Acquire) => {
                            return Ok(SessionEnd::Disconnected);
                        }
                        Err(error) => return Ok(SessionEnd::ProbeFailed(error)),
                    };
                    if !process_alive.load(Ordering::Acquire) {
                        return Ok(SessionEnd::Disconnected);
                    }
                    let (epoch, healthy) = match masked_probe_from_response(&response) {
                        Ok(probe) => probe,
                        Err(_) if !process_alive.load(Ordering::Acquire) => {
                            return Ok(SessionEnd::Disconnected);
                        }
                        Err(error) => return Ok(SessionEnd::ProbeFailed(error)),
                    };
                    if !process_alive.load(Ordering::Acquire) {
                        return Ok(SessionEnd::Disconnected);
                    }
                    if let Err(error) = readiness.observe(epoch, healthy, Instant::now()) {
                        return Ok(SessionEnd::RuntimeFailed(error));
                    }
                    next_probe = Instant::now() + LIFECYCLE_POLL_INTERVAL;
                }
            }

            match self.socket.read() {
                Ok(Message::Text(text)) => {
                    let event: Value = serde_json::from_str(&text)
                        .map_err(|error| format!("invalid CDP session event: {error}"))?;
                    if matches!(
                        event.get("method").and_then(Value::as_str),
                        Some(
                            "Inspector.targetCrashed"
                                | "Inspector.detached"
                                | "Target.detachedFromTarget"
                        )
                    ) {
                        if readiness.is_some()
                            && event.get("method").and_then(Value::as_str)
                                == Some("Inspector.targetCrashed")
                        {
                            return Ok(SessionEnd::RuntimeFailed(
                                "page crashed during runtime".into(),
                            ));
                        }
                        return Err(TARGET_CRASHED_ERROR.into());
                    }
                }
                Ok(Message::Ping(_)) => self
                    .socket
                    .flush()
                    .map_err(|error| format!("CDP session ping reply failed: {error}"))?,
                Ok(Message::Close(_)) if readiness.is_some() => {
                    return Ok(SessionEnd::ProbeFailed("CDP session closed".into()));
                }
                Ok(Message::Close(_)) => return Err("CDP session closed".into()),
                Ok(_) => {}
                Err(tungstenite::Error::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(error) if readiness.is_some() => {
                    return Ok(SessionEnd::ProbeFailed(format!(
                        "CDP session disconnected: {error}"
                    )));
                }
                Err(error) => return Err(format!("CDP session disconnected: {error}")),
            }
        }
        Ok(SessionEnd::Disconnected)
    }

    pub(super) fn close(self) {
        let mut socket = self.socket;
        let _ = socket.close(None);
    }
}

/// 非Windows worker持有成功会话；重连使用新的注册集合。
#[cfg(not(target_os = "windows"))]
pub(crate) fn inject_shared_ui_with_retained_session<F>(
    debug_port: u16,
    options: &InjectionOptions,
    process_alive: &AtomicBool,
    on_target: F,
    state: &mut InjectionAttemptState,
    prepared_ui_target: &mut Option<String>,
) -> Result<InjectionSession, String>
where
    F: FnMut(&str),
{
    let source = inject_source_for_options_with_runtime(options, INJECT_JS);
    let base_health_expression = ui_ready_expression_for_options(options);
    let require_document_epoch = options.profile_mask.is_some();
    let health_expression = if require_document_epoch {
        format!(
            "(() => {{ const refreshedMask = {}; const probe = {base_health_expression}; probe.profileMask = refreshedMask; probe.documentEpoch = performance.timeOrigin; return probe; }})()",
            profile_mask_health_expression(),
        )
    } else {
        base_health_expression
    };
    let payload = InjectionPayload {
        source: &source,
        health_expression: &health_expression,
        require_profile_mask: options.profile_mask.is_some(),
        require_codex_mode: options.window_kind == CdpWindowKind::Incognito,
        require_document_epoch,
    };
    let mut on_target = on_target;
    let mut last = "cdp page not ready".to_string();
    let mut refused = 0u8;
    for _ in 0..8 {
        ensure_injection_active(process_alive)?;
        let mut registered_script_targets = HashSet::new();
        match try_inject_session_with_deadline(
            debug_port,
            &payload,
            &mut registered_script_targets,
            process_alive,
            &mut on_target,
            &mut state.readiness,
            &|_: &TcpStream| Ok(()),
            None,
            Some(prepared_ui_target),
        ) {
            Ok(session) => return Ok(session),
            Err(error)
                if is_codex_mode_blocked_error(&error) || is_codex_mode_waiting_error(&error) =>
            {
                return Err(error);
            }
            Err(error) if is_terminal_codex_mode_error(&error) => return Err(error),
            Err(error) => {
                let refused_now = error.contains("Connection refused")
                    || error.contains("Connection reset")
                    || error.contains("os error 61")
                    || error.contains("os error 111");
                if refused_now {
                    refused += 1;
                    if refused >= 8 {
                        return Err(error);
                    }
                }
                last = error;
            }
        }
        thread::sleep(Duration::from_millis(200));
    }
    Err(last)
}

#[cfg(not(target_os = "windows"))]
fn masked_probe_from_response(response: &Value) -> Result<(f64, bool), String> {
    let epoch = super::super::document_epoch_from_probe(response)?;
    let (button, profile_mask) = super::super::ui_probe_flags(response, true)?;
    Ok((epoch, button && profile_mask == Some(true)))
}
