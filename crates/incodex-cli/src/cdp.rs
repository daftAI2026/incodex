//! Localhost Chrome DevTools Protocol client for `incodex open`.
//!
//! Launch flags and the page-target filter follow the packaged-Codex CDP
//! pattern used by other desktop launchers (debug port + allow-origins,
//! prefer `app://-/index.html`, skip chrome:// and prewarm routes). The
//! payload is our MIT `inject.js`, not a third-party injector.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use tungstenite::{connect, Message};

const INJECT_JS: &str = include_str!("../../../dist/incodex-inject.js");
const INJECT_PREFIX: &str = "window.__incodexIncognito=true;";

#[derive(Debug, Clone)]
pub struct CdpTarget {
    pub r#type: String,
    pub url: String,
    pub ws: String,
}

pub fn allocate_debug_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|err| err.to_string())?;
    let port = listener.local_addr().map_err(|err| err.to_string())?.port();
    drop(listener);
    Ok(port)
}

pub fn debug_launch_args(user_data_dir: &str, debug_port: u16) -> Vec<String> {
    vec![
        format!("--user-data-dir={user_data_dir}"),
        format!("--remote-debugging-port={debug_port}"),
        format!("--remote-allow-origins=http://127.0.0.1:{debug_port}"),
    ]
}

pub fn inject_source() -> String {
    format!("{INJECT_PREFIX}\n{INJECT_JS}")
}

pub fn pick_codex_page_target(targets: &[CdpTarget]) -> Option<&CdpTarget> {
    targets.iter().find(|target| is_primary_codex_page(target))
}

pub fn is_primary_codex_page(target: &CdpTarget) -> bool {
    if target.r#type != "page" || target.ws.is_empty() {
        return false;
    }
    let url = target.url.as_str();
    if url.starts_with("chrome://") || url.starts_with("devtools://") {
        return false;
    }
    if url.contains("quick-chat-prewarm") {
        return false;
    }
    url.starts_with("app://-/index.html") || url.starts_with("app://-/")
}

pub fn inject_shared_ui(debug_port: u16) -> Result<(), String> {
    let source = inject_source();
    let mut last = "cdp page not ready".to_string();
    let mut refused = 0u8;
    for _ in 0..20 {
        match try_inject(debug_port, &source) {
            Ok(()) => return Ok(()),
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

fn try_inject(debug_port: u16, source: &str) -> Result<(), String> {
    let targets = list_targets(debug_port)?;
    let page = pick_codex_page_target(&targets).ok_or("no Codex page target")?;
    let (mut socket, _) = connect(&page.ws).map_err(|err| err.to_string())?;
    send_cdp(
        &mut socket,
        1,
        "Page.enable",
        json!({}),
    )?;
    send_cdp(
        &mut socket,
        2,
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": source }),
    )?;
    send_cdp(
        &mut socket,
        3,
        "Runtime.evaluate",
        json!({ "expression": source, "returnByValue": true }),
    )?;
    let _ = socket.close(None);
    Ok(())
}

fn send_cdp(
    socket: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>,
    id: u64,
    method: &str,
    params: Value,
) -> Result<(), String> {
    let body = json!({ "id": id, "method": method, "params": params });
    socket
        .send(Message::Text(body.to_string()))
        .map_err(|err| err.to_string())?;
    for _ in 0..20 {
        let msg = socket.read().map_err(|err| err.to_string())?;
        let Message::Text(text) = msg else {
            continue;
        };
        let parsed: Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
        if parsed.get("id").and_then(Value::as_u64) == Some(id) {
            if parsed.get("error").is_some() {
                return Err(format!("cdp {method} failed: {text}"));
            }
            return Ok(());
        }
    }
    Err(format!("cdp {method} timed out"))
}

fn list_targets(debug_port: u16) -> Result<Vec<CdpTarget>, String> {
    let raw = http_get_json(debug_port, "/json/list")
        .or_else(|_| http_get_json(debug_port, "/json"))?;
    let list = raw.as_array().ok_or("cdp /json is not an array")?;
    Ok(list
        .iter()
        .map(|item| CdpTarget {
            r#type: item
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            url: item
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            ws: item
                .get("webSocketDebuggerUrl")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        })
        .collect())
}

fn http_get_json(debug_port: u16, path: &str) -> Result<Value, String> {
    let mut last = "cdp http failed".to_string();
    for host in ["127.0.0.1", "[::1]"] {
        match http_get_json_host(host, debug_port, path) {
            Ok(value) => return Ok(value),
            Err(err) => last = err,
        }
    }
    Err(last)
}

fn http_get_json_host(host: &str, debug_port: u16, path: &str) -> Result<Value, String> {
    let addr = if host == "[::1]" {
        format!("[::1]:{debug_port}")
    } else {
        format!("127.0.0.1:{debug_port}")
    };
    let mut stream = TcpStream::connect(&addr).map_err(|err| err.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|err| err.to_string())?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: {host}:{debug_port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|err| err.to_string())?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).map_err(|err| err.to_string())?;
    let text = String::from_utf8_lossy(&buf);
    let body = text.split("\r\n\r\n").nth(1).ok_or("cdp http missing body")?;
    serde_json::from_str(body).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_codex_app_page_and_skips_chrome_and_prewarm() {
        let targets = vec![
            CdpTarget {
                r#type: "page".into(),
                url: "chrome://newtab".into(),
                ws: "ws://127.0.0.1:1/devtools/page/a".into(),
            },
            CdpTarget {
                r#type: "page".into(),
                url: "app://-/index.html?initialRoute=%2Fchatgpt%2Fquick-chat-prewarm".into(),
                ws: "ws://127.0.0.1:1/devtools/page/b".into(),
            },
            CdpTarget {
                r#type: "page".into(),
                url: "app://-/index.html".into(),
                ws: "ws://127.0.0.1:1/devtools/page/c".into(),
            },
        ];
        let picked = pick_codex_page_target(&targets).unwrap();
        assert_eq!(picked.url, "app://-/index.html");
        assert!(inject_source().contains("__incodexIncognito=true"));
        assert!(inject_source().contains("data-incodex-privacy-toggle"));
    }
}
