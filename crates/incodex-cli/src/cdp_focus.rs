/*
 * [INPUT]: 依赖guarded CDP Runtime.evaluate、Electron官方窗口动作与焦点回执。
 * [OUTPUT]: 提供官方窗口动作及document.hasFocus有界确认。
 * [POS]: CDP共享聚焦事务；调用方保留目标选择、进程归属与身份检查。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::net::TcpStream;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::WebSocket;

use crate::cdp::send_guarded_cdp;

const FOCUS_ACK_TIMEOUT: Duration = Duration::from_secs(1);
const FOCUS_ACK_POLL: Duration = Duration::from_millis(50);
const OFFICIAL_WINDOW_ACTION_EXPRESSION: &str = r#"(async () => {
  if (window !== window.top || location.href !== 'app://-/index.html' ||
      document.querySelectorAll('[data-incodex-privacy-toggle]').length !== 1) {
    throw new Error('Codex window action context is not ready');
  }
  const bridge = window.electronBridge;
  if (typeof bridge?.sendMessageFromView !== 'function') {
    throw new Error('official window action is unavailable');
  }
  await bridge.sendMessageFromView({type:'open-current-main-window', stealFocus:true});
  return document.hasFocus();
})()"#;
const FOCUS_PROBE_EXPRESSION: &str = r#"window === window.top &&
  location.href === 'app://-/index.html' &&
  document.querySelectorAll('[data-incodex-privacy-toggle]').length === 1 &&
  document.hasFocus()"#;

pub(crate) fn focus_official_window<G>(
    socket: &mut WebSocket<TcpStream>,
    first_request_id: u64,
    owner_guard: &G,
) -> Result<(), String>
where
    G: Fn(&TcpStream) -> Result<(), String>,
{
    let reply = send_guarded_cdp(
        socket,
        first_request_id,
        "Runtime.evaluate",
        official_window_action_params(),
        owner_guard,
    )?;
    let mut focused = focus_ack(&reply)?;
    let deadline = Instant::now() + FOCUS_ACK_TIMEOUT;
    let mut request_id = first_request_id + 1;
    while !focused {
        if Instant::now() >= deadline {
            return Err("official window did not acknowledge focus".into());
        }
        thread::sleep(FOCUS_ACK_POLL);
        let reply = send_guarded_cdp(
            socket,
            request_id,
            "Runtime.evaluate",
            json!({"expression": FOCUS_PROBE_EXPRESSION, "returnByValue": true}),
            owner_guard,
        )?;
        focused = focus_ack(&reply)?;
        request_id += 1;
    }
    Ok(())
}

fn official_window_action_params() -> Value {
    json!({
        "expression": OFFICIAL_WINDOW_ACTION_EXPRESSION,
        "awaitPromise": true,
        "returnByValue": true
    })
}

fn focus_ack(reply: &Value) -> Result<bool, String> {
    reply
        .pointer("/result/result/value")
        .and_then(Value::as_bool)
        .ok_or("official window action returned no focus evidence".into())
}

#[cfg(test)]
mod tests {
    use super::{focus_ack, focus_official_window, official_window_action_params};
    use std::io::Read;
    use std::net::{TcpListener, TcpStream};
    use tungstenite::protocol::Role;
    use tungstenite::WebSocket;

    #[test]
    fn raise_reuses_the_hosts_self_window_action_and_requires_real_focus() {
        let params = official_window_action_params();

        assert_eq!(params["awaitPromise"], true);
        assert_eq!(params["returnByValue"], true);
        let expression = params["expression"].as_str().unwrap();
        assert!(expression.contains("sendMessageFromView"));
        assert!(expression.contains("open-current-main-window"));
        assert!(expression.contains("document.hasFocus()"));
    }

    #[test]
    fn invoke_completion_without_actual_focus_is_not_an_ack() {
        let reply = |value| serde_json::json!({"result":{"result":{"value": value}}});
        assert!(focus_ack(&reply(serde_json::json!(true))).unwrap());
        assert!(!focus_ack(&reply(serde_json::json!(false))).unwrap());
        for value in [
            serde_json::Value::Null,
            serde_json::json!({}),
            serde_json::json!(1),
        ] {
            assert!(focus_ack(&reply(value)).is_err());
        }
    }

    #[test]
    fn focus_helper_preserves_the_callers_connection_owner_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let client = TcpStream::connect(address).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        let mut socket = WebSocket::from_raw_socket(client, Role::Client, None);

        let expected = "captured normal Codex owner changed";
        let error =
            focus_official_window(&mut socket, 1, &|_: &TcpStream| Err(expected.to_string()))
                .unwrap_err();
        assert_eq!(error, expected);

        drop(socket);
        let mut bytes = [0; 1];
        assert_eq!(
            peer.read(&mut bytes).unwrap(),
            0,
            "the focus helper must not send CDP after an owner error"
        );
    }
}
