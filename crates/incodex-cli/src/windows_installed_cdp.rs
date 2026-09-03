//! 安装态 Store 进程的最小 CDP 适配器。
//!
//! Package Debugger 恢复官方进程前，调用方已经把随机 localhost 端口写入挂起
//! 进程的命令行。本模块只接受属于该 Store package 的 listener/connection，先
//! 复用共享注入器挂载正常窗口，再用一个受限 binding 把按钮动作交给 `incodex open`。

use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::Error as WebSocketError;
use tungstenite::Message;

use crate::cdp::{
    connect_cdp_websocket, inject_shared_ui_with_options_while_alive_and_guard_with_readiness,
    is_primary_codex_page, list_targets, pick_codex_page_target, send_guarded_cdp,
    ui_ready_expression_for_options, validate_ui_probe_result_for_options, CdpWindowKind,
    CodexModeReadiness, InjectionOptions,
};
use crate::open_presentation::OPENED_MESSAGE;
use crate::windows_process::{
    ipv4_connection_server_owner, ipv4_listener_owner, running_package_process_ids,
};

const BINDING_NAME: &str = "__incodexNativeAction";
const BRIDGE_READY_TIMEOUT: Duration = Duration::from_secs(45);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(100);
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct InstalledBridgeRequest {
    pub request_id: String,
    pub execution_context_id: u64,
}

/// 仅提供安装态正常窗口所需的 native action，UI 本身始终来自共享 Runtime。
pub(crate) fn installed_bridge_source() -> String {
    format!(
        r#"(() => {{
  if (window !== window.top || window.location.href !== "app://-/index.html") return;
  const pending = new Map();
  window.__incodexResolveNativeAction = (response) => {{
    const resolve = pending.get(response?.requestId);
    if (!resolve) return;
    pending.delete(response.requestId);
    resolve(response);
  }};
  window.incodex = window.incodex || {{}};
  window.incodex.requestIncognitoAction = (payload) => {{
    if (payload?.action !== "open" || typeof payload?.requestId !== "string") {{
      return Promise.resolve({{ ok: false, code: "UNKNOWN_ACTION" }});
    }}
    return new Promise((resolve) => {{
      pending.set(payload.requestId, resolve);
      window.{BINDING_NAME}(JSON.stringify(payload));
    }});
  }};
}})();"#
    )
}

pub(crate) fn parse_installed_bridge_request(payload: &str) -> Result<String, String> {
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
    Ok(request_id.to_string())
}

