//! 安装态 Store 进程的最小 CDP 适配器。
//!
//! Package Debugger 恢复官方进程前，调用方已经把随机 localhost 端口写入挂起
//! 进程的命令行。本模块只接受属于该 Store package 的 listener/connection，先
//! 复用共享注入器挂载正常窗口，再用一个受限 binding 把按钮动作交给 `incodex open`。

use crate::runtime_ui_update::UiGeneration;
use crate::windows_runtime_ui::InstalledUiUpdates;
use std::collections::VecDeque;
use std::net::TcpStream;
use std::path::Path;
use std::process::Child;
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::Error as WebSocketError;
use tungstenite::Message;

use crate::cdp::{
    connect_cdp_websocket,
    inject_shared_ui_with_options_while_alive_and_guard_with_readiness_and_runtime,
    is_primary_codex_page, list_targets, pick_codex_page_target, send_guarded_cdp,
    ui_ready_expression_for_options, validate_ui_probe_result_for_options, CdpWindowKind,
    CodexModeReadiness, InjectionOptions,
};
use crate::windows_installed_native_open::{
    launch_native_open, native_open_bridge_response, take_native_open_requests_for_resolution,
    NativeOpenBridgeRequest, NativeOpenOutcome, NativeOpenState,
};
use crate::windows_process::{
    ipv4_connection_server_owner, ipv4_listener_owner, running_package_process_ids,
};

const BINDING_NAME: &str = "__incodexNativeAction";
// 更新后官方初始化可能持续数分钟；这是附加功能预算，不是官方进程寿命。
const BRIDGE_READY_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const STARTUP_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(100);

struct InstalledCdpContext<'a> {
    package_full_name: &'a str,
    main_process_id: u32,
    runtime_source: &'a str,
    native_open_executable: &'a Path,
    user_root: &'a Path,
    runtime_release: &'a str,
}

/// 仅提供安装态正常窗口所需的 native action，UI 本身始终来自共享 Runtime。
pub(crate) fn installed_bridge_source() -> String {
    let actions_controller = include_str!("../assets/incodex-windows-actions.cjs");
    format!(
        r#"(() => {{
  if (window !== window.top || window.location.href !== "app://-/index.html") return;
  const pending = window.__incodexNativeActionPending || new Map();
  window.__incodexNativeActionPending = pending;
  window.__incodexResolveNativeAction = (response) => {{
    const resolve = pending.get(response?.requestId);
    if (!resolve) return;
    pending.delete(response.requestId);
    resolve(response);
  }};
  const nativeOpen = (payload) => {{
    if (payload?.action !== "open" || typeof payload?.requestId !== "string") {{
      return Promise.resolve({{ ok: false, code: "UNKNOWN_ACTION" }});
    }}
    return new Promise((resolve) => {{
      pending.set(payload.requestId, resolve);
      const bounds = [window.screenX, window.screenY, window.outerWidth, window.outerHeight];
      const sourceBounds = bounds.every(Number.isSafeInteger) ? bounds.join(",") : undefined;
      window.{BINDING_NAME}(JSON.stringify({{ ...payload, sourceBounds }}));
    }});
  }};
  const createController = (() => {{ const module = {{exports:{{}}}}; const exports = module.exports;
    {actions_controller}
    return module.exports.createWindowsActionController;
  }})();
  const controller = window.__incodexWindowsActions || createController(nativeOpen);
  window.__incodexWindowsActions = controller;
  window.incodex = window.incodex || {{}};
  window.incodex.requestIncognitoAction = payload => controller.request(payload);
}})();"#
    )
}

