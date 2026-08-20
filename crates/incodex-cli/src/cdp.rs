//! Localhost Chrome DevTools Protocol client for `incodex open`.
//!
//! Launch flags and the page-target filter follow the packaged-Codex CDP
//! pattern used by other desktop launchers (debug port + allow-origins,
//! prefer `app://-/index.html`, skip chrome:// and prewarm routes). The
//! payload is our MIT `inject.js`, not a third-party injector.

use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use tungstenite::{connect, Message};

const INJECT_JS: &str = include_str!("../../../dist/incodex-inject.js");
const INJECT_PREFIX: &str = "window.__incodexIncognito=true;";
const MAX_HTTP_RESPONSE_BYTES: usize = 1024 * 1024;
const MACOS_WINDOW_TILE_PIXELS: i32 = 22;

#[derive(Debug, Clone)]
pub struct CdpTarget {
    pub id: String,
    pub r#type: String,
    pub url: String,
    pub ws: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn chrome_tile_bounds(source: WindowBounds) -> WindowBounds {
    WindowBounds {
        x: source.x + MACOS_WINDOW_TILE_PIXELS,
        y: source.y + MACOS_WINDOW_TILE_PIXELS,
        width: source.width,
        height: source.height,
    }
}

#[derive(Debug, Clone, Default)]
pub struct InjectionOptions {
    pub locale: Option<String>,
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
    inject_source_for_locale(None)
}

pub fn inject_source_for_locale(locale: Option<&str>) -> String {
    let locale = serde_json::to_string(locale.unwrap_or("")).unwrap_or_else(|_| "\"\"".into());
    format!("{INJECT_PREFIX}window.__incodexLocale={locale};\n{INJECT_JS}")
}

pub fn ui_ready_expression() -> &'static str {
    "Boolean(document.querySelector('[data-incodex-privacy-toggle]') && document.querySelector('[data-incodex-banner-host]'))"
}

pub fn validate_cdp_websocket_url(url: &str, expected_port: u16) -> Result<(), String> {
    let uri: tungstenite::http::Uri = url
        .parse()
        .map_err(|err| format!("invalid CDP WebSocket URL: {err}"))?;
    match uri.scheme_str() {
        Some("ws" | "wss") => {}
        _ => return Err("CDP WebSocket URL must use ws or wss".into()),
    }
    let host = uri.host().ok_or("CDP WebSocket URL has no host")?;
    let ip: IpAddr = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse()
        .map_err(|_| "CDP WebSocket host must be a loopback IP address".to_string())?;
    if !ip.is_loopback() {
        return Err("CDP WebSocket host must be loopback".into());
    }
    if uri.port_u16() != Some(expected_port) {
        return Err(format!(
            "CDP WebSocket port {:?} does not match debug port {expected_port}",
            uri.port_u16()
        ));
    }
    Ok(())
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
    if url.contains("quick-chat-prewarm") || url.contains("avatar-overlay") {
        return false;
    }
    url == "app://-/index.html"
}

pub fn inject_shared_ui(debug_port: u16) -> Result<(), String> {
    inject_shared_ui_with_options(debug_port, &InjectionOptions::default())
}

pub fn inject_shared_ui_with_options(
    debug_port: u16,
    options: &InjectionOptions,
) -> Result<(), String> {
    let source = inject_source_for_locale(options.locale.as_deref());
    let mut last = "cdp page not ready".to_string();
    let mut refused = 0u8;
    for _ in 0..8 {
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
    validate_cdp_websocket_url(&page.ws, debug_port)?;
    let (mut socket, _) = connect(&page.ws).map_err(|err| err.to_string())?;
    send_cdp(&mut socket, 1, "Page.enable", json!({}))?;
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
    let health = send_cdp(
        &mut socket,
        4,
        "Runtime.evaluate",
        json!({ "expression": ui_ready_expression(), "returnByValue": true }),
    )?;
    if health
        .pointer("/result/result/value")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err("Incodex button and banner are not mounted yet".into());
    }
    let _ = socket.close(None);
    Ok(())
}

fn send_cdp(
    socket: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value, String> {
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
            if parsed.pointer("/result/exceptionDetails").is_some() {
                return Err(format!("cdp {method} exception: {text}"));
            }
            return Ok(parsed);
        }
    }
    Err(format!("cdp {method} timed out"))
}

fn list_targets(debug_port: u16) -> Result<Vec<CdpTarget>, String> {
    let raw =
        http_get_json(debug_port, "/json/list").or_else(|_| http_get_json(debug_port, "/json"))?;
    let list = raw.as_array().ok_or("cdp /json is not an array")?;
    list.iter()
        .map(|item| {
            let ws = item
                .get("webSocketDebuggerUrl")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if !ws.is_empty() {
                validate_cdp_websocket_url(&ws, debug_port)?;
            }
            Ok(CdpTarget {
                id: item
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
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
                ws,
            })
        })
        .collect()
}