pub(crate) fn installed_bridge_request_from_event(
    message: &Value,
) -> Option<InstalledBridgeRequest> {
    if message.get("method").and_then(Value::as_str) != Some("Runtime.bindingCalled")
        || message.pointer("/params/name").and_then(Value::as_str) != Some(BINDING_NAME)
    {
        return None;
    }
    let request_id = message
        .pointer("/params/payload")
        .and_then(Value::as_str)
        .and_then(|payload| parse_installed_bridge_request(payload).ok())?;
    let execution_context_id = message
        .pointer("/params/executionContextId")
        .and_then(Value::as_u64)?;
    Some(InstalledBridgeRequest {
        request_id,
        execution_context_id,
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
) -> Result<(), String> {
    let options = InjectionOptions {
        window_kind: CdpWindowKind::Normal,
        ..InjectionOptions::default()
    };
    let alive = AtomicBool::new(true);
    let mut readiness = CodexModeReadiness::default();
    let deadline = Instant::now() + BRIDGE_READY_TIMEOUT;
    let mut last = "installed Codex CDP page not ready".to_string();
    while Instant::now() < deadline && package_process_is_alive(package_full_name, main_process_id)?
    {
        if !listener_belongs_to_package(debug_port, package_full_name)? {
            thread::sleep(PROCESS_POLL_INTERVAL);
            continue;
        }
        match inject_shared_ui_with_options_while_alive_and_guard_with_readiness(
            debug_port,
            &options,
            &alive,
            |_| {},
            &mut readiness,
            &|stream| require_package_connection_owner(stream, package_full_name),
        ) {
            Ok(_) => {
                return run_bridge_until_exit(
                    debug_port,
                    package_full_name,
                    main_process_id,
                    &options,
                    &alive,
                    &mut readiness,
                );
            }
            Err(error) => last = error,
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
    Err(format!("installed Windows UI injection failed: {last}"))
}

fn run_bridge_until_exit(
    debug_port: u16,
    package_full_name: &str,
    main_process_id: u32,
    options: &InjectionOptions,
    alive: &AtomicBool,
    readiness: &mut CodexModeReadiness,
) -> Result<(), String> {
    let mut reinject = false;
    while package_process_is_alive(package_full_name, main_process_id)? {
        if reinject {
            let guard =
                |stream: &TcpStream| require_package_connection_owner(stream, package_full_name);
            match inject_shared_ui_with_options_while_alive_and_guard_with_readiness(
                debug_port,
                options,
                alive,
                |_| {},
                readiness,
                &guard,
            ) {
                Ok(_) => {}
                Err(error) if is_transient_websocket_error(&error) => {
                    thread::sleep(PROCESS_POLL_INTERVAL);
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        match run_bridge_session(debug_port, package_full_name, options) {
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
    package_full_name: &str,
    options: &InjectionOptions,
) -> Result<(), String> {
    if !listener_belongs_to_package(debug_port, package_full_name)? {
        return Err("installed CDP listener is not owned by the official package".to_string());
    }
    let targets = list_targets(debug_port)?;
    let page = pick_codex_page_target(&targets).ok_or("no installed Codex page target")?;
    if !is_primary_codex_page(page) {
        return Err("installed CDP selected a non-primary page".to_string());
    }
    let mut socket = connect_cdp_websocket(&page.ws, debug_port)?;
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
        match socket.read() {
            Ok(Message::Text(text)) => {
                let message: Value = serde_json::from_str(&text)
                    .map_err(|_| "installed CDP bridge received malformed JSON".to_string())?;
                if installed_page_requires_reinjection(&message) {
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
                if context
                    .pointer("/result/result/value")
                    .and_then(Value::as_bool)
                    != Some(true)
                {
                    continue;
                }
                let result = launch_native_open();
                command_id += 1;
                let response = match result {
                    Ok(()) => json!({
                        "requestId": request.request_id,
                        "ok": true,
                        "code": "OK"
                    }),
                    Err(reason) => json!({
                        "requestId": request.request_id,
                        "ok": false,
                        "code": "FAILED",
                        "reason": reason
                    }),
                };
                let expression = format!("window.__incodexResolveNativeAction?.({})", response);
                send_guarded_cdp(
                    &mut socket,
                    command_id,
                    "Runtime.evaluate",
                    json!({
                        "expression": expression,
                        "contextId": request.execution_context_id,
                        "returnByValue": true
                    }),
                    &guard,
                )?;
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

fn launch_native_open() -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("cannot locate the installed Incodex helper: {error}"))?;
    let mut child = Command::new(executable)
        .arg("open")
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot start native Incodex open: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "native Incodex open has no readiness channel".to_string())?;
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = sender.send(false);
                    break;
                }
                Ok(_) if line.contains(OPENED_MESSAGE) => {
                    let _ = sender.send(true);
                    while reader.read_line(&mut line).unwrap_or(0) != 0 {
                        line.clear();
                    }
                    break;
                }
                Ok(_) => {}
                Err(_) => {
                    let _ = sender.send(false);
                    break;
                }
            }
        }
    });
    match receiver.recv_timeout(BRIDGE_READY_TIMEOUT) {
        Ok(true) => {
            thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(())
        }
        Ok(false) => {
            let _ = child.kill();
            let _ = child.wait();
            Err("native Incodex open exited before the incognito window was ready".to_string())
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err("native Incodex open timed out".to_string())
        }
    }
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
}

#[cfg(test)]
mod tests {
    use super::{
        installed_bridge_request_from_event, installed_bridge_source,
        installed_page_requires_reinjection, is_transient_websocket_error,
        parse_installed_bridge_request, InstalledBridgeRequest,
    };
    use serde_json::json;

    #[test]
    fn bridge_source_only_accepts_open_actions() {
        let source = installed_bridge_source();
        assert!(source.contains("__incodexNativeAction"));
        assert!(source.contains("payload?.action !== \"open\""));
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
            InstalledBridgeRequest {
                request_id: "incodex-12345678".to_string(),
                execution_context_id: 17,
            }
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
