//! [INPUT]: 依赖父级 CDP 共享类型与loopback HTTP/WebSocket、guard 与既有 deadline 适配。
//! [OUTPUT]: 原样提供既有transport函数；保持父级导出和调用契约。
//! [POS]: CDP 的transport职责边界；不复制 Runtime 或建立新入口。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use super::*;

pub fn validate_cdp_websocket_url(url: &str, expected_port: u16) -> Result<(), String> {
    let uri: tungstenite::http::Uri = url
        .parse()
        .map_err(|err| format!("invalid CDP WebSocket URL: {err}"))?;
    websocket_socket_addr_from_uri(&uri, expected_port).map(|_| ())
}

pub(super) fn websocket_socket_addr_from_uri(
    uri: &tungstenite::http::Uri,
    expected_port: u16,
) -> Result<SocketAddr, String> {
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
    Ok(SocketAddr::new(ip, expected_port))
}

pub(super) fn ensure_cdp_deadline_if_present(deadline: Option<Instant>) -> Result<(), String> {
    if let Some(deadline) = deadline {
        ensure_cdp_deadline(deadline)?;
    }
    Ok(())
}

pub(super) fn ensure_cdp_deadline(deadline: Instant) -> Result<(), String> {
    if Instant::now() < deadline {
        Ok(())
    } else {
        Err(CDP_DEADLINE_EXPIRED_ERROR.into())
    }
}

pub(super) fn bounded_cdp_io_deadline(deadline: Instant) -> Result<Instant, String> {
    let now = Instant::now();
    if now >= deadline {
        return Err(CDP_DEADLINE_EXPIRED_ERROR.into());
    }
    Ok(std::cmp::min(deadline, now + CDP_IO_TIMEOUT))
}

pub(super) fn cdp_remaining_until(deadline: Instant) -> Result<Duration, String> {
    deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(|| CDP_DEADLINE_EXPIRED_ERROR.to_string())
}

pub(super) fn send_cdp(
    socket: &mut WebSocket<TcpStream>,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let deadline = Instant::now() + CDP_IO_TIMEOUT;
    socket
        .get_mut()
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let result = send_cdp_with_deadline(socket, id, method, params, deadline);
    let restore = socket.get_mut().set_nonblocking(false);
    if let Err(error) = restore {
        return Err(error.to_string());
    }
    result
}

pub(crate) fn send_guarded_cdp<G>(
    socket: &mut WebSocket<TcpStream>,
    id: u64,
    method: &str,
    params: Value,
    connection_guard: &G,
) -> Result<Value, String>
where
    G: Fn(&TcpStream) -> Result<(), String>,
{
    connection_guard(socket.get_ref())?;
    send_cdp(socket, id, method, params)
}

pub(super) fn send_guarded_cdp_with_deadline<G>(
    socket: &mut WebSocket<TcpStream>,
    id: u64,
    method: &str,
    params: Value,
    connection_guard: &G,
    deadline: Option<Instant>,
) -> Result<Value, String>
where
    G: Fn(&TcpStream) -> Result<(), String>,
{
    match deadline {
        Some(deadline) => {
            send_guarded_cdp_until(socket, id, method, params, connection_guard, deadline)
        }
        None => send_guarded_cdp(socket, id, method, params, connection_guard),
    }
}

pub(super) fn send_guarded_cdp_until<G>(
    socket: &mut WebSocket<TcpStream>,
    id: u64,
    method: &str,
    params: Value,
    connection_guard: &G,
    deadline: Instant,
) -> Result<Value, String>
where
    G: Fn(&TcpStream) -> Result<(), String>,
{
    ensure_cdp_deadline(deadline)?;
    connection_guard(socket.get_ref())?;
    send_cdp_until(socket, id, method, params, deadline)
}

pub(super) fn send_cdp_until(
    socket: &mut WebSocket<TcpStream>,
    id: u64,
    method: &str,
    params: Value,
    deadline: Instant,
) -> Result<Value, String> {
    let io_deadline = bounded_cdp_io_deadline(deadline)?;
    socket
        .get_mut()
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let result = send_cdp_with_deadline(socket, id, method, params, io_deadline);
    let restore = socket.get_mut().set_nonblocking(false);
    if let Err(error) = restore {
        return Err(error.to_string());
    }
    result
}

