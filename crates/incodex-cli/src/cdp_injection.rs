//! [INPUT]: 依赖父级 CDP 共享类型与共享 Runtime payload、官方模式状态与既有注入时序。
//! [OUTPUT]: 共用payload注册与执行；非Windows worker持有Ready会话并在断线后重注册。
//! [POS]: CDP注入边界；保留会话拥有新文档注册，一次性调用仍关闭临时连接。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use std::collections::HashSet;

use super::*;
#[cfg(target_os = "macos")]
const BLOCKED_UI_PRESENT_EXPRESSION: &str = "window.__incodexUiProbe?.button === 'present'";
#[path = "cdp_session.rs"]
mod session;
#[cfg(not(target_os = "windows"))]
pub(crate) use session::inject_shared_ui_with_retained_session;
pub(crate) use session::InjectionSession;

pub fn inject_shared_ui(debug_port: u16) -> Result<(), String> {
    inject_shared_ui_with_options(debug_port, &InjectionOptions::default()).map(|_| ())
}

pub fn inject_shared_ui_with_options(
    debug_port: u16,
    options: &InjectionOptions,
) -> Result<String, String> {
    inject_shared_ui_with_options_and_target(debug_port, options, |_| {})
}

pub fn inject_shared_ui_with_options_and_target<F>(
    debug_port: u16,
    options: &InjectionOptions,
    on_target: F,
) -> Result<String, String>
where
    F: FnMut(&str),
{
    let process_alive = AtomicBool::new(true);
    inject_shared_ui_with_options_while_alive(debug_port, options, &process_alive, on_target)
}

pub(crate) fn inject_shared_ui_with_options_while_alive<F>(
    debug_port: u16,
    options: &InjectionOptions,
    process_alive: &AtomicBool,
    on_target: F,
) -> Result<String, String>
where
    F: FnMut(&str),
{
    let mut readiness = CodexModeReadiness::default();
    inject_shared_ui_with_options_while_alive_with_readiness(
        debug_port,
        options,
        process_alive,
        on_target,
        &mut readiness,
    )
}

pub(crate) fn inject_shared_ui_with_options_while_alive_with_readiness<F>(
    debug_port: u16,
    options: &InjectionOptions,
    process_alive: &AtomicBool,
    on_target: F,
    readiness: &mut CodexModeReadiness,
) -> Result<String, String>
where
    F: FnMut(&str),
{
    inject_shared_ui_with_options_while_alive_and_guard_with_readiness(
        debug_port,
        options,
        process_alive,
        on_target,
        readiness,
        &|_| Ok(()),
    )
}

pub(crate) fn inject_shared_ui_with_options_while_alive_and_guard_with_readiness<F, G>(
    debug_port: u16,
    options: &InjectionOptions,
    process_alive: &AtomicBool,
    on_target: F,
    readiness: &mut CodexModeReadiness,
    connection_guard: &G,
) -> Result<String, String>
where
    F: FnMut(&str),
    G: Fn(&TcpStream) -> Result<(), String>,
{
    inject_shared_ui_with_options_while_alive_and_guard_with_readiness_and_runtime(
        debug_port,
        options,
        process_alive,
        on_target,
        readiness,
        connection_guard,
        INJECT_JS,
    )
}

pub(crate) fn inject_shared_ui_with_options_while_alive_and_guard_with_readiness_and_runtime<F, G>(
    debug_port: u16,
    options: &InjectionOptions,
    process_alive: &AtomicBool,
    on_target: F,
    readiness: &mut CodexModeReadiness,
    connection_guard: &G,
    runtime_source: &str,
) -> Result<String, String>
where
    F: FnMut(&str),
    G: Fn(&TcpStream) -> Result<(), String>,
{
    let mut registered_script_targets = HashSet::new();
    inject_with_registration(
        debug_port,
        options,
        process_alive,
        on_target,
        readiness,
        connection_guard,
        runtime_source,
        &mut registered_script_targets,
    )
}

