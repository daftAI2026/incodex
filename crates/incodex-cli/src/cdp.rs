//! Localhost Chrome DevTools Protocol client for `incodex open`.
//!
//! It injects the shared Runtime only into the top-level Codex page and keeps
//! every HTTP and WebSocket operation bounded by a deadline.
use std::collections::HashSet;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::{client::ClientHandshake, HandshakeError};
use tungstenite::{Message, WebSocket};

use crate::profile_mask::{ProfileAvatar, ProfileMask};

#[path = "cdp_mode.rs"]
mod mode;
pub(crate) use mode::Readiness as CodexModeReadiness;
use mode::{Action as CodexModeAction, PageState as CodexModePageState};

const INJECT_JS: &str = include_str!("../../../dist/incodex-inject.js");
const INJECT_PREFIX: &str = "window.__incodexIncognito=true;";
const OFFICIAL_CODEX_PAGE_URL: &str = "app://-/index.html";
const MAX_HTTP_RESPONSE_BYTES: usize = 1024 * 1024;
const CDP_IO_TIMEOUT: Duration = Duration::from_secs(2);
const LIFECYCLE_POLL_INTERVAL: Duration = Duration::from_millis(200);
const PRIMARY_TARGET_MISSING_POLLS: u8 = 2;
const WINDOWS_CDP_FAILURE_POLLS: u8 = 3;
const WINDOWS_LIFECYCLE_CDP_TIMEOUT: Duration = Duration::from_millis(400);
const PROFILE_MASK_FAILURE_POLLS: u8 = 2;
const WINDOWS_PROFILE_MASK_TRANSPORT_FAILURE_POLLS: u8 = 4;
const BROWSER_CLOSE_ATTEMPTS: u8 = 3;
pub const OFFICIAL_NEW_CODEX_URL: &str = "codex://new?mode=codex";

#[derive(Debug, Clone)]
pub struct CdpTarget {
    pub id: String,
    pub r#type: String,
    pub url: String,
    pub ws: String,
}

#[derive(Debug, Clone, Default)]
pub struct InjectionOptions {
    pub locale: Option<String>,
    pub profile_mask: Option<ProfileMask>,
}

#[derive(Clone, Copy)]
struct LifecyclePolicy {
    max_consecutive_errors: Option<u8>,
    adopt_replacement: bool,
    cdp_timeout: Option<Duration>,
}

struct InjectionPayload<'a> {
    source: &'a str,
    health_expression: &'a str,
    require_profile_mask: bool,
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
    debug_launch_args_for_platform(user_data_dir, debug_port, cfg!(target_os = "windows"))
}

fn debug_launch_args_for_platform(
    user_data_dir: &str,
    debug_port: u16,
    windows: bool,
) -> Vec<String> {
    let mut args = launch_arg_prefix_for_platform(user_data_dir, windows);
    if windows {
        args.push("--remote-debugging-address=127.0.0.1".to_string());
    }
    args.extend([
        format!("--remote-debugging-port={debug_port}"),
        format!("--remote-allow-origins=http://127.0.0.1:{debug_port}"),
        OFFICIAL_NEW_CODEX_URL.to_string(),
    ]);
    args
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn launch_arg_prefix(user_data_dir: &str) -> Vec<String> {
    launch_arg_prefix_for_platform(user_data_dir, cfg!(target_os = "windows"))
}

fn launch_arg_prefix_for_platform(user_data_dir: &str, windows: bool) -> Vec<String> {
    if windows {
        vec![format!("--user-data-dir={user_data_dir}")]
    } else {
        vec![
            "-NSAutomaticWindowAnimationsEnabled".to_string(),
            "false".to_string(),
            format!("--user-data-dir={user_data_dir}"),
        ]
    }
}

pub fn inject_source() -> String {
    inject_source_for_options(&InjectionOptions::default())
}

pub fn inject_source_for_locale(locale: Option<&str>) -> String {
    inject_source_for_options(&InjectionOptions {
        locale: locale.map(str::to_string),
        profile_mask: None,
    })
}

pub fn inject_source_for_options(options: &InjectionOptions) -> String {
    let locale = json_string(options.locale.as_deref().unwrap_or(""));
    let platform = if cfg!(target_os = "windows") {
        "window.__incodexPlatform=\"win32\";"
    } else {
        ""
    };
    let profile_bootstrap = match &options.profile_mask {
        Some(profile_mask) => format!(
            "(window.top===window&&window.location.href==={})?{}:null",
            json_string(OFFICIAL_CODEX_PAGE_URL),
            profile_mask_json(profile_mask)
        ),
        None => "null".to_string(),
    };
    format!(
        "{INJECT_PREFIX}window.__incodexLocale={locale};{platform}window.__incodexProfileMask={profile_bootstrap};\n{INJECT_JS}"
    )
}

fn json_string(value: &str) -> String {
    Value::String(value.to_string()).to_string()
}

fn profile_mask_json(mask: &ProfileMask) -> String {
    let name = json_string(&mask.name);
    let avatar = match &mask.avatar {
        ProfileAvatar::Generated => "{\"kind\":\"generated\"}".to_string(),
        ProfileAvatar::DataUrl(data_url) => {
            let data_url = json_string(data_url);
            format!("{{\"dataUrl\":{data_url}}}")
        }
    };
    format!("{{\"name\":{name},\"avatar\":{avatar}}}")
}

pub fn ui_ready_expression() -> &'static str {
    "(() => ({button: Boolean(document.querySelector('[data-incodex-privacy-toggle]')), banner: Boolean((document.querySelector('[data-incodex-banner-host]') && document.querySelector('[data-incodex-landing]')) || (() => { try { return sessionStorage.getItem('incodex-banner-dismissed') === '1'; } catch { return false; } })())}))()"
}

