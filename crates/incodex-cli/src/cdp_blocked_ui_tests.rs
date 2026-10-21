//! [INPUT]: 使用真实 loopback HTTP/WebSocket 与 CDP 注入私有边界。
//! [OUTPUT]: 锁定官方阻塞页仍注入相同 payload、但不确认就绪的契约。
//! [POS]: cdp_mode 的跨调用回归；验证阻塞、注册去重与后续 Codex 成功。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::Message;

use super::super::{try_inject, InjectionPayload, CODEX_MODE_BLOCKED_ERROR};
use super::{PageState, Readiness};

const INJECTION_SOURCE: &str = "window.__fixtureIncodexPayload = true;";
const HEALTH_EXPRESSION: &str = "window.__fixtureIncodexHealth()";

#[derive(Clone, Copy)]
enum ModeProbe {
    Blocked,
    Codex,
}

fn write_json_response(stream: &mut TcpStream, value: &Value) {
    let body = value.to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).unwrap();
}

fn mode_probe_response(probe: ModeProbe) -> Value {
    let (mode_available, mode_label, blocker_visible) = match probe {
        ModeProbe::Blocked => (false, "", true),
        ModeProbe::Codex => (true, "Codex", false),
    };
    json!({
        "result": {"result": {"value": {
            "modeAvailable": mode_available,
            "modeLabel": mode_label,
            "officialBlockerVisible": blocker_visible
        }}}
    })
}

fn start_cdp_fixture(probes: Vec<ModeProbe>) -> (u16, thread::JoinHandle<Vec<Vec<Value>>>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let mut calls = Vec::with_capacity(probes.len());
        for probe in probes {
            let (mut http, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let read = http.read(&mut request).unwrap();
            assert!(read > 0, "CDP fixture received an empty HTTP request");
            write_json_response(
                &mut http,
                &json!([{
                    "id": "main",
                    "type": "page",
                    "url": "app://-/index.html",
                    "webSocketDebuggerUrl": format!(
                        "ws://127.0.0.1:{port}/devtools/page/main"
                    )
                }]),
            );

            let (stream, _) = listener.accept().unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let mut commands = Vec::new();
            loop {
                let message = match socket.read() {
                    Ok(message) => message,
                    Err(_) => break,
                };
                let Message::Text(text) = message else {
                    continue;
                };
                let command: Value = serde_json::from_str(&text).unwrap();
                let id = command.get("id").and_then(Value::as_u64).unwrap();
                let method = command
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let expression = command
                    .pointer("/params/expression")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let is_mode_probe =
                    method == "Runtime.evaluate" && expression.contains("officialBlockerVisible");
                let is_health_probe =
                    method == "Runtime.evaluate" && expression == HEALTH_EXPRESSION;
                let is_payload = method == "Runtime.evaluate" && expression == INJECTION_SOURCE;
                commands.push(command);

                let response = if is_mode_probe {
                    let mut value = mode_probe_response(probe);
                    value["id"] = json!(id);
                    value
                } else if is_health_probe {
                    json!({
                        "id": id,
                        "result": {"result": {"value": {"button": true, "banner": false}}}
                    })
                } else {
                    json!({"id": id, "result": {}})
                };
                socket
                    .send(Message::Text(response.to_string().into()))
                    .unwrap();

                if is_health_probe || (is_payload && matches!(probe, ModeProbe::Blocked)) {
                    break;
                }
            }
            calls.push(commands);
        }
        calls
    });
    (port, server)
}

fn method_count(commands: &[Value], method: &str) -> usize {
    commands
        .iter()
        .filter(|command| command.get("method").and_then(Value::as_str) == Some(method))
        .count()
}

fn expression_count(commands: &[Value], expression: &str) -> usize {
    commands
        .iter()
        .filter(|command| {
            command.get("method").and_then(Value::as_str) == Some("Runtime.evaluate")
                && command
                    .pointer("/params/expression")
                    .and_then(Value::as_str)
                    == Some(expression)
        })
        .count()
}

#[test]
fn blocked_ui_still_gets_payload_without_fallback_or_ready_ack() {
    let (port, server) = start_cdp_fixture(vec![
        ModeProbe::Blocked,
        ModeProbe::Blocked,
        ModeProbe::Codex,
    ]);
    let payload = InjectionPayload {
        source: INJECTION_SOURCE,
        health_expression: HEALTH_EXPRESSION,
        require_profile_mask: false,
        require_codex_mode: true,
    };
    let process_alive = AtomicBool::new(true);
    let mut registered_script_targets = HashSet::new();
    let mut readiness = Readiness::new(Instant::now(), Duration::ZERO);
    let mut outcomes = Vec::new();

    for _ in 0..2 {
        let mut on_target = |_| {};
        outcomes.push(try_inject(
            port,
            &payload,
            &mut registered_script_targets,
            &process_alive,
            &mut on_target,
            &mut readiness,
            &|_| Ok(()),
        ));
    }
    let mut on_target = |_| {};
    outcomes.push(try_inject(
        port,
        &payload,
        &mut registered_script_targets,
        &process_alive,
        &mut on_target,
        &mut readiness,
        &|_| Ok(()),
    ));

    let calls = server.join().unwrap();
    assert_eq!(
        outcomes[0].as_ref().err().map(String::as_str),
        Some(CODEX_MODE_BLOCKED_ERROR),
        "an official blocker must remain a blocked launch"
    );
    assert_eq!(
        outcomes[1].as_ref().err().map(String::as_str),
        Some(CODEX_MODE_BLOCKED_ERROR),
        "repeated blocker probes must not consume the active readiness budget"
    );
    assert_eq!(outcomes[2].as_ref().ok().map(String::as_str), Some("main"));
    assert_eq!(registered_script_targets.len(), 1);

    assert_eq!(calls.len(), 3);
    for (index, commands) in calls.iter().enumerate() {
        assert_eq!(
            expression_count(commands, INJECTION_SOURCE),
            1,
            "the same Runtime payload must execute on call {index}"
        );
        assert_eq!(
            method_count(commands, "Page.addScriptToEvaluateOnNewDocument"),
            if index == 0 { 1 } else { 0 },
            "the target must be registered once across calls"
        );
        assert_eq!(
            expression_count(commands, HEALTH_EXPRESSION),
            if index == 2 { 1 } else { 0 },
            "blocked calls must not perform ready health validation"
        );
        assert_eq!(
            method_count(commands, "Input.dispatchKeyEvent"),
            0,
            "an official blocker must not trigger mode fallback"
        );
    }
    assert!(matches!(
        readiness.observe(PageState::BlockedByOfficialUi),
        super::Action::BlockedByOfficialUi
    ));
}