#[allow(clippy::too_many_arguments)]
fn inject_with_registration<F, G>(
    debug_port: u16,
    options: &InjectionOptions,
    process_alive: &AtomicBool,
    mut on_target: F,
    readiness: &mut CodexModeReadiness,
    connection_guard: &G,
    runtime_source: &str,
    registered_script_targets: &mut HashSet<String>,
) -> Result<String, String>
where
    F: FnMut(&str),
    G: Fn(&TcpStream) -> Result<(), String>,
{
    let source = inject_source_for_options_with_runtime(options, runtime_source);
    let health_expression = ui_ready_expression_for_options(options);
    let require_profile_mask = options.profile_mask.is_some();
    let payload = InjectionPayload {
        source: &source,
        health_expression: &health_expression,
        require_profile_mask,
        require_codex_mode: options.window_kind == CdpWindowKind::Incognito,
    };
    let mut last = "cdp page not ready".to_string();
    let mut refused = 0u8;
    for _ in 0..8 {
        ensure_injection_active(process_alive)?;
        match try_inject(
            debug_port,
            &payload,
            registered_script_targets,
            process_alive,
            &mut on_target,
            readiness,
            connection_guard,
        ) {
            Ok(target_id) => return Ok(target_id),
            Err(err) if is_codex_mode_blocked_error(&err) || is_codex_mode_waiting_error(&err) => {
                return Err(err);
            }
            Err(err) if is_terminal_codex_mode_error(&err) => return Err(err),
            Err(err) => {
                let refused_now = err.contains("Connection refused")
                    || err.contains("Connection reset")
                    || err.contains("os error 61")
                    || err.contains("os error 111");
                if refused_now {
                    refused += 1;
                    if refused >= 8 {
                        return Err(err);
                    }
                }
                last = err;
            }
        }
        thread::sleep(Duration::from_millis(200));
    }
    Err(last)
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn inject_shared_ui_once_until<G>(
    debug_port: u16,
    options: &InjectionOptions,
    process_alive: &AtomicBool,
    state: &mut InjectionAttemptState,
    connection_guard: &G,
    runtime_source: &str,
    deadline: Instant,
) -> Result<String, String>
where
    G: Fn(&TcpStream) -> Result<(), String>,
{
    let source = inject_source_for_options_with_runtime(options, runtime_source);
    let health_expression = ui_ready_expression_for_options(options);
    let payload = InjectionPayload {
        source: &source,
        health_expression: &health_expression,
        require_profile_mask: options.profile_mask.is_some(),
        require_codex_mode: options.window_kind == CdpWindowKind::Incognito,
    };
    let mut on_target = |_: &str| {};

    try_inject_until(
        debug_port,
        &payload,
        &mut state.registered_script_targets,
        process_alive,
        &mut on_target,
        &mut state.readiness,
        connection_guard,
        deadline,
    )
}

pub(super) fn try_inject<F, G>(
    debug_port: u16,
    payload: &InjectionPayload<'_>,
    registered_script_targets: &mut HashSet<String>,
    process_alive: &AtomicBool,
    on_target: &mut F,
    readiness: &mut CodexModeReadiness,
    connection_guard: &G,
) -> Result<String, String>
where
    F: FnMut(&str),
    G: Fn(&TcpStream) -> Result<(), String>,
{
    try_inject_with_deadline(
        debug_port,
        payload,
        registered_script_targets,
        process_alive,
        on_target,
        readiness,
        connection_guard,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
#[cfg(any(target_os = "windows", test))]
pub(super) fn try_inject_until<F, G>(
    debug_port: u16,
    payload: &InjectionPayload<'_>,
    registered_script_targets: &mut HashSet<String>,
    process_alive: &AtomicBool,
    on_target: &mut F,
    readiness: &mut CodexModeReadiness,
    connection_guard: &G,
    deadline: Instant,
) -> Result<String, String>
where
    F: FnMut(&str),
    G: Fn(&TcpStream) -> Result<(), String>,
{
    try_inject_with_deadline(
        debug_port,
        payload,
        registered_script_targets,
        process_alive,
        on_target,
        readiness,
        connection_guard,
        Some(deadline),
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn try_inject_with_deadline<F, G>(
    debug_port: u16,
    payload: &InjectionPayload<'_>,
    registered_script_targets: &mut HashSet<String>,
    process_alive: &AtomicBool,
    on_target: &mut F,
    readiness: &mut CodexModeReadiness,
    connection_guard: &G,
    deadline: Option<Instant>,
) -> Result<String, String>
where
    F: FnMut(&str),
    G: Fn(&TcpStream) -> Result<(), String>,
{
    let session = try_inject_session_with_deadline(
        debug_port,
        payload,
        registered_script_targets,
        process_alive,
        on_target,
        readiness,
        connection_guard,
        deadline,
        None,
    )?;
    let target_id = session.target_id.clone();
    if deadline.is_none() {
        session.close();
    }
    Ok(target_id)
}

#[allow(clippy::too_many_arguments)]
fn try_inject_session_with_deadline<F, G>(
    debug_port: u16,
    payload: &InjectionPayload<'_>,
    registered_script_targets: &mut HashSet<String>,
    process_alive: &AtomicBool,
    on_target: &mut F,
    readiness: &mut CodexModeReadiness,
    connection_guard: &G,
    deadline: Option<Instant>,
    mut prepared_ui_target: Option<&mut Option<String>>,
) -> Result<InjectionSession, String>
where
    F: FnMut(&str),
    G: Fn(&TcpStream) -> Result<(), String>,
{
    ensure_injection_active(process_alive)?;
    ensure_cdp_deadline_if_present(deadline)?;
    let targets = list_targets_with_deadline(debug_port, deadline).map_err(|error| {
        record_target_discovery_failure(payload.require_codex_mode, readiness, error)
    })?;
    ensure_injection_active(process_alive)?;
    ensure_cdp_deadline_if_present(deadline)?;
    let page = match pick_codex_page_target(&targets) {
        Some(page) => page,
        None if payload.require_codex_mode => {
            return Err(codex_mode_action_error(
                readiness.observe(CodexModePageState::NotReady),
            ));
        }
        None => return Err("no Codex page target".into()),
    };
    on_target(&page.id);
    if let Some(prepared) = prepared_ui_target.as_mut() {
        if prepared.as_deref() != Some(page.id.as_str()) {
            **prepared = None;
        }
    }
    ensure_cdp_deadline_if_present(deadline)?;
    let mut socket = connect_cdp_websocket_with_deadline(&page.ws, debug_port, deadline)
        .map_err(|error| record_injection_probe_failure_if_needed(payload, readiness, error))?;
    ensure_injection_active(process_alive)?;
    ensure_cdp_deadline_if_present(deadline)?;
    send_guarded_cdp_with_deadline(
        &mut socket,
        1,
        "Page.enable",
        json!({}),
        connection_guard,
        deadline,
    )
    .map_err(|error| record_injection_probe_failure_if_needed(payload, readiness, error))?;
    if payload.require_codex_mode {
        let mode_result = confirm_official_codex_mode(
            &mut socket,
            process_alive,
            readiness,
            connection_guard,
            deadline,
        );
        #[cfg(target_os = "macos")]
        if let Err(ref error) = mode_result {
            if is_codex_mode_blocked_error(error) {
                if let Some(prepared) = prepared_ui_target.as_mut() {
                    prepare_blocked_ui_without_registration(
                        &mut socket,
                        &page.id,
                        payload,
                        prepared,
                        process_alive,
                        connection_guard,
                        deadline,
                    );
                } else {
                    prepare_blocked_ui(
                        &mut socket,
                        &page.id,
                        payload,
                        registered_script_targets,
                        process_alive,
                        connection_guard,
                        deadline,
                    );
                }
            }
        }
        mode_result?;
    }
    evaluate_shared_payload(
        &mut socket,
        &page.id,
        payload,
        registered_script_targets,
        process_alive,
        connection_guard,
        deadline,
    )
    .map_err(|error| {
        record_post_mode_injection_failure(payload.require_codex_mode, readiness, error)
    })?;
    let health = send_guarded_cdp_with_deadline(
        &mut socket,
        6,
        "Runtime.evaluate",
        json!({ "expression": payload.health_expression, "returnByValue": true }),
        connection_guard,
        deadline,
    )
    .map_err(|error| {
        record_post_mode_injection_failure(payload.require_codex_mode, readiness, error)
    })?;
    validate_ui_probe_result_for_options(&health, payload.require_profile_mask).map_err(
        |error| record_post_mode_injection_failure(payload.require_codex_mode, readiness, error),
    )?;
    ensure_cdp_deadline_if_present(deadline)?;
    Ok(InjectionSession {
        target_id: page.id.clone(),
        socket,
    })
}

/// 普通确认与阻挡准备调用同一注册/执行实现，不复制共享Runtime。
fn evaluate_shared_payload<G>(
    socket: &mut WebSocket<TcpStream>,
    target_id: &str,
    payload: &InjectionPayload<'_>,
    registered_script_targets: &mut HashSet<String>,
    process_alive: &AtomicBool,
    connection_guard: &G,
    deadline: Option<Instant>,
) -> Result<(), String>
where
    G: Fn(&TcpStream) -> Result<(), String>,
{
    ensure_injection_active(process_alive)?;
    ensure_cdp_deadline_if_present(deadline)?;
    if !registered_script_targets.contains(target_id) {
        send_guarded_cdp_with_deadline(
            socket,
            4,
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": payload.source }),
            connection_guard,
            deadline,
        )?;
        registered_script_targets.insert(target_id.to_string());
    }
    ensure_injection_active(process_alive)?;
    ensure_cdp_deadline_if_present(deadline)?;
    evaluate_payload(socket, payload, process_alive, connection_guard, deadline)
}

fn evaluate_payload<G>(
    socket: &mut WebSocket<TcpStream>,
    payload: &InjectionPayload<'_>,
    process_alive: &AtomicBool,
    connection_guard: &G,
    deadline: Option<Instant>,
) -> Result<(), String>
where
    G: Fn(&TcpStream) -> Result<(), String>,
{
    ensure_injection_active(process_alive)?;
    ensure_cdp_deadline_if_present(deadline)?;
    send_guarded_cdp_with_deadline(
        socket,
        5,
        "Runtime.evaluate",
        json!({ "expression": payload.source, "returnByValue": true }),
        connection_guard,
        deadline,
    )?;
    ensure_injection_active(process_alive)?;
    ensure_cdp_deadline_if_present(deadline)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn prepare_blocked_ui_without_registration<G>(
    socket: &mut WebSocket<TcpStream>,
    target_id: &str,
    payload: &InjectionPayload<'_>,
    prepared_ui_target: &mut Option<String>,
    process_alive: &AtomicBool,
    connection_guard: &G,
    deadline: Option<Instant>,
) where
    G: Fn(&TcpStream) -> Result<(), String>,
{
    if prepared_ui_target.as_deref() == Some(target_id)
        && blocked_ui_is_present(socket, process_alive, connection_guard, deadline) == Some(true)
    {
        return;
    }
    // Blocked轮询会关闭本次连接；这里只修复当前文档，不注册已失效的session脚本。
    if evaluate_payload(socket, payload, process_alive, connection_guard, deadline).is_ok() {
        *prepared_ui_target = Some(target_id.to_string());
    }
}

#[cfg(target_os = "macos")]
fn prepare_blocked_ui<G>(
    socket: &mut WebSocket<TcpStream>,
    target_id: &str,
    payload: &InjectionPayload<'_>,
    registered_script_targets: &mut HashSet<String>,
    process_alive: &AtomicBool,
    connection_guard: &G,
    deadline: Option<Instant>,
) where
    G: Fn(&TcpStream) -> Result<(), String>,
{
    // 控件已由同一Runtime安装时，只读轻量标记，避免重复执行全文。
    if registered_script_targets.contains(target_id)
        && blocked_ui_is_present(socket, process_alive, connection_guard, deadline) == Some(true)
    {
        return;
    }
    // 这里不验收Ready，不消耗模式失败预算。原Blocked回执始终保留。
    let _ = evaluate_shared_payload(
        socket,
        target_id,
        payload,
        registered_script_targets,
        process_alive,
        connection_guard,
        deadline,
    );
}

#[cfg(target_os = "macos")]
fn blocked_ui_is_present<G>(
    socket: &mut WebSocket<TcpStream>,
    process_alive: &AtomicBool,
    connection_guard: &G,
    deadline: Option<Instant>,
) -> Option<bool>
where
    G: Fn(&TcpStream) -> Result<(), String>,
{
    ensure_injection_active(process_alive).ok()?;
    ensure_cdp_deadline_if_present(deadline).ok()?;
    send_guarded_cdp_with_deadline(
        socket,
        3,
        "Runtime.evaluate",
        json!({ "expression": BLOCKED_UI_PRESENT_EXPRESSION, "returnByValue": true }),
        connection_guard,
        deadline,
    )
    .ok()
    .and_then(|reply| {
        reply
            .pointer("/result/result/value")
            .and_then(Value::as_bool)
    })
}

pub(super) fn ensure_injection_active(process_alive: &AtomicBool) -> Result<(), String> {
    if process_alive.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err("CDP injection cancelled after child exit".into())
    }
}

pub(super) fn confirm_official_codex_mode<G>(
    socket: &mut WebSocket<TcpStream>,
    process_alive: &AtomicBool,
    readiness: &mut CodexModeReadiness,
    connection_guard: &G,
    deadline: Option<Instant>,
) -> Result<(), String>
where
    G: Fn(&TcpStream) -> Result<(), String>,
{
    ensure_injection_active(process_alive)?;
    ensure_cdp_deadline_if_present(deadline)?;
    let response = send_guarded_cdp_with_deadline(
        socket,
        10,
        "Runtime.evaluate",
        json!({
            "expression": mode::PROBE_EXPRESSION,
            "returnByValue": true
        }),
        connection_guard,
        deadline,
    )
    .map_err(|error| record_codex_mode_probe_failure(readiness, error))?;

    let page_state = codex_mode_page_state(&response)
        .map_err(|error| record_codex_mode_probe_failure(readiness, error))?;
    let action = readiness.observe(page_state);
    if action == CodexModeAction::SelectFallback {
        let mut next_id = 11;
        dispatch_codex_mode_fallback(socket, &mut next_id, connection_guard, deadline)
            .map_err(|error| record_codex_mode_probe_failure(readiness, error))?;
    }
    match action {
        CodexModeAction::Confirmed => Ok(()),
        CodexModeAction::BlockedByOfficialUi => Err(CODEX_MODE_BLOCKED_ERROR.into()),
        CodexModeAction::Wait | CodexModeAction::SelectFallback => {
            Err(CODEX_MODE_WAITING_ERROR.into())
        }
        CodexModeAction::Unresolved => Err(CODEX_MODE_UNAVAILABLE_ERROR.into()),
    }
}

pub(super) fn record_target_discovery_failure(
    require_codex_mode: bool,
    readiness: &mut CodexModeReadiness,
    error: String,
) -> String {
    if !require_codex_mode {
        return error;
    }
    if error.contains("Connection refused")
        || error.contains("os error 61")
        || error.contains("os error 111")
        || error.contains("os error 10061")
    {
        return codex_mode_action_error(readiness.observe(CodexModePageState::NotReady));
    }
    record_codex_mode_probe_failure(readiness, error)
}

pub(super) fn record_injection_probe_failure_if_needed(
    payload: &InjectionPayload<'_>,
    readiness: &mut CodexModeReadiness,
    error: String,
) -> String {
    if payload.require_codex_mode {
        record_codex_mode_probe_failure(readiness, error)
    } else {
        error
    }
}

pub(super) fn codex_mode_action_error(action: CodexModeAction) -> String {
    match action {
        CodexModeAction::BlockedByOfficialUi => CODEX_MODE_BLOCKED_ERROR.into(),
        CodexModeAction::Unresolved => CODEX_MODE_UNAVAILABLE_ERROR.into(),
        CodexModeAction::Confirmed | CodexModeAction::Wait | CodexModeAction::SelectFallback => {
            CODEX_MODE_WAITING_ERROR.into()
        }
    }
}

pub(super) fn record_codex_mode_probe_failure(
    readiness: &mut CodexModeReadiness,
    error: String,
) -> String {
    if readiness.observe_probe_failure() == CodexModeAction::Unresolved {
        CODEX_MODE_UNAVAILABLE_ERROR.into()
    } else {
        error
    }
}

pub(super) fn record_post_mode_injection_failure(
    require_codex_mode: bool,
    readiness: &mut CodexModeReadiness,
    error: String,
) -> String {
    if require_codex_mode && readiness.observe_probe_failure() == CodexModeAction::Unresolved {
        UI_INJECTION_UNAVAILABLE_ERROR.into()
    } else {
        error
    }
}

pub(crate) fn is_terminal_ui_injection_error(error: &str) -> bool {
    error == UI_INJECTION_UNAVAILABLE_ERROR
}

pub(crate) fn is_terminal_codex_mode_error(error: &str) -> bool {
    error == CODEX_MODE_UNAVAILABLE_ERROR || is_terminal_ui_injection_error(error)
}

pub(crate) fn is_codex_mode_blocked_error(error: &str) -> bool {
    error == CODEX_MODE_BLOCKED_ERROR
}

pub(crate) fn is_codex_mode_waiting_error(error: &str) -> bool {
    error == CODEX_MODE_WAITING_ERROR
}

pub(super) fn codex_mode_page_state(response: &Value) -> Result<CodexModePageState, String> {
    let snapshot = response
        .pointer("/result/result/value")
        .and_then(Value::as_object)
        .ok_or("malformed Codex mode probe result")?;
    let mode_available = snapshot
        .get("modeAvailable")
        .and_then(Value::as_bool)
        .ok_or("malformed Codex mode probe result")?;
    let mode_label = snapshot
        .get("modeLabel")
        .and_then(Value::as_str)
        .ok_or("malformed Codex mode probe result")?;
    let blocker_visible = snapshot
        .get("officialBlockerVisible")
        .and_then(Value::as_bool)
        .ok_or("malformed Codex mode probe result")?;

    if mode_available && mode_label == "Codex" {
        return Ok(CodexModePageState::Codex);
    }
    if blocker_visible {
        return Ok(CodexModePageState::BlockedByOfficialUi);
    }
    if !mode_available {
        return Ok(CodexModePageState::NotReady);
    }
    Ok(CodexModePageState::Other)
}

pub(super) fn dispatch_codex_mode_fallback<G>(
    socket: &mut WebSocket<TcpStream>,
    next_id: &mut u64,
    connection_guard: &G,
    deadline: Option<Instant>,
) -> Result<(), String>
where
    G: Fn(&TcpStream) -> Result<(), String>,
{
    let key = |r#type: &str| {
        json!({
            "type": r#type,
            "modifiers": 2,
            "key": "3",
            "code": "Digit3",
            "windowsVirtualKeyCode": 51
        })
    };
    for r#type in ["rawKeyDown", "keyUp"] {
        send_guarded_cdp_with_deadline(
            socket,
            *next_id,
            "Input.dispatchKeyEvent",
            key(r#type),
            connection_guard,
            deadline,
        )?;
        *next_id += 1;
    }
    Ok(())
}
