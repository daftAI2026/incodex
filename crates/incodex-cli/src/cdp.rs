/*
 * [INPUT]: 依赖 loopback HTTP/WebSocket、进程归属和编译期inject产物。
 * [OUTPUT]: 汇集传输、官方窗口焦点ACK、Runtime注入与身份监督接口。
 * [POS]: native open的CDP门面；各适配器共用同一Runtime与所有权规则。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
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

#[cfg(target_os = "macos")]
#[path = "cdp_focus.rs"]
mod focus;
#[cfg(target_os = "macos")]
pub(crate) use focus::focus_official_window;

#[path = "cdp_transport.rs"]
mod transport;
pub use transport::*;
#[path = "cdp_lifecycle.rs"]
mod lifecycle;
pub use lifecycle::*;
#[path = "cdp_injection.rs"]
mod injection;
pub use injection::*;

#[path = "cdp_mode.rs"]
mod mode;
pub(crate) use mode::Readiness as CodexModeReadiness;
pub(crate) use mode::POLL_INTERVAL as CODEX_MODE_POLL_INTERVAL;
use mode::{Action as CodexModeAction, PageState as CodexModePageState};

const INJECT_JS: &str = include_str!("../../../dist/incodex-inject.js");
const OFFICIAL_CODEX_PAGE_URL: &str = "app://-/index.html";
const MAX_HTTP_RESPONSE_BYTES: usize = 1024 * 1024;
const CDP_IO_TIMEOUT: Duration = Duration::from_secs(2);
const LIFECYCLE_POLL_INTERVAL: Duration = Duration::from_millis(200);
const PRIMARY_TARGET_MISSING_POLLS: u8 = 2;
const WINDOWS_CDP_FAILURE_POLLS: u8 = 3;
const WINDOWS_LIFECYCLE_CDP_TIMEOUT: Duration = Duration::from_millis(400);
#[cfg(any(target_os = "windows", test))]
const PROFILE_MASK_FAILURE_POLLS: u8 = 2;
#[cfg(any(target_os = "windows", test))]
const WINDOWS_PROFILE_MASK_TRANSPORT_FAILURE_POLLS: u8 = 4;
#[cfg(test)]
const BROWSER_CLOSE_ATTEMPTS: u8 = 3;
const CODEX_MODE_BLOCKED_ERROR: &str = "Codex mode is blocked by official UI";
const CODEX_MODE_WAITING_ERROR: &str = "Codex mode is not ready yet";
const CODEX_MODE_UNAVAILABLE_ERROR: &str =
    "Codex mode remained unavailable within its readiness deadline";
pub(crate) const UI_INJECTION_UNAVAILABLE_ERROR: &str =
    "UI injection remained unavailable within its readiness deadline";
const CDP_DEADLINE_EXPIRED_ERROR: &str = "CDP injection deadline expired";
const TARGET_CRASHED_ERROR: &str = "CDP target crashed";
pub const OFFICIAL_NEW_CODEX_URL: &str = "codex://new?mode=codex";

#[derive(Debug, Clone)]
pub struct CdpTarget {
    pub id: String,
    pub r#type: String,
    pub url: String,
    pub ws: String,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum CdpWindowKind {
    #[default]
    Incognito,
    Normal,
}

#[derive(Debug, Clone, Default)]
pub struct InjectionOptions {
    pub window_kind: CdpWindowKind,
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
    require_codex_mode: bool,
}

#[derive(Default)]
pub(crate) struct InjectionAttemptState {
    pub(crate) readiness: CodexModeReadiness,
    registered_script_targets: HashSet<String>,
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
        ..InjectionOptions::default()
    })
}

pub fn inject_source_for_options(options: &InjectionOptions) -> String {
    inject_source_for_options_with_runtime(options, INJECT_JS)
}

pub(crate) fn inject_source_for_options_with_runtime(
    options: &InjectionOptions,
    runtime_source: &str,
) -> String {
    let incognito = match options.window_kind {
        CdpWindowKind::Incognito => "true",
        CdpWindowKind::Normal => "false",
    };
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
        "window.__incodexIncognito={incognito};window.__incodexLocale={locale};{platform}window.__incodexProfileMask={profile_bootstrap};\n{runtime_source}"
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
    let base = match options.window_kind {
        CdpWindowKind::Incognito => ui_ready_expression().to_string(),
        CdpWindowKind::Normal => {
            "(() => ({button: Boolean(document.querySelector('[data-incodex-privacy-toggle]')), banner: true}))()".to_string()
        }
    };
    if options.profile_mask.is_none() {
        return base;
    }
    format!(
        "(() => {{ const base = {base}; base.profileMask = window.__incodexProfileMaskHealth === true; return base; }})()",
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
    let _banner = object
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

    // 原生握手证明 Runtime 控件和必要的身份遮罩就绪；说明条状态单独保留，
    // 不以说明条渲染延迟否定已经安全创建的窗口。
    if button {
        Ok(())
    } else {
        Err("Incodex button is not mounted yet".into())
    }
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

#[cfg(not(target_os = "windows"))]
#[path = "cdp_masked_lifecycle.rs"]
mod masked_lifecycle;