pub fn ui_ready_expression_for_options(options: &InjectionOptions) -> String {
    if options.profile_mask.is_none() {
        return ui_ready_expression().to_string();
    }
    format!(
        "(() => {{ const base = {base}; base.profileMask = window.__incodexProfileMaskHealth === true; return base; }})()",
        base = ui_ready_expression()
    )
}

pub fn validate_ui_probe_result(response: &Value) -> Result<(), String> {
    validate_ui_probe_result_for_options(response, false)
}

pub fn validate_ui_probe_result_for_options(
    response: &Value,
    require_profile_mask: bool,
) -> Result<(), String> {
    let malformed = || "malformed Incodex UI probe result".to_string();
    let value = response
        .pointer("/result/result/value")
        .ok_or_else(malformed)?;
    let object = value.as_object().ok_or_else(malformed)?;
    let button = object
        .get("button")
        .and_then(Value::as_bool)
        .ok_or_else(malformed)?;
    let banner = object
        .get("banner")
        .and_then(Value::as_bool)
        .ok_or_else(malformed)?;

    if require_profile_mask {
        let profile_mask = object
            .get("profileMask")
            .and_then(Value::as_bool)
            .ok_or_else(malformed)?;
        if !profile_mask {
            return Err("Incodex profile mask is not mounted uniquely".into());
        }
    }

    match (button, banner) {
        (true, true) => Ok(()),
        (false, true) => Err("Incodex button is not mounted yet".into()),
        (true, false) => Err("Incodex banner is not mounted yet".into()),
        (false, false) => Err("Incodex button and banner are not mounted yet".into()),
    }
}

pub fn validate_cdp_websocket_url(url: &str, expected_port: u16) -> Result<(), String> {
    let uri: tungstenite::http::Uri = url
        .parse()
        .map_err(|err| format!("invalid CDP WebSocket URL: {err}"))?;
    websocket_socket_addr_from_uri(&uri, expected_port).map(|_| ())
}

