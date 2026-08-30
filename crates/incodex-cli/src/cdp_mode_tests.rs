use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use serde_json::{json, Value};
use tungstenite::Message;

use super::{
    codex_mode_page_state, inject_shared_ui_with_options, ui_ready_expression, CodexModeAction,
    CodexModePageState, CodexModeReadiness, InjectionOptions,
};

fn write_json_response(stream: &mut TcpStream, value: &Value) {
    let body = value.to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).unwrap();
}

fn mode_probe_response(mode_available: bool, mode_label: &str, blocker_visible: bool) -> Value {
    json!({
        "result": {"result": {"value": {
            "modeAvailable": mode_available,
            "modeLabel": mode_label,
            "officialBlockerVisible": blocker_visible
        }}}
    })
}

#[test]
fn codex_mode_probe_treats_missing_ui_and_optional_dialogs_as_pending() {
    assert_eq!(
        codex_mode_page_state(&mode_probe_response(false, "", false)).unwrap(),
        CodexModePageState::Pending
    );
    assert_eq!(
        codex_mode_page_state(&mode_probe_response(true, "ChatGPT", true)).unwrap(),
        CodexModePageState::Pending
    );
    assert_eq!(
        codex_mode_page_state(&mode_probe_response(true, "Codex", false)).unwrap(),
        CodexModePageState::Codex
    );
}

#[test]
fn codex_readiness_waits_for_optional_official_blockers() {
    let mut readiness = CodexModeReadiness::default();

    assert_eq!(
        readiness.observe(CodexModePageState::Pending),
        CodexModeAction::Wait
    );
    assert_eq!(
        readiness.observe(CodexModePageState::Codex),
        CodexModeAction::Confirmed
    );
}

#[test]
fn codex_readiness_uses_one_bounded_fallback_for_stable_chatgpt() {
    let mut readiness = CodexModeReadiness::default();

    assert_eq!(
        readiness.observe(CodexModePageState::Other),
        CodexModeAction::Wait
    );
    assert_eq!(
        readiness.observe(CodexModePageState::Other),
        CodexModeAction::Wait
    );
    assert_eq!(
        readiness.observe(CodexModePageState::Other),
        CodexModeAction::SelectFallback
    );
    assert_eq!(
        readiness.observe(CodexModePageState::Other),
        CodexModeAction::Wait
    );
    assert_eq!(
        readiness.observe(CodexModePageState::Other),
        CodexModeAction::Unresolved
    );
}

#[test]
fn codex_readiness_keeps_unresolved_terminal_without_counter_overflow() {
    let mut readiness = CodexModeReadiness::default();
    for _ in 0..3 {
        readiness.observe(CodexModePageState::Other);
    }
    readiness.observe(CodexModePageState::Other);
    assert_eq!(
        readiness.observe(CodexModePageState::Other),
        CodexModeAction::Unresolved
    );

    for _ in 0..300 {
        assert_eq!(
            readiness.observe(CodexModePageState::Other),
            CodexModeAction::Unresolved
        );
    }
}