pub(crate) fn parse_installed_bridge_request(
    payload: &str,
) -> Result<(String, Option<String>), String> {
    let value: Value = serde_json::from_str(payload)
        .map_err(|_| "installed CDP bridge request is not valid JSON".to_string())?;
    if value.get("action").and_then(Value::as_str) != Some("open") {
        return Err("installed CDP bridge accepts only open".to_string());
    }
    let request_id = value
        .get("requestId")
        .and_then(Value::as_str)
        .filter(|request_id| {
            (8..=96).contains(&request_id.len())
                && request_id.starts_with("incodex-")
                && request_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
        .ok_or_else(|| "installed CDP bridge request id is invalid".to_string())?;
    let source_bounds = value
        .get("sourceBounds")
        .and_then(Value::as_str)
        .filter(|bounds| incodex_core::windows_session::tiled_live_bounds(bounds).is_ok())
        .map(str::to_string);
    Ok((request_id.to_string(), source_bounds))
}

pub(crate) fn installed_bridge_request_from_event(
    message: &Value,
) -> Option<NativeOpenBridgeRequest> {
    if message.get("method").and_then(Value::as_str) != Some("Runtime.bindingCalled")
        || message.pointer("/params/name").and_then(Value::as_str) != Some(BINDING_NAME)
    {
        return None;
    }
    let (request_id, source_bounds) = message
        .pointer("/params/payload")
        .and_then(Value::as_str)
        .and_then(|payload| parse_installed_bridge_request(payload).ok())?;
    let execution_context_id = message
        .pointer("/params/executionContextId")
        .and_then(Value::as_u64)?;
    Some(NativeOpenBridgeRequest {
        request_id,
        execution_context_id,
        source_bounds,
    })
}

fn installed_page_requires_reinjection(message: &Value) -> bool {
    message.get("method").and_then(Value::as_str) == Some("Page.frameNavigated")
        && message.pointer("/params/frame/parentId").is_none()
        && message.pointer("/params/frame/url").and_then(Value::as_str)
            == Some("app://-/index.html")
}

pub(crate) fn inject_installed_shared_ui(
    debug_port: u16,
    package_full_name: &str,
    main_process_id: u32,
    runtime_source: &str,
    native_open_executable: &Path,
    user_root: &Path,
    runtime_release: &str,
) -> Result<(), String> {
    let context = InstalledCdpContext {
        package_full_name,
        main_process_id,
        runtime_source,
        native_open_executable,
        user_root,
        runtime_release,
    };
    let options = InjectionOptions {
        window_kind: CdpWindowKind::Normal,
        ..InjectionOptions::default()
    };
    let alive = AtomicBool::new(true);
    let mut injection_state = crate::cdp::InjectionAttemptState::default();
    let process_identity = crate::windows_runtime_ui::process_identity(main_process_id);
    record_installed_ui_phase(user_root, &process_identity, "waiting");
    let injection = wait_for_installed_ui(
        BRIDGE_READY_TIMEOUT,
        STARTUP_RETRY_INTERVAL,
        || package_process_is_alive(package_full_name, main_process_id),
        |deadline| {
            if !listener_belongs_to_package(debug_port, package_full_name)? {
                return Err("installed Codex CDP listener not ready".into());
            }
            crate::cdp::inject_shared_ui_once_until(
                debug_port,
                &options,
                &alive,
                &mut injection_state,
                &|stream| {
                    if !package_process_is_alive(package_full_name, main_process_id)? {
                        return Err("official process exited during injection".into());
                    }
                    require_package_connection_owner(stream, package_full_name)
                },
                runtime_source,
                deadline,
            )
        },
    );
    if let Err(error) = injection {
        record_installed_ui_phase(user_root, &process_identity, "injection-unavailable");
        return Err(error);
    }
    record_installed_ui_phase(user_root, &process_identity, "ready");
    let bridge = run_bridge_until_exit(
        debug_port,
        &context,
        &options,
        &alive,
        &mut injection_state.readiness,
    );
    record_installed_ui_phase(
        user_root,
        &process_identity,
        if bridge.is_ok() {
            "closed"
        } else {
            "bridge-unavailable"
        },
    );
    bridge
}

fn record_installed_ui_phase(root: &Path, process_identity: &Value, phase: &str) {
    let result = crate::windows_update_observer_log::installed_ui_runtime_status(
        root,
        phase,
        process_identity,
    );
    if let Err(error) = result {
        eprintln!("Windows installed UI diagnostics unavailable: {error}");
    }
}

fn wait_for_installed_ui<T>(
    budget: Duration,
    interval: Duration,
    mut process_alive: impl FnMut() -> Result<bool, String>,
    mut attempt: impl FnMut(Instant) -> Result<T, String>,
) -> Result<T, String> {
    let deadline = Instant::now() + budget;
    let mut last = "installed Codex CDP page not ready".to_string();
    while Instant::now() < deadline && process_alive()? {
        match attempt(deadline) {
            Ok(value) => return Ok(value),
            Err(error) => last = error,
        }
        thread::sleep(interval.min(deadline.saturating_duration_since(Instant::now())));
    }
    Err(format!("installed Windows UI injection failed: {last}"))
}

fn run_bridge_until_exit(
    debug_port: u16,
    context: &InstalledCdpContext<'_>,
    options: &InjectionOptions,
    alive: &AtomicBool,
    readiness: &mut CodexModeReadiness,
) -> Result<(), String> {
    let mut updates = match InstalledUiUpdates::new(
        context.user_root,
        context.runtime_release,
        context.package_full_name,
        context.runtime_source,
        context.main_process_id,
    ) {
        Ok(updates) => Some(updates),
        Err(error) => {
            eprintln!("Windows live Runtime UI unavailable: {error}");
            crate::windows_runtime_ui::report_controller_unavailable(
                context.user_root,
                context.main_process_id,
            );
            None
        }
    };
    let mut reinject = false;
    let mut native_open = NativeOpenState::<Child>::default();
    let mut pending_native_open = VecDeque::new();
    let mut pending_native_outcome = None;
    while package_process_is_alive(context.package_full_name, context.main_process_id)? {
        if reinject {
            if let Some(updates) = updates.as_mut() {
                updates.renderer_invalidated();
            }
            let guard = |stream: &TcpStream| {
                require_package_connection_owner(stream, context.package_full_name)
            };
            match inject_shared_ui_with_options_while_alive_and_guard_with_readiness_and_runtime(
                debug_port,
                options,
                alive,
                |_| {},
                readiness,
                &guard,
                updates
                    .as_ref()
                    .map(InstalledUiUpdates::source)
                    .unwrap_or(context.runtime_source),
            ) {
                Ok(_) => {}
                Err(error) if is_transient_websocket_error(&error) => {
                    thread::sleep(PROCESS_POLL_INTERVAL);
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        match run_bridge_session(
            debug_port,
            context,
            options,
            &mut native_open,
            &mut pending_native_open,
            &mut pending_native_outcome,
            &mut updates,
        ) {
            Ok(()) => reinject = true,
            Err(error) if is_transient_websocket_error(&error) => reinject = true,
            Err(error) => return Err(error),
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
    Ok(())
}

fn run_bridge_session(
    debug_port: u16,
    context: &InstalledCdpContext<'_>,
    options: &InjectionOptions,
    native_open: &mut NativeOpenState<Child>,
    pending_native_open: &mut VecDeque<NativeOpenBridgeRequest>,
    pending_native_outcome: &mut Option<NativeOpenOutcome>,
    updates: &mut Option<InstalledUiUpdates>,
) -> Result<(), String> {
    let package_full_name = context.package_full_name;
    let native_open_executable = context.native_open_executable;
    if !listener_belongs_to_package(debug_port, package_full_name)? {
        return Err("installed CDP listener is not owned by the official package".to_string());
    }
    let targets = list_targets(debug_port)?;
    let page = pick_codex_page_target(&targets).ok_or("no installed Codex page target")?;
    if !is_primary_codex_page(page) {
        return Err("installed CDP selected a non-primary page".to_string());
    }
    let mut socket = connect_cdp_websocket(&page.ws, debug_port)?;
    let cdp_read_timeout = socket
        .get_ref()
        .read_timeout()
        .map_err(|error| format!("cannot inspect installed CDP read timeout: {error}"))?;
    let guard = |stream: &TcpStream| require_package_connection_owner(stream, package_full_name);
    send_guarded_cdp(&mut socket, 100, "Page.enable", json!({}), &guard)?;
    send_guarded_cdp(&mut socket, 101, "Runtime.enable", json!({}), &guard)?;
    send_guarded_cdp(
        &mut socket,
        102,
        "Runtime.addBinding",
        json!({ "name": BINDING_NAME }),
        &guard,
    )?;
    let source = installed_bridge_source();
    send_guarded_cdp(
        &mut socket,
        103,
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": source }),
        &guard,
    )?;
    send_guarded_cdp(
        &mut socket,
        104,
        "Runtime.evaluate",
        json!({ "expression": source, "returnByValue": true }),
        &guard,
    )?;
    let health = ui_ready_expression_for_options(options);
    let health_response = send_guarded_cdp(
        &mut socket,
        105,
        "Runtime.evaluate",
        json!({ "expression": health, "returnByValue": true }),
        &guard,
    )?;
    validate_ui_probe_result_for_options(&health_response, options.profile_mask.is_some())?;

    let mut command_id = 200u64;
    loop {
        if let Some(updates) = updates.as_mut() {
            updates.refresh(
                |candidate| {
                    apply_installed_ui_generation(
                        debug_port,
                        package_full_name,
                        &page.ws,
                        candidate,
                        false,
                    )
                },
                |candidate| {
                    apply_installed_ui_generation(
                        debug_port,
                        package_full_name,
                        &page.ws,
                        candidate,
                        true,
                    )
                },
            );
        }
        if let Some(outcome) = native_open.poll(native_open_child_is_alive) {
            if !matches!(&outcome, NativeOpenOutcome::Pending) {
                *pending_native_outcome = Some(outcome);
            }
        }
        if let Some(outcome) = pending_native_outcome.as_ref() {
            resolve_pending_native_open(
                &mut socket,
                package_full_name,
                pending_native_open,
                outcome,
                &mut command_id,
            )?;
            if pending_native_open.is_empty() {
                *pending_native_outcome = None;
            }
        }
        socket
            .get_ref()
            .set_read_timeout(Some(PROCESS_POLL_INTERVAL))
            .map_err(|error| format!("cannot set installed CDP poll interval: {error}"))?;
        let read_result = socket.read();
        socket
            .get_ref()
            .set_read_timeout(cdp_read_timeout)
            .map_err(|error| format!("cannot restore installed CDP read timeout: {error}"))?;
        match read_result {
            Ok(Message::Text(text)) => {
                let message: Value = serde_json::from_str(&text)
                    .map_err(|_| "installed CDP bridge received malformed JSON".to_string())?;
                if installed_page_requires_reinjection(&message) {
                    pending_native_open.clear();
                    *pending_native_outcome = None;
                    return Ok(());
                }
                let Some(request) = installed_bridge_request_from_event(&message) else {
                    continue;
                };
                command_id += 1;
                let context = send_guarded_cdp(
                    &mut socket,
                    command_id,
                    "Runtime.evaluate",
                    json!({
                        "expression": "window === window.top && window.location.href === \"app://-/index.html\"",
                        "contextId": request.execution_context_id,
                        "returnByValue": true
                    }),
                    &guard,
                )?;
                if !is_installed_primary_context(&context) {
                    continue;
                }
                if let Some(outcome) = native_open.poll(native_open_child_is_alive) {
                    if !matches!(&outcome, NativeOpenOutcome::Pending) {
                        *pending_native_outcome = Some(outcome);
                    }
                }
                if let Some(outcome) = pending_native_outcome.as_ref() {
                    resolve_pending_native_open(
                        &mut socket,
                        package_full_name,
                        pending_native_open,
                        outcome,
                        &mut command_id,
                    )?;
                    if pending_native_open.is_empty() {
                        *pending_native_outcome = None;
                    }
                }
                let source_bounds = request.source_bounds.clone();
                pending_native_open.push_back(request);
                let outcome = native_open.request(
                    || {
                        let selected = updates
                            .as_ref()
                            .map(InstalledUiUpdates::native_open_executable)
                            .transpose()?;
                        launch_native_open(
                            selected.as_deref().unwrap_or(native_open_executable),
                            source_bounds.as_deref(),
                        )
                    },
                    native_open_child_is_alive,
                );
                if !matches!(&outcome, NativeOpenOutcome::Pending) {
                    *pending_native_outcome = Some(outcome);
                }
                if let Some(outcome) = pending_native_outcome.as_ref() {
                    resolve_pending_native_open(
                        &mut socket,
                        package_full_name,
                        pending_native_open,
                        outcome,
                        &mut command_id,
                    )?;
                    if pending_native_open.is_empty() {
                        *pending_native_outcome = None;
                    }
                }
            }
            Ok(Message::Ping(payload)) => socket
                .send(Message::Pong(payload))
                .map_err(|error| error.to_string())?,
            Ok(Message::Close(_)) => return Ok(()),
            Ok(_) => {}
            Err(WebSocketError::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => return Err(format!("installed CDP bridge disconnected: {error}")),
        }
    }
}

fn apply_installed_ui_generation(
    debug_port: u16,
    package_full_name: &str,
    page_websocket: &str,
    candidate: &UiGeneration,
    commit: bool,
) -> Result<bool, String> {
    if !listener_belongs_to_package(debug_port, package_full_name)? {
        return Err("Runtime update listener changed owner".into());
    }
    // Reuse this bridge's exact page target. A separate socket must not consume
    // its binding/pending-open events or pick another window across navigation.
    let mut socket = connect_cdp_websocket(page_websocket, debug_port)?;
    let guard = |stream: &TcpStream| require_package_connection_owner(stream, package_full_name);
    let id = candidate
        .files
        .get("incodex-inject.js")
        .ok_or("Runtime update has no injector identity")?;
    let action_id = candidate
        .files
        .get("incodex-main-actions.cjs")
        .ok_or("Runtime update has no action identity")?;
    let action_id_json = serde_json::to_string(action_id).map_err(|error| error.to_string())?;
    let request =
        serde_json::to_string(&json!({"protocol":1,"id":id})).map_err(|error| error.to_string())?;
    let expression = if commit {
        format!(
            r#"(() => {{
          if (window !== window.top || window.location.href !== "app://-/index.html") return null;
          const controller = window.__incodexWindowsActions;
          const ui = window.__incodexRendererGeneration;
          if (ui?.protocol !== 1 || ui.id !== {request}.id || ui.restartRequired !== false) return null;
          return {{ui, actions:controller?.commitStaged({action_id_json})}};
        }})()"#
        )
    } else {
        format!(
            r#"(() => {{
      if (window !== window.top || window.location.href !== "app://-/index.html") return null;
      const controller = window.__incodexWindowsActions;
      if (!controller) return null;
      const factory = (() => {{ const module = {{exports:{{}}}}; const exports = module.exports;
        {}; return module.exports.createMainActions;
      }})();
      const actions = controller.prepare(factory, {action_id_json});
      window.__incodexIncognito=false; window.__incodexPlatform='win32'; window.__incodexRendererRequest={request};
      {};
      const ui = window.__incodexRendererGeneration;
      if (ui?.protocol === 1 && ui.id === {request}.id && ui.restartRequired === false) controller.stage(actions);
      return {{ ui, actions:controller.stagedGeneration() }};
    }})()"#,
            candidate.action_source, candidate.source
        )
    };
    let response = send_guarded_cdp(
        &mut socket,
        1,
        "Runtime.evaluate",
        json!({"expression":expression,"returnByValue":true}),
        &guard,
    )?;
    let value = &response["result"]["result"]["value"];
    Ok(value["ui"]["protocol"] == 1
        && value["ui"]["id"] == *id
        && value["ui"]["restartRequired"] == false
        && value["actions"]["protocol"] == 1
        && value["actions"]["id"] == *action_id)
}

fn is_installed_primary_context(response: &Value) -> bool {
    response
        .pointer("/result/result/value")
        .and_then(Value::as_bool)
        == Some(true)
}

fn resolve_pending_native_open(
    socket: &mut tungstenite::WebSocket<TcpStream>,
    package_full_name: &str,
    pending: &mut VecDeque<NativeOpenBridgeRequest>,
    outcome: &NativeOpenOutcome,
    command_id: &mut u64,
) -> Result<(), String> {
    let guard = |stream: &TcpStream| require_package_connection_owner(stream, package_full_name);
    let mut requests = take_native_open_requests_for_resolution(pending, outcome);
    while let Some(request) = requests.pop_front() {
        *command_id += 1;
        let context = send_guarded_cdp(
            socket,
            *command_id,
            "Runtime.evaluate",
            json!({
                "expression": "window === window.top && window.location.href === \"app://-/index.html\"",
                "contextId": request.execution_context_id,
                "returnByValue": true
            }),
            &guard,
        );
        let context = match context {
            Ok(context) => context,
            Err(error) if is_stale_execution_context_error(&error) => continue,
            Err(error) => {
                pending.push_back(request);
                pending.append(&mut requests);
                return Err(error);
            }
        };
        let response =
            native_open_bridge_response(&request, outcome, is_installed_primary_context(&context));
        let Some(response) = response else {
            continue;
        };
        *command_id += 1;
        let expression = format!("window.__incodexResolveNativeAction?.({response})");
        if let Err(error) = send_guarded_cdp(
            socket,
            *command_id,
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "contextId": request.execution_context_id,
                "returnByValue": true
            }),
            &guard,
        ) {
            if is_stale_execution_context_error(&error) {
                continue;
            }
            pending.push_back(request);
            pending.append(&mut requests);
            return Err(error);
        }
    }
    Ok(())
}