fn websocket_socket_addr_from_uri(
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
    url == OFFICIAL_CODEX_PAGE_URL
}

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
    mut on_target: F,
    readiness: &mut CodexModeReadiness,
    connection_guard: &G,
) -> Result<String, String>
where
    F: FnMut(&str),
    G: Fn(&TcpStream) -> Result<(), String>,
{
    let source = inject_source_for_options(options);
    let health_expression = ui_ready_expression_for_options(options);
    let require_profile_mask = options.profile_mask.is_some();
    let payload = InjectionPayload {
        source: &source,
        health_expression: &health_expression,
        require_profile_mask,
    };
    let mut registered_script_targets = HashSet::new();
    let mut last = "cdp page not ready".to_string();
    let mut refused = 0u8;
    for _ in 0..8 {
        ensure_injection_active(process_alive)?;
        match try_inject(
            debug_port,
            &payload,
            &mut registered_script_targets,
            process_alive,
            &mut on_target,
            readiness,
            connection_guard,
        ) {
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

fn try_inject<F, G>(
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
    ensure_injection_active(process_alive)?;
    let targets = list_targets(debug_port)?;
    ensure_injection_active(process_alive)?;
    let page = pick_codex_page_target(&targets).ok_or("no Codex page target")?;
    on_target(&page.id);
    let mut socket = connect_cdp_websocket(&page.ws, debug_port)?;
    ensure_injection_active(process_alive)?;
    send_guarded_cdp(&mut socket, 1, "Page.enable", json!({}), connection_guard)?;
    confirm_official_codex_mode(&mut socket, process_alive, readiness, connection_guard)?;
    ensure_injection_active(process_alive)?;
    if !registered_script_targets.contains(&page.id) {
        send_guarded_cdp(
            &mut socket,
            4,
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": payload.source }),
            connection_guard,
        )?;
        registered_script_targets.insert(page.id.clone());
    }
    ensure_injection_active(process_alive)?;
    send_guarded_cdp(
        &mut socket,
        5,
        "Runtime.evaluate",
        json!({ "expression": payload.source, "returnByValue": true }),
        connection_guard,
    )?;
    ensure_injection_active(process_alive)?;
    let health = send_guarded_cdp(
        &mut socket,
        6,
        "Runtime.evaluate",
        json!({ "expression": payload.health_expression, "returnByValue": true }),
        connection_guard,
    )?;
    validate_ui_probe_result_for_options(&health, payload.require_profile_mask)?;
    let target_id = page.id.clone();
    let _ = socket.close(None);
    Ok(target_id)
}

fn ensure_injection_active(process_alive: &AtomicBool) -> Result<(), String> {
    if process_alive.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err("CDP injection cancelled after child exit".into())
    }
}

fn confirm_official_codex_mode<G>(
    socket: &mut WebSocket<TcpStream>,
    process_alive: &AtomicBool,
    readiness: &mut CodexModeReadiness,
    connection_guard: &G,
) -> Result<(), String>
where
    G: Fn(&TcpStream) -> Result<(), String>,
{
    let mut next_id = 10;
    loop {
        ensure_injection_active(process_alive)?;
        let response = send_guarded_cdp(
            socket,
            next_id,
            "Runtime.evaluate",
            json!({
                "expression": mode::PROBE_EXPRESSION,
                "returnByValue": true
            }),
            connection_guard,
        )?;
        next_id += 1;

        match readiness.observe(codex_mode_page_state(&response)?) {
            CodexModeAction::Confirmed => return Ok(()),
            CodexModeAction::Wait => {}
            CodexModeAction::SelectFallback => {
                dispatch_codex_mode_fallback(socket, &mut next_id, connection_guard)?;
            }
            CodexModeAction::Unresolved => {
                return Err("Codex mode remained unavailable after keyboard fallback".into());
            }
        }

        thread::sleep(mode::POLL_INTERVAL);
    }
}

fn codex_mode_page_state(response: &Value) -> Result<CodexModePageState, String> {
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
    if blocker_visible || !mode_available {
        return Ok(CodexModePageState::Pending);
    }
    Ok(CodexModePageState::Other)
}

fn dispatch_codex_mode_fallback<G>(
    socket: &mut WebSocket<TcpStream>,
    next_id: &mut u64,
    connection_guard: &G,
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
        send_guarded_cdp(
            socket,
            *next_id,
            "Input.dispatchKeyEvent",
            key(r#type),
            connection_guard,
        )?;
        *next_id += 1;
    }
    Ok(())
}

pub fn start_primary_lifecycle_monitor(
    debug_port: u16,
    process_alive: Arc<AtomicBool>,
) -> Result<(), String> {
    let targets = list_targets(debug_port)?;
    let target_id = pick_codex_page_target(&targets)
        .ok_or("no Codex page target")?
        .id
        .clone();
    start_lifecycle_monitor(debug_port, target_id, process_alive);
    Ok(())
}

pub fn start_lifecycle_monitor(
    debug_port: u16,
    primary_target_id: String,
    process_alive: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        monitor_primary_target(debug_port, &primary_target_id, &process_alive, || {
            let _ = close_browser_with_retries(debug_port);
            false
        })
    });
}

