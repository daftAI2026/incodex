//! Localhost Chrome DevTools Protocol client for `incodex open`.
//!
//! Launch flags and the page-target filter follow the packaged-Codex CDP
//! pattern used by other desktop launchers (debug port + allow-origins,
//! prefer `app://-/index.html`, skip chrome:// and prewarm routes). The
//! payload is our MIT `inject.js`, not a third-party injector.

use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::{client::ClientHandshake, HandshakeError};
use tungstenite::{Message, WebSocket};

const INJECT_JS: &str = include_str!("../../../dist/incodex-inject.js");
const INJECT_PREFIX: &str = "window.__incodexIncognito=true;";
const MAX_HTTP_RESPONSE_BYTES: usize = 1024 * 1024;
const MACOS_WINDOW_TILE_PIXELS: i32 = 22;
const CDP_IO_TIMEOUT: Duration = Duration::from_secs(2);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CdpLifecycleAction {
    Wait,
    CloseBrowser,
}

pub fn allocate_debug_port() -> Result<u16, String> {
    // Chromium 必须在 listener 释放后自行 bind；这是不可避免的短暂 TOCTOU。
    // 后续 HTTP/CDP 操作均有硬截止时间，抢占或 bind 失败只会得到有界的 UI 错误。
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
    inject_shared_ui_with_options(debug_port, &InjectionOptions::default()).map(|_| ())
}