fn native_open_child_is_alive(child: &mut Child) -> Result<bool, String> {
    child
        .try_wait()
        .map(|status| status.is_none())
        .map_err(|error| format!("cannot inspect native Incodex open child: {error}"))
}

fn is_stale_execution_context_error(error: &str) -> bool {
    error.contains("Cannot find context with specified id")
}

fn listener_belongs_to_package(debug_port: u16, package_full_name: &str) -> Result<bool, String> {
    let Some(owner) = ipv4_listener_owner(debug_port)
        .map_err(|error| format!("cannot inspect installed CDP listener owner: {error}"))?
    else {
        return Ok(false);
    };
    Ok(running_package_process_ids(package_full_name)
        .map_err(|error| format!("cannot inspect installed package processes: {error}"))?
        .contains(&owner))
}

fn require_package_connection_owner(
    stream: &TcpStream,
    package_full_name: &str,
) -> Result<(), String> {
    let owner = ipv4_connection_server_owner(stream)
        .map_err(|error| format!("cannot inspect installed CDP connection owner: {error}"))?
        .ok_or_else(|| "cannot identify installed CDP connection owner".to_string())?;
    if running_package_process_ids(package_full_name)
        .map_err(|error| format!("cannot inspect installed package processes: {error}"))?
        .contains(&owner)
    {
        Ok(())
    } else {
        Err("installed CDP connection owner is outside the official package".to_string())
    }
}