pub fn start_lifecycle_signal_monitor(
    debug_port: u16,
    primary_target_id: String,
    process_alive: Arc<AtomicBool>,
    close_requested: Arc<AtomicBool>,
    cdp_failed: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        monitor_primary_target_with_failure_limit(
            debug_port,
            &primary_target_id,
            &process_alive,
            LifecyclePolicy {
                max_consecutive_errors: Some(WINDOWS_CDP_FAILURE_POLLS),
                adopt_replacement: false,
                cdp_timeout: Some(WINDOWS_LIFECYCLE_CDP_TIMEOUT),
            },
            || {
                close_requested.store(true, Ordering::Release);
                true
            },
            || cdp_failed.store(true, Ordering::Release),
        )
    })
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn monitor_profile_mask_health<F>(
    debug_port: u16,
    process_alive: &AtomicBool,
    on_failure: F,
) -> Result<(), String>
where
    F: FnMut(&str),
{
    monitor_profile_mask_health_with_guard(
        debug_port,
        process_alive,
        on_failure,
        &|_| Ok(()),
        false,
    )
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn start_profile_mask_signal_monitor<G>(
    debug_port: u16,
    process_alive: Arc<AtomicBool>,
    cdp_failed: Arc<AtomicBool>,
    connection_guard: G,
) -> thread::JoinHandle<()>
where
    G: Fn(&TcpStream) -> Result<(), String> + Send + 'static,
{
    thread::spawn(move || {
        let _ = monitor_profile_mask_health_with_guard(
            debug_port,
            &process_alive,
            |_| cdp_failed.store(true, Ordering::Release),
            &connection_guard,
            true,
        );
    })
}

fn monitor_profile_mask_health_with_guard<F, G>(
    debug_port: u16,
    process_alive: &AtomicBool,
    mut on_failure: F,
    connection_guard: &G,
    defer_missing_target: bool,
) -> Result<(), String>
where
    F: FnMut(&str),
    G: Fn(&TcpStream) -> Result<(), String>,
{
    let mut failures = ProfileMaskFailureCounters::for_platform(cfg!(target_os = "windows"));
    while process_alive.load(Ordering::Acquire) {
        thread::sleep(LIFECYCLE_POLL_INTERVAL);
        if !process_alive.load(Ordering::Acquire) {
            return Ok(());
        }
        let (error, failure_kind, failure_limit) =
            match probe_profile_mask_health(debug_port, process_alive, connection_guard) {
                Ok(true) => {
                    failures.clear();
                    continue;
                }
                Ok(false) => (
                    "Incodex profile mask could not be restored".to_string(),
                    ProfileMaskFailureKind::Unhealthy,
                    PROFILE_MASK_FAILURE_POLLS,
                ),
                Err(ProfileMaskProbeError::TargetMissing) if defer_missing_target => continue,
                Err(ProfileMaskProbeError::TargetMissing) => (
                    "no Codex page target".to_string(),
                    ProfileMaskFailureKind::Unhealthy,
                    PROFILE_MASK_FAILURE_POLLS,
                ),
                Err(ProfileMaskProbeError::ProbeFailed(error)) => (
                    error,
                    ProfileMaskFailureKind::Transport,
                    profile_mask_transport_failure_polls(cfg!(target_os = "windows")),
                ),
            };
        if failures.record(failure_kind, failure_limit) {
            on_failure(&error);
            return Err(error);
        }
    }
    Ok(())
}

enum ProfileMaskFailureCounters {
    Consecutive(u8),
    Independent { unhealthy: u8, transport: u8 },
}

enum ProfileMaskFailureKind {
    Unhealthy,
    Transport,
}

impl ProfileMaskFailureCounters {
    fn for_platform(windows: bool) -> Self {
        if windows {
            Self::Independent {
                unhealthy: 0,
                transport: 0,
            }
        } else {
            Self::Consecutive(0)
        }
    }

    fn clear(&mut self) {
        match self {
            Self::Consecutive(failures) => *failures = 0,
            Self::Independent {
                unhealthy,
                transport,
            } => {
                *unhealthy = 0;
                *transport = 0;
            }
        }
    }

    fn record(&mut self, kind: ProfileMaskFailureKind, limit: u8) -> bool {
        let counter = match self {
            Self::Consecutive(failures) => failures,
            Self::Independent {
                unhealthy,
                transport,
            } => match kind {
                ProfileMaskFailureKind::Unhealthy => unhealthy,
                ProfileMaskFailureKind::Transport => transport,
            },
        };
        *counter = counter.saturating_add(1);
        *counter >= limit
    }
}

fn profile_mask_transport_failure_polls(windows: bool) -> u8 {
    if windows {
        WINDOWS_PROFILE_MASK_TRANSPORT_FAILURE_POLLS
    } else {
        PROFILE_MASK_FAILURE_POLLS
    }
}

enum ProfileMaskProbeError {
    TargetMissing,
    ProbeFailed(String),
}

fn probe_profile_mask_health<G>(
    debug_port: u16,
    process_alive: &AtomicBool,
    connection_guard: &G,
) -> Result<bool, ProfileMaskProbeError>
where
    G: Fn(&TcpStream) -> Result<(), String>,
{
    ensure_injection_active(process_alive).map_err(ProfileMaskProbeError::ProbeFailed)?;
    let targets = list_targets(debug_port).map_err(ProfileMaskProbeError::ProbeFailed)?;
    let page = pick_codex_page_target(&targets).ok_or(ProfileMaskProbeError::TargetMissing)?;
    let mut socket =
        connect_cdp_websocket(&page.ws, debug_port).map_err(ProfileMaskProbeError::ProbeFailed)?;
    let response = send_guarded_cdp(
        &mut socket,
        1,
        "Runtime.evaluate",
        json!({
            "expression": profile_mask_health_expression(),
            "returnByValue": true
        }),
        connection_guard,
    )
    .map_err(ProfileMaskProbeError::ProbeFailed)?;
    let healthy = response
        .pointer("/result/result/value")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            ProfileMaskProbeError::ProbeFailed("malformed profile mask health result".to_string())
        })?;
    Ok(healthy)
}