fn http_get_json(debug_port: u16, path: &str) -> Result<Value, String> {
    let mut errors = Vec::new();
    for host in ["127.0.0.1", "[::1]"] {
        match http_get_json_host(host, debug_port, path) {
            Ok(value) => return Ok(value),
            Err(err) => errors.push(format!("{host}: {err}")),
        }
    }
    Err(format!("cdp http failed: {}", errors.join("; ")))
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
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|err| err.to_string())?;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {host}:{debug_port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|err| err.to_string())?;
    let mut response = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        if let Some((body_start, content_length)) = response_frame(&response)? {
            let end = body_start + content_length;
            if response.len() >= end {
                return serde_json::from_slice(&response[body_start..end])
                    .map_err(|err| err.to_string());
            }
        }
        if response.len() >= MAX_HTTP_RESPONSE_BYTES {
            return Err("cdp http response exceeded 1 MB".into());
        }
        let read = stream.read(&mut chunk).map_err(|err| err.to_string())?;
        if read == 0 {
            return Err("cdp http closed before a complete response".into());
        }
        response.extend_from_slice(&chunk[..read]);
    }
}

fn response_frame(response: &[u8]) -> Result<Option<(usize, usize)>, String> {
    let Some(header_end) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        return Ok(None);
    };
    let headers = String::from_utf8_lossy(&response[..header_end]);
    let status = headers.lines().next().unwrap_or("");
    if !(status.starts_with("HTTP/") && status.contains(" 200 ")) {
        return Err(format!("cdp http returned {status}"));
    }
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .ok_or("cdp http response missing Content-Length")?;
    if content_length > MAX_HTTP_RESPONSE_BYTES {
        return Err("cdp http Content-Length exceeded 1 MB".into());
    }
    Ok(Some((header_end + 4, content_length)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::time::Instant;

    #[test]
    fn prefers_codex_app_page_and_skips_chrome_and_prewarm() {
        let targets = vec![
            CdpTarget {
                id: "a".into(),
                r#type: "page".into(),
                url: "chrome://newtab".into(),
                ws: "ws://127.0.0.1:1/devtools/page/a".into(),
            },
            CdpTarget {
                id: "d".into(),
                r#type: "page".into(),
                url: "app://-/index.html?initialRoute=%2Favatar-overlay".into(),
                ws: "ws://127.0.0.1:1/devtools/page/d".into(),
            },
            CdpTarget {
                id: "b".into(),
                r#type: "page".into(),
                url: "app://-/index.html?initialRoute=%2Fchatgpt%2Fquick-chat-prewarm".into(),
                ws: "ws://127.0.0.1:1/devtools/page/b".into(),
            },
            CdpTarget {
                id: "c".into(),
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

    #[test]
    fn cdp_http_finishes_at_content_length_even_when_chromium_keeps_socket_open() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = "[]";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            thread::sleep(Duration::from_millis(2_500));
        });

        let started = Instant::now();
        let value = http_get_json_host("127.0.0.1", port, "/json/list")
            .expect("a complete Content-Length body must not wait for EOF");
        assert_eq!(value, json!([]));
        assert!(started.elapsed() < Duration::from_millis(500));
        server.join().unwrap();
    }

    #[test]
    fn cdp_websocket_is_confined_to_the_allocated_loopback_port() {
        assert!(validate_cdp_websocket_url("ws://127.0.0.1:43123/devtools/page/a", 43123).is_ok());
        assert!(validate_cdp_websocket_url("ws://[::1]:43123/devtools/page/a", 43123).is_ok());
        assert!(validate_cdp_websocket_url("ws://127.0.0.1:43124/devtools/page/a", 43123).is_err());
        assert!(
            validate_cdp_websocket_url("ws://example.com:43123/devtools/page/a", 43123).is_err()
        );
    }

    #[test]
    fn injected_ui_carries_locale_and_requires_button_and_banner_health() {
        let source = inject_source_for_locale(Some("zh-CN"));
        assert!(source.contains("window.__incodexIncognito=true"));
        assert!(source.contains("window.__incodexLocale=\"zh-CN\""));
        let health = ui_ready_expression();
        assert!(health.contains("data-incodex-privacy-toggle"));
        assert!(health.contains("data-incodex-banner-host"));
    }

    #[test]
    fn open_window_uses_chromiums_macos_tile_offset_and_keeps_source_size() {
        let source = WindowBounds {
            x: 100,
            y: 80,
            width: 1280,
            height: 800,
        };
        assert_eq!(
            chrome_tile_bounds(source),
            WindowBounds {
                x: 122,
                y: 102,
                width: 1280,
                height: 800,
            }
        );
    }

    #[test]
    fn losing_the_primary_page_closes_the_whole_isolated_browser() {
        let targets = vec![CdpTarget {
            id: "overlay".into(),
            r#type: "page".into(),
            url: "app://-/index.html?initialRoute=%2Favatar-overlay".into(),
            ws: "ws://127.0.0.1:43123/devtools/page/overlay".into(),
        }];

        assert_eq!(
            lifecycle_action("main", &targets),
            CdpLifecycleAction::CloseBrowser
        );
        assert_eq!(
            browser_close_message(),
            json!({ "id": 1, "method": "Browser.close", "params": {} })
        );
    }
}