fn package_process_is_alive(package_full_name: &str, process_id: u32) -> Result<bool, String> {
    Ok(running_package_process_ids(package_full_name)
        .map_err(|error| format!("cannot inspect installed package processes: {error}"))?
        .contains(&process_id))
}

fn is_transient_websocket_error(error: &str) -> bool {
    error.contains("disconnected")
        || error.contains("Connection reset")
        || error.contains("timed out")
        || error.contains("no installed Codex page target")
        || error.contains("no Codex page target")
        || error.contains("Incodex button is not mounted yet")
        || error.contains("Cannot find context with specified id")
}

#[cfg(test)]
mod tests {
    use super::{wait_for_installed_ui, BRIDGE_READY_TIMEOUT};
    use std::time::{Duration, Instant};

    #[test]
    fn installed_slow_start_survives_the_old_readiness_budget() {
        // 缩小千倍的时间轴：官方第 180 秒就绪，不能在第 45 秒放弃。
        let started = Instant::now();
        let budget = BRIDGE_READY_TIMEOUT / 1000;
        let result = wait_for_installed_ui(
            budget,
            Duration::from_millis(2),
            || Ok(true),
            |_| {
                if started.elapsed() >= Duration::from_millis(180) {
                    Ok("injected")
                } else {
                    Err("official app still initializing".into())
                }
            },
        );
        assert_eq!(result.unwrap(), "injected");
    }