fn profile_mask_health_expression() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "window.__incodexRefreshProfileMaskHealth?.() === true"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "window.__incodexProfileMaskHealth === true"
    }
}

fn monitor_primary_target<F>(
    debug_port: u16,
    primary_target_id: &str,
    process_alive: &AtomicBool,
    on_close: F,
) where
    F: FnMut() -> bool,
{
    monitor_primary_target_with_failure_limit(
        debug_port,
        primary_target_id,
        process_alive,
        LifecyclePolicy {
            max_consecutive_errors: None,
            adopt_replacement: true,
            cdp_timeout: None,
        },
        on_close,
        || {},
    );
}

fn monitor_primary_target_with_failure_limit<F>(
    debug_port: u16,
    primary_target_id: &str,
    process_alive: &AtomicBool,
    policy: LifecyclePolicy,
    mut on_close: F,
    mut on_cdp_failure: impl FnMut(),
) where
    F: FnMut() -> bool,
{
    let mut primary_target_id = primary_target_id.to_string();
    let mut missing_polls = 0u8;
    let mut consecutive_errors = 0u8;
    while process_alive.load(Ordering::Acquire) {
        thread::sleep(LIFECYCLE_POLL_INTERVAL);
        let targets = match policy
            .cdp_timeout
            .map(|timeout| list_targets_with_timeout(debug_port, timeout))
            .unwrap_or_else(|| list_targets(debug_port))
        {
            Ok(targets) => {
                consecutive_errors = 0;
                targets
            }
            Err(_) => {
                consecutive_errors = consecutive_errors.saturating_add(1);
                if policy
                    .max_consecutive_errors
                    .is_some_and(|limit| consecutive_errors >= limit)
                {
                    on_cdp_failure();
                    return;
                }
                continue;
            }
        };
        if targets.iter().any(|target| target.id == primary_target_id) {
            missing_polls = 0;
            continue;
        }
        if let Some(replacement) = pick_codex_page_target(&targets) {
            if policy.adopt_replacement {
                primary_target_id.clone_from(&replacement.id);
                missing_polls = 0;
                continue;
            }
        }

        missing_polls = missing_polls.saturating_add(1);
        if missing_polls < PRIMARY_TARGET_MISSING_POLLS {
            continue;
        }
        if on_close() {
            return;
        }
        missing_polls = 0;
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
    let mut socket = connect_cdp_websocket(websocket, debug_port)?;
    socket
        .send(Message::Text(browser_close_message().to_string().into()))
        .map_err(|err| err.to_string())
}

fn close_browser_with_retries(debug_port: u16) -> Result<(), String> {
    let mut last_error = "Browser.close was not attempted".to_string();
    for attempt in 0..BROWSER_CLOSE_ATTEMPTS {
        match close_browser(debug_port) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = error,
        }
        if attempt + 1 < BROWSER_CLOSE_ATTEMPTS {
            thread::sleep(LIFECYCLE_POLL_INTERVAL);
        }
    }
    Err(last_error)
}