pub fn inject_shared_ui_with_options(
    debug_port: u16,
    options: &InjectionOptions,
) -> Result<String, String> {
    let source = inject_source_for_locale(options.locale.as_deref());
    let mut last = "cdp page not ready".to_string();
    let mut refused = 0u8;
    for _ in 0..8 {
        match try_inject(debug_port, &source) {
            Ok(target_id) => return Ok(target_id),
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

fn try_inject(debug_port: u16, source: &str) -> Result<String, String> {
    let targets = list_targets(debug_port)?;
    let page = pick_codex_page_target(&targets).ok_or("no Codex page target")?;
    validate_cdp_websocket_url(&page.ws, debug_port)?;
    let (mut socket, _) = connect_cdp_websocket(&page.ws, debug_port)?;
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
    let target_id = page.id.clone();
    let _ = socket.close(None);
    Ok(target_id)
}

pub fn start_lifecycle_monitor(debug_port: u16, primary_target_id: String) {
    thread::spawn(move || monitor_primary_target(debug_port, &primary_target_id));
}

fn monitor_primary_target(debug_port: u16, primary_target_id: &str) {
    loop {
        thread::sleep(Duration::from_millis(200));
        let Ok(targets) = list_targets(debug_port) else {
            return;
        };
        match lifecycle_action(primary_target_id, &targets) {
            CdpLifecycleAction::Wait => {}
            CdpLifecycleAction::CloseBrowser => {
                let _ = close_browser(debug_port);
                return;
            }
        }
    }
}

fn lifecycle_action(primary_target_id: &str, targets: &[CdpTarget]) -> CdpLifecycleAction {
    if targets.iter().any(|target| target.id == primary_target_id) {
        CdpLifecycleAction::Wait
    } else {
        CdpLifecycleAction::CloseBrowser
    }
}

fn browser_close_message() -> Value {
    json!({ "id": 1, "method": "Browser.close", "params": {} })
}

fn close_browser(debug_port: u16) -> Result<(), String> {
    let version = http_get_json(debug_port, "/json/version")?;
    let websocket = version
        .get("webSocketDebuggerUrl")
        .and_then(Value::as_str)
        .ok_or("CDP browser target has no WebSocket URL")?;
    validate_cdp_websocket_url(websocket, debug_port)?;
    let (mut socket, _) = connect_cdp_websocket(websocket, debug_port)?;
    socket
        .send(Message::Text(browser_close_message().to_string()))
        .map_err(|err| err.to_string())
}

fn send_cdp(
    socket: &mut WebSocket<TcpStream>,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let body = json!({ "id": id, "method": method, "params": params });
    let deadline = Instant::now() + CDP_IO_TIMEOUT;
    socket
        .get_mut()
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let result = (|| {
        loop {
            match socket.send(Message::Text(body.to_string())) {
                Ok(()) => break,
                Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(format!("cdp {method} timed out"));
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error.to_string()),
            }
        }

        loop {
            if Instant::now() >= deadline {
                return Err(format!("cdp {method} timed out"));
            }
            match socket.read() {
                Ok(Message::Text(text)) => {
                    let parsed: Value =
                        serde_json::from_str(&text).map_err(|err| err.to_string())?;
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
                Ok(_) => {}
                Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error.to_string()),
            }
        }
    })();
    let restore = socket.get_mut().set_nonblocking(false);
    if let Err(error) = restore {
        return Err(error.to_string());
    }
    result
}

fn connect_cdp_websocket(
    url: &str,
    expected_port: u16,
) -> Result<
    (
        WebSocket<TcpStream>,
        tungstenite::handshake::client::Response,
    ),
    String,
> {
    let addr = websocket_socket_addr(url, expected_port)?;
    let stream = TcpStream::connect_timeout(&addr, CDP_IO_TIMEOUT)
        .map_err(|error| format!("CDP WebSocket connect timed out or failed: {error}"))?;
    stream
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let request = url
        .into_client_request()
        .map_err(|error| format!("invalid CDP WebSocket request: {error}"))?;
    let mut handshake = ClientHandshake::start(stream, request, None)
        .map_err(|error| format!("CDP WebSocket handshake failed: {error}"))?;
    let deadline = Instant::now() + CDP_IO_TIMEOUT;
    let (mut socket, response) = loop {
        if Instant::now() >= deadline {
            return Err("CDP WebSocket handshake timed out".into());
        }
        match handshake.handshake() {
            Ok(result) => break result,
            Err(HandshakeError::Interrupted(next)) => {
                handshake = next;
                thread::sleep(Duration::from_millis(5));
            }
            Err(HandshakeError::Failure(error)) => {
                return Err(format!("CDP WebSocket handshake failed: {error}"));
            }
        }
    };
    socket
        .get_mut()
        .set_nonblocking(false)
        .map_err(|error| error.to_string())?;
    socket
        .get_mut()
        .set_read_timeout(Some(CDP_IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    socket
        .get_mut()
        .set_write_timeout(Some(CDP_IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    Ok((socket, response))
}

fn websocket_socket_addr(url: &str, expected_port: u16) -> Result<SocketAddr, String> {
    let uri: tungstenite::http::Uri = url
        .parse()
        .map_err(|error| format!("invalid CDP WebSocket URL: {error}"))?;
    validate_cdp_websocket_url(url, expected_port)?;
    let host = uri.host().ok_or("CDP WebSocket URL has no host")?;
    let ip: IpAddr = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse()
        .map_err(|_| "CDP WebSocket host must be a loopback IP address".to_string())?;
    let port = uri.port_u16().ok_or("CDP WebSocket URL has no port")?;
    Ok(SocketAddr::new(ip, port))
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
    let socket_addr: SocketAddr = addr
        .parse()
        .map_err(|error| format!("invalid CDP address {addr}: {error}"))?;
    let deadline = Instant::now() + CDP_IO_TIMEOUT;
    let mut stream =
        TcpStream::connect_timeout(&socket_addr, CDP_IO_TIMEOUT).map_err(|err| err.to_string())?;
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or("cdp http operation timed out")?;
    stream
        .set_read_timeout(Some(remaining))
        .map_err(|err| err.to_string())?;
    stream
        .set_write_timeout(Some(remaining))
        .map_err(|err| err.to_string())?;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {host}:{debug_port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|err| err.to_string())?;
    let mut response = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("cdp http operation timed out")?;
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|err| err.to_string())?;
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
    fn cdp_http_has_an_overall_deadline_for_a_slow_local_endpoint() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]";
            for byte in response {
                if stream.write_all(&[*byte]).is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        });

        let started = Instant::now();
        let result = http_get_json_host("127.0.0.1", port, "/json/list");
        assert!(
            result.is_err(),
            "a slow endpoint must not complete normally"
        );
        assert!(
            started.elapsed() < Duration::from_millis(2_500),
            "slow CDP endpoint exceeded its bounded operation deadline"
        );
        server.join().unwrap();
    }

    #[test]
    fn cdp_websocket_handshake_has_a_finite_timeout() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let response = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: invalid\r\n\r\n";
            for byte in response {
                if stream.write_all(&[*byte]).is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(60));
            }
        });

        let started = Instant::now();
        let result =
            connect_cdp_websocket(&format!("ws://127.0.0.1:{port}/devtools/page/test"), port);
        assert!(result.is_err());
        assert!(
            started.elapsed() < Duration::from_millis(2_500),
            "CDP WebSocket handshake was not bounded"
        );
        server.join().unwrap();
    }

    #[test]
    fn cdp_command_read_has_an_overall_deadline_for_a_fragmented_frame() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let payload = br#"{"id":1,"result":{"value":"0123456789abcdef"}}"#;
            let mut frame = vec![0x81, payload.len() as u8];
            frame.extend_from_slice(payload);
            let raw = socket.get_mut();
            for byte in frame {
                if raw.write_all(&[byte]).is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(70));
            }
        });

        let (mut socket, _) =
            connect_cdp_websocket(&format!("ws://127.0.0.1:{port}/devtools/page/test"), port)
                .unwrap();
        let started = Instant::now();
        let result = send_cdp(&mut socket, 1, "Runtime.evaluate", json!({}));
        assert!(result.is_err(), "a slow fragmented response must time out");
        assert!(
            started.elapsed() < Duration::from_millis(2_500),
            "fragmented CDP command exceeded its overall deadline"
        );
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