    #[test]
    fn installed_readiness_does_not_sleep_past_its_total_budget() {
        let started = Instant::now();
        let result: Result<(), String> = wait_for_installed_ui(
            Duration::from_millis(30),
            Duration::from_secs(1),
            || Ok(true),
            |_| Err("not ready".into()),
        );
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn installed_readiness_cancels_when_the_official_process_exits() {
        let mut checks = 0;
        let mut attempts = 0;
        let result: Result<(), String> = wait_for_installed_ui(
            Duration::from_secs(1),
            Duration::from_millis(1),
            || {
                checks += 1;
                Ok(checks == 1)
            },
            |_| {
                attempts += 1;
                Err("not ready".into())
            },
        );
        assert!(result.is_err());
        assert_eq!(attempts, 1);
    }

    use super::{
        installed_bridge_request_from_event, installed_bridge_source,
        installed_page_requires_reinjection, is_installed_primary_context,
        is_stale_execution_context_error, is_transient_websocket_error,
        parse_installed_bridge_request,
    };
    use crate::windows_installed_native_open::{
        native_open_bridge_response, take_native_open_requests_for_resolution,
        NativeOpenBridgeRequest, NativeOpenOutcome,
    };
    use serde_json::json;
    use std::collections::VecDeque;

    #[test]
    fn bridge_source_only_accepts_open_actions() {
        let source = installed_bridge_source();
        assert!(source.contains("__incodexNativeAction"));
        assert!(source.contains("payload?.action !== \"open\""));
        assert!(source.contains("window.screenX"));
        assert!(source.contains("window.outerWidth"));
        assert!(source.contains("sourceBounds"));
    }

    #[test]
    fn pending_native_open_keeps_requests_without_emitting_failure() {
        let request = NativeOpenBridgeRequest {
            request_id: "incodex-12345678".into(),
            execution_context_id: 17,
            source_bounds: Some("250,136,1399,820".into()),
        };
        let mut pending = VecDeque::from([request]);
        let unresolved =
            take_native_open_requests_for_resolution(&mut pending, &NativeOpenOutcome::Pending);
        let failure_response =
            native_open_bridge_response(&pending[0], &NativeOpenOutcome::Pending, true);

        assert!(unresolved.is_empty());
        assert_eq!(pending.len(), 1);
        assert_eq!(failure_response, None);

        let resolved =
            take_native_open_requests_for_resolution(&mut pending, &NativeOpenOutcome::Ready);
        assert!(pending.is_empty());
        assert_eq!(resolved[0].execution_context_id, 17);
        assert_eq!(
            native_open_bridge_response(&resolved[0], &NativeOpenOutcome::Ready, true),
            Some(json!({
                "requestId": "incodex-12345678",
                "ok": true,
                "code": "OK"
            }))
        );
    }

    #[test]
    fn native_open_response_is_suppressed_for_a_stale_context() {
        let request = NativeOpenBridgeRequest {
            request_id: "incodex-12345678".into(),
            execution_context_id: 19,
            source_bounds: None,
        };
        let current = json!({ "result": { "result": { "value": true } } });
        let stale = json!({ "result": { "result": { "value": false } } });

        assert!(is_installed_primary_context(&current));
        assert!(!is_installed_primary_context(&stale));
        assert_eq!(
            native_open_bridge_response(&request, &NativeOpenOutcome::Ready, false),
            None
        );
        assert!(is_stale_execution_context_error(
            "Runtime.evaluate failed: Cannot find context with specified id"
        ));
    }

    #[test]
    fn bridge_rejects_untrusted_request_ids() {
        assert!(parse_installed_bridge_request(r#"{"action":"open","requestId":"bad"}"#).is_err());
        assert!(parse_installed_bridge_request(
            r#"{"action":"close","requestId":"incodex-12345678"}"#
        )
        .is_err());
    }

    #[test]
    fn bridge_extracts_only_the_expected_binding_event() {
        let event = json!({
            "method": "Runtime.bindingCalled",
            "params": {
                "name": "__incodexNativeAction",
                "payload": "{\"action\":\"open\",\"requestId\":\"incodex-12345678\"}",
                "executionContextId": 17
            }
        });
        assert_eq!(
            installed_bridge_request_from_event(&event).expect("valid binding event"),
            NativeOpenBridgeRequest {
                request_id: "incodex-12345678".to_string(),
                execution_context_id: 17,
                source_bounds: None,
            }
        );

        let with_bounds = json!({
            "method": "Runtime.bindingCalled",
            "params": {
                "name": "__incodexNativeAction",
                "payload": "{\"action\":\"open\",\"requestId\":\"incodex-12345678\",\"sourceBounds\":\"250,136,1399,820\"}",
                "executionContextId": 17
            }
        });
        assert_eq!(
            installed_bridge_request_from_event(&with_bounds)
                .expect("valid source bounds")
                .source_bounds
                .as_deref(),
            Some("250,136,1399,820")
        );

        let mut missing_context = event;
        missing_context["params"]
            .as_object_mut()
            .expect("binding params")
            .remove("executionContextId");
        assert!(installed_bridge_request_from_event(&missing_context).is_none());
    }

    #[test]
    fn bridge_retries_after_a_replacement_target_loses_the_shared_ui() {
        assert!(is_transient_websocket_error(
            "Incodex button is not mounted yet"
        ));
        assert!(is_transient_websocket_error(
            "cdp Runtime.evaluate failed: Cannot find context with specified id"
        ));
    }

    #[test]
    fn bridge_reinjects_only_after_a_top_level_codex_navigation() {
        let primary = json!({
            "method": "Page.frameNavigated",
            "params": {
                "frame": {
                    "id": "main",
                    "url": "app://-/index.html"
                }
            }
        });
        assert!(installed_page_requires_reinjection(&primary));

        let mut child = primary.clone();
        child["params"]["frame"]["parentId"] = json!("main");
        assert!(!installed_page_requires_reinjection(&child));

        let mut foreign = primary;
        foreign["params"]["frame"]["url"] = json!("https://example.com/");
        assert!(!installed_page_requires_reinjection(&foreign));
    }
}