fn send_cdp(
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

fn send_guarded_cdp<G>(
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

fn send_cdp_with_deadline<S: Read + Write>(
    socket: &mut WebSocket<S>,
    id: u64,
    method: &str,
    params: Value,
    deadline: Instant,
) -> Result<Value, String> {
    let body = json!({ "id": id, "method": method, "params": params });
    let message = Message::Text(body.to_string().into());

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
}

fn connect_cdp_websocket(url: &str, expected_port: u16) -> Result<WebSocket<TcpStream>, String> {
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
    let (mut socket, _) = loop {
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
    Ok(socket)
}

fn websocket_socket_addr(url: &str, expected_port: u16) -> Result<SocketAddr, String> {
    let uri: tungstenite::http::Uri = url
        .parse()
        .map_err(|error| format!("invalid CDP WebSocket URL: {error}"))?;
    websocket_socket_addr_from_uri(&uri, expected_port)
}

fn list_targets(debug_port: u16) -> Result<Vec<CdpTarget>, String> {
    list_targets_with_timeout(debug_port, CDP_IO_TIMEOUT)
}

fn list_targets_with_timeout(debug_port: u16, timeout: Duration) -> Result<Vec<CdpTarget>, String> {
    let raw = http_get_json_with_timeout(debug_port, "/json/list", timeout)
        .or_else(|_| http_get_json_with_timeout(debug_port, "/json", timeout))?;
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
    http_get_json_with_timeout(debug_port, path, CDP_IO_TIMEOUT)
}

fn http_get_json_with_timeout(
    debug_port: u16,
    path: &str,
    timeout: Duration,
) -> Result<Value, String> {
    let mut errors = Vec::new();
    for host in cdp_hosts_for_platform(cfg!(target_os = "windows")) {
        match http_get_json_host_with_timeout(host, debug_port, path, timeout) {
            Ok(value) => return Ok(value),
            Err(error) => errors.push(format!("{host}: {error}")),
        }
    }
    Err(format!("cdp http failed: {}", errors.join("; ")))
}

fn cdp_hosts_for_platform(windows: bool) -> &'static [&'static str] {
    if windows {
        &["127.0.0.1"]
    } else {
        &["127.0.0.1", "[::1]"]
    }
}

#[cfg(test)]
fn http_get_json_host(host: &str, debug_port: u16, path: &str) -> Result<Value, String> {
    http_get_json_host_with_timeout(host, debug_port, path, CDP_IO_TIMEOUT)
}

fn http_get_json_host_with_timeout(
    host: &str,
    debug_port: u16,
    path: &str,
    timeout: Duration,
) -> Result<Value, String> {
    let addr = format!("{host}:{debug_port}");
    let socket_addr: SocketAddr = addr
        .parse()
        .map_err(|error| format!("invalid CDP address {addr}: {error}"))?;
    let deadline = Instant::now() + timeout;
    let mut stream =
        TcpStream::connect_timeout(&socket_addr, timeout).map_err(|err| err.to_string())?;
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
#[path = "cdp_partial_flush_tests.rs"]
mod partial_flush_tests;
#[cfg(test)]
#[path = "cdp_unit_tests.rs"]
mod unit_tests;

#[cfg(test)]
#[path = "cdp_ui_probe_tests.rs"]
mod ui_probe_tests;

#[cfg(test)]
#[path = "cdp_mode_tests.rs"]
mod mode_tests;

#[cfg(test)]
#[path = "cdp_lifecycle_tests.rs"]
mod lifecycle_tests;