#[test]
fn terminal_codex_mode_failure_stops_the_cdp_retry_layer() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    let connections = Arc::new(AtomicUsize::new(0));
    let server = {
        let stop = stop.clone();
        let connections = connections.clone();
        thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("CDP test server failed: {error}"),
                };
                let mut peek = [0_u8; 2048];
                let size = stream.peek(&mut peek).unwrap();
                let request = String::from_utf8_lossy(&peek[..size]);
                if request.starts_with("GET /devtools/") {
                    connections.fetch_add(1, Ordering::AcqRel);
                    stream.set_nonblocking(false).unwrap();
                    let mut socket = tungstenite::accept(stream).unwrap();
                    while let Ok(Message::Text(text)) = socket.read() {
                        let command: Value = serde_json::from_str(&text).unwrap();
                        let id = command.get("id").and_then(Value::as_u64).unwrap();
                        let expression = command
                            .pointer("/params/expression")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let response = if expression.contains("officialBlockerVisible") {
                            json!({
                                "id": id,
                                "result": {"result": {"value": {
                                    "modeAvailable": true,
                                    "modeLabel": "ChatGPT",
                                    "officialBlockerVisible": false
                                }}}
                            })
                        } else {
                            json!({"id": id, "result": {}})
                        };
                        socket
                            .send(Message::Text(response.to_string().into()))
                            .unwrap();
                    }
                    continue;
                }

                let mut request = [0_u8; 2048];
                stream.read(&mut request).unwrap();
                write_json_response(
                    &mut stream,
                    &json!([{
                        "id": "main",
                        "type": "page",
                        "url": "app://-/index.html",
                        "webSocketDebuggerUrl": format!(
                            "ws://127.0.0.1:{port}/devtools/page/main"
                        )
                    }]),
                );
            }
        })
    };

    let error = inject_shared_ui_with_options(port, &InjectionOptions::default()).unwrap_err();
    stop.store(true, Ordering::Release);
    server.join().unwrap();

    assert!(
        error.contains("Codex mode remained unavailable"),
        "unexpected terminal error: {error}"
    );
    assert_eq!(
        connections.load(Ordering::Acquire),
        1,
        "terminal mode failure must not reconnect through the inner retry layer"
    );
}

#[test]
fn open_does_not_send_the_keyboard_fallback_when_codex_is_already_selected() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (commands_tx, commands_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut http, _) = listener.accept().unwrap();
        let target = json!([{
            "id": "main",
            "type": "page",
            "url": "app://-/index.html",
            "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/main")
        }]);
        write_json_response(&mut http, &target);

        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        loop {
            let Message::Text(text) = socket.read().unwrap() else {
                continue;
            };
            let command: Value = serde_json::from_str(&text).unwrap();
            commands_tx.send(command.clone()).unwrap();
            let id = command.get("id").and_then(Value::as_u64).unwrap();
            let expression = command
                .pointer("/params/expression")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let is_mode_probe = command.get("method").and_then(Value::as_str)
                == Some("Runtime.evaluate")
                && expression.contains("officialBlockerVisible");
            let is_health_probe = command.get("method").and_then(Value::as_str)
                == Some("Runtime.evaluate")
                && expression == ui_ready_expression();
            let response = if is_mode_probe {
                json!({
                    "id": id,
                    "result": {"result": {"value": {
                        "modeAvailable": true,
                        "modeLabel": "Codex",
                        "officialBlockerVisible": false
                    }}}
                })
            } else if is_health_probe {
                json!({
                    "id": id,
                    "result": {"result": {"value": {"button": true, "banner": true}}}
                })
            } else {
                json!({"id": id, "result": {}})
            };
            socket
                .send(Message::Text(response.to_string().into()))
                .unwrap();
            if is_health_probe {
                break;
            }
        }
    });

    inject_shared_ui_with_options(port, &InjectionOptions::default()).unwrap();
    server.join().unwrap();
    let commands: Vec<Value> = commands_rx.try_iter().collect();
    let mode_keys: Vec<&Value> = commands
        .iter()
        .filter(|command| {
            command.get("method").and_then(Value::as_str) == Some("Input.dispatchKeyEvent")
        })
        .collect();

    assert!(
        mode_keys.is_empty(),
        "an already-selected Codex page must not receive Ctrl+3"
    );
    let mode_probe = commands
        .iter()
        .position(|command| {
            command
                .pointer("/params/expression")
                .and_then(Value::as_str)
                .is_some_and(|expression| expression.contains("officialBlockerVisible"))
        })
        .unwrap();
    let first_injection = commands
        .iter()
        .position(|command| {
            command
                .pointer("/params/expression")
                .and_then(Value::as_str)
                .is_some_and(|expression| expression.contains("window.__incodexIncognito=true"))
        })
        .unwrap();
    assert!(
        mode_probe < first_injection,
        "official mode confirmation must finish before UI injection"
    );
}