pub(super) fn send_cdp_with_deadline<S: Read + Write>(
    socket: &mut WebSocket<S>,
    id: u64,
    method: &str,
    params: Value,
    deadline: Instant,
) -> Result<Value, String> {
    let body = json!({ "id": id, "method": method, "params": params });
    let message = Message::Text(body.to_string().into());
    if Instant::now() >= deadline {
        return Err(format!("cdp {method} timed out"));
    }

    // 先把命令写入 WebSocket 缓冲区；即使底层只写了一部分，也只能重试 flush。
    // 再次 write/send 会重新排队同一命令，导致 CDP 收到重复请求。
    match socket.write(message) {
        Ok(()) => {}
        Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {}
        Err(tungstenite::Error::WriteBufferFull(_)) => {
            return Err(format!("cdp {method} write buffer full"));
        }
        Err(error) => return Err(error.to_string()),
    }

    loop {
        if Instant::now() >= deadline {
            return Err(format!("cdp {method} timed out"));
        }
        match socket.flush() {
            Ok(()) => break,
            Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(tungstenite::Error::WriteBufferFull(_)) => {
                return Err(format!("cdp {method} write buffer full"));
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
                let parsed: Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
                if parsed.get("method").and_then(Value::as_str) == Some("Inspector.targetCrashed") {
                    return Err(TARGET_CRASHED_ERROR.into());
                }
                if parsed.get("id").and_then(Value::as_u64) == Some(id) {
                    if parsed.pointer("/error/message").and_then(Value::as_str)
                        == Some("Target crashed")
                    {
                        return Err(TARGET_CRASHED_ERROR.into());
                    }
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
}

pub(crate) fn connect_cdp_websocket(
    url: &str,
    expected_port: u16,
) -> Result<WebSocket<TcpStream>, String> {
    connect_cdp_websocket_with_deadline(url, expected_port, None)
}

pub(super) fn connect_cdp_websocket_with_deadline(
    url: &str,
    expected_port: u16,
    deadline: Option<Instant>,
) -> Result<WebSocket<TcpStream>, String> {
    let addr = websocket_socket_addr(url, expected_port)?;
    let connect_deadline = match deadline {
        Some(deadline) => bounded_cdp_io_deadline(deadline)?,
        None => Instant::now() + CDP_IO_TIMEOUT,
    };
    let stream = TcpStream::connect_timeout(&addr, cdp_remaining_until(connect_deadline)?)
        .map_err(|error| format!("CDP WebSocket connect timed out or failed: {error}"))?;
    stream
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let request = url
        .into_client_request()
        .map_err(|error| format!("invalid CDP WebSocket request: {error}"))?;
    let handshake_deadline = match deadline {
        Some(deadline) => bounded_cdp_io_deadline(deadline)?,
        None => Instant::now() + CDP_IO_TIMEOUT,
    };
    let mut handshake = ClientHandshake::start(stream, request, None)
        .map_err(|error| format!("CDP WebSocket handshake failed: {error}"))?;
    let (mut socket, _) = loop {
        if Instant::now() >= handshake_deadline {
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
    if let Some(deadline) = deadline {
        ensure_cdp_deadline(deadline)?;
    }
    socket
        .get_mut()
        .set_nonblocking(false)
        .map_err(|error| error.to_string())?;
    let socket_timeout = match deadline {
        Some(deadline) => cdp_remaining_until(bounded_cdp_io_deadline(deadline)?)?,
        None => CDP_IO_TIMEOUT,
    };
    socket
        .get_mut()
        .set_read_timeout(Some(socket_timeout))
        .map_err(|error| error.to_string())?;
    socket
        .get_mut()
        .set_write_timeout(Some(socket_timeout))
        .map_err(|error| error.to_string())?;
    Ok(socket)
}

pub(super) fn websocket_socket_addr(url: &str, expected_port: u16) -> Result<SocketAddr, String> {
    let uri: tungstenite::http::Uri = url
        .parse()
        .map_err(|error| format!("invalid CDP WebSocket URL: {error}"))?;
    websocket_socket_addr_from_uri(&uri, expected_port)
}

pub(crate) fn list_targets(debug_port: u16) -> Result<Vec<CdpTarget>, String> {
    list_targets_with_timeout(debug_port, CDP_IO_TIMEOUT)
}

pub(super) fn list_targets_with_timeout(
    debug_port: u16,
    timeout: Duration,
) -> Result<Vec<CdpTarget>, String> {
    list_targets_for_platform(debug_port, timeout, cfg!(target_os = "windows"))
}

pub(super) fn list_targets_for_platform(
    debug_port: u16,
    timeout: Duration,
    windows: bool,
) -> Result<Vec<CdpTarget>, String> {
    if windows {
        return list_targets_until(debug_port, Instant::now() + timeout);
    }
    // Windows 的总预算不改变 Mac 原有的逐地址、逐路径回退预算。
    let raw = http_get_json_for_platform(debug_port, "/json/list", timeout, false)
        .or_else(|_| http_get_json_for_platform(debug_port, "/json", timeout, false))?;
    parse_cdp_targets(raw, debug_port)
}

pub(super) fn list_targets_with_deadline(
    debug_port: u16,
    deadline: Option<Instant>,
) -> Result<Vec<CdpTarget>, String> {
    match deadline {
        Some(deadline) => list_targets_until(debug_port, deadline),
        None => list_targets(debug_port),
    }
}

pub(super) fn list_targets_until(
    debug_port: u16,
    deadline: Instant,
) -> Result<Vec<CdpTarget>, String> {
    let raw = http_get_json_until(debug_port, "/json/list", deadline).or_else(|error| {
        ensure_cdp_deadline(deadline).map_err(|_| error)?;
        http_get_json_until(debug_port, "/json", deadline)
    })?;
    parse_cdp_targets(raw, debug_port)
}

pub(super) fn parse_cdp_targets(raw: Value, debug_port: u16) -> Result<Vec<CdpTarget>, String> {
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

#[cfg(test)]
pub(super) fn http_get_json(debug_port: u16, path: &str) -> Result<Value, String> {
    http_get_json_with_timeout(debug_port, path, CDP_IO_TIMEOUT)
}

#[cfg(test)]
pub(super) fn http_get_json_with_timeout(
    debug_port: u16,
    path: &str,
    timeout: Duration,
) -> Result<Value, String> {
    http_get_json_for_platform(debug_port, path, timeout, cfg!(target_os = "windows"))
}

pub(super) fn http_get_json_for_platform(
    debug_port: u16,
    path: &str,
    timeout: Duration,
    windows: bool,
) -> Result<Value, String> {
    if windows {
        return http_get_json_until(debug_port, path, Instant::now() + timeout);
    }
    let mut errors = Vec::new();
    for host in cdp_hosts_for_platform(false) {
        match http_get_json_host_with_timeout(host, debug_port, path, timeout) {
            Ok(value) => return Ok(value),
            Err(error) => errors.push(format!("{host}: {error}")),
        }
    }
    Err(format!("cdp http failed: {}", errors.join("; ")))
}

pub(super) fn http_get_json_until(
    debug_port: u16,
    path: &str,
    deadline: Instant,
) -> Result<Value, String> {
    let mut errors = Vec::new();
    for host in cdp_hosts_for_platform(cfg!(target_os = "windows")) {
        if Instant::now() >= deadline {
            break;
        }
        match http_get_json_host_until(host, debug_port, path, deadline) {
            Ok(value) => return Ok(value),
            Err(error) => errors.push(format!("{host}: {error}")),
        }
    }
    if errors.is_empty() {
        return Err(CDP_DEADLINE_EXPIRED_ERROR.into());
    }
    Err(format!("cdp http failed: {}", errors.join("; ")))
}

pub(super) fn cdp_hosts_for_platform(windows: bool) -> &'static [&'static str] {
    if windows {
        &["127.0.0.1"]
    } else {
        &["127.0.0.1", "[::1]"]
    }
}

#[cfg(test)]
pub(super) fn http_get_json_host(host: &str, debug_port: u16, path: &str) -> Result<Value, String> {
    http_get_json_host_with_timeout(host, debug_port, path, CDP_IO_TIMEOUT)
}

pub(super) fn http_get_json_host_with_timeout(
    host: &str,
    debug_port: u16,
    path: &str,
    timeout: Duration,
) -> Result<Value, String> {
    http_get_json_host_until(host, debug_port, path, Instant::now() + timeout)
}

pub(super) fn http_get_json_host_until(
    host: &str,
    debug_port: u16,
    path: &str,
    deadline: Instant,
) -> Result<Value, String> {
    let addr = format!("{host}:{debug_port}");
    let socket_addr: SocketAddr = addr
        .parse()
        .map_err(|error| format!("invalid CDP address {addr}: {error}"))?;
    let io_deadline = bounded_cdp_io_deadline(deadline)?;
    let mut stream = TcpStream::connect_timeout(&socket_addr, cdp_remaining_until(io_deadline)?)
        .map_err(|err| err.to_string())?;
    let remaining = cdp_remaining_until(io_deadline)?;
    stream
        .set_read_timeout(Some(remaining))
        .map_err(|err| err.to_string())?;
    stream
        .set_write_timeout(Some(remaining))
        .map_err(|err| err.to_string())?;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {host}:{debug_port}\r\nConnection: close\r\n\r\n");
    ensure_cdp_deadline(io_deadline)?;
    stream
        .write_all(request.as_bytes())
        .map_err(|err| err.to_string())?;
    let mut response = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let remaining = cdp_remaining_until(io_deadline)?;
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

pub(super) fn response_frame(response: &[u8]) -> Result<Option<(usize, usize)>, String> {
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
