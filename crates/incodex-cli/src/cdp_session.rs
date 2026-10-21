//! [INPUT]: 依赖CDP注入内核、共享Runtime与官方模式状态。
//! [OUTPUT]: 非Windows open持有Ready CDP session；重连按新session注册新文档脚本。
//! [POS]: CDP会话边界；Blocked仍只修当前页，Ready session存活至child结束。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

#[cfg(not(target_os = "windows"))]
use std::collections::HashSet;

use super::*;

pub(crate) struct InjectionSession {
    pub(crate) target_id: String,
    pub(super) socket: WebSocket<TcpStream>,
}

impl InjectionSession {
    #[cfg(not(target_os = "windows"))]
    pub(crate) fn wait_for_disconnect(
        &mut self,
        process_alive: &AtomicBool,
        stop_requested: &AtomicBool,
    ) -> Result<(), String> {
        while process_alive.load(Ordering::Acquire) && !stop_requested.load(Ordering::Acquire) {
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
                        return Err(TARGET_CRASHED_ERROR.into());
                    }
                }
                Ok(Message::Ping(_)) => self
                    .socket
                    .flush()
                    .map_err(|error| format!("CDP session ping reply failed: {error}"))?,
                Ok(Message::Close(_)) => return Err("CDP session closed".into()),
                Ok(_) => {}
                Err(tungstenite::Error::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(error) => return Err(format!("CDP session disconnected: {error}")),
            }
        }
        Ok(())
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
    let health_expression = ui_ready_expression_for_options(options);
    let payload = InjectionPayload {
        source: &source,
        health_expression: &health_expression,
        require_profile_mask: options.profile_mask.is_some(),
        require_codex_mode: options.window_kind == CdpWindowKind::Incognito,
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
