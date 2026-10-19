/*
 * [INPUT]: 依赖已验证宿主连接、共享动作控制器和Rust原生open生命周期。
 * [OUTPUT]: 将普通页面的帽子请求转交既有open，并执行双ACK代际更新。
 * [POS]: launch的薄CDP适配；不创建会话，不改官方主进程或退出宿主。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use crate::cdp::{connect_cdp_websocket, is_primary_codex_page, list_targets, send_guarded_cdp};
use crate::macos_launch_process::NormalHost;
use crate::macos_launch_runtime::LaunchRuntime;
use crate::runtime_ui_update::UiGeneration;
use crate::windows_installed_native_open::{
    launch_native_open_for_app, native_open_bridge_response, validate_native_open_request,
    NativeOpenBridgeRequest, NativeOpenOutcome, NativeOpenState,
};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::net::TcpStream;
use std::path::Path;
use std::process::Child;
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

const BINDING: &str = "__incodexNativeAction";
const CONTEXT: &str = "window === window.top && window.location.href === 'app://-/index.html'";
const STARTUP: Duration = Duration::from_secs(60);
const UI_ACK: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(100);

// ---- 只重试未确认的文档或提交；不重试原生迁移。 ----
fn refresh_retry_delay(status: &Value, retries: usize) -> Option<Duration> {
    let pending_publication =
        status["published"].is_object() && status["published"] != status["activeUi"];
    let unconfirmed = status["phase"] == "rollback-failed"
        || status["rendererAckId"].is_null()
        || status["actionAckId"].is_null()
        || pending_publication;
    if status["restartRequired"] == true || !unconfirmed {
        return None;
    }
    [250, 1_000, 2_000]
        .get(retries)
        .copied()
        .map(Duration::from_millis)
}

struct Page {
    id: String,
    ws: String,
    events: WebSocket<TcpStream>,
}
struct Pending {
    page: String,
    request: NativeOpenBridgeRequest,
}

// ---- 共享控制器仅依赖transport；业务factory来自验证后的Runtime。 ----
fn bridge_source() -> String {
    let controller = include_str!("../assets/incodex-windows-actions.cjs");
    format!(
        r#"(() => {{
      if (!({CONTEXT})) return;
      const pending = window.__incodexNativeActionPending || new Map();
      window.__incodexNativeActionPending = pending;
      window.__incodexResolveNativeAction = response => {{
        const resolve = pending.get(response?.requestId);
        if (resolve) {{ pending.delete(response.requestId); resolve(response); }}
      }};
      const nativeOpen = (payload, runtimeRelease) => new Promise(resolve => {{
        pending.set(payload.requestId, resolve);
        const bounds = [window.screenX,window.screenY,window.outerWidth,window.outerHeight];
        const sourceBounds = bounds.every(Number.isSafeInteger) ? bounds.join(',') : undefined;
        window.{BINDING}(JSON.stringify({{...payload,sourceBounds,runtimeRelease}}));
      }});
      const createController = (() => {{ const module = {{exports:{{}}}}; const exports=module.exports;
        {controller}
        return module.exports.createNativeActionController;
      }})();
      const actions = window.__incodexNativeActions || createController(nativeOpen);
      window.__incodexNativeActions=actions;
      window.incodex=window.incodex || {{}};
      window.incodex.requestIncognitoAction=payload=>actions.request(payload);
    }})()"#
    )
}
fn generation_expression(generation: &UiGeneration, commit: bool) -> Result<String, String> {
    let injector = generation
        .files
        .get("incodex-inject.js")
        .ok_or("missing injector hash")?;
    let action = generation
        .files
        .get("incodex-main-actions.cjs")
        .ok_or("missing action hash")?;
    let id = serde_json::to_string(injector).map_err(|e| e.to_string())?;
    let action_id = serde_json::to_string(action).map_err(|e| e.to_string())?;
    let release = serde_json::to_string(&generation.release).map_err(|e| e.to_string())?;
    if commit {
        return Ok(format!(
            r#"(() => {{
          if (!({CONTEXT})) return null;
          return {{actions:window.__incodexNativeActions?.commitStaged({action_id})}};
        }})()"#
        ));
    }
    Ok(format!(
        r#"(() => {{
      if (!({CONTEXT})) return null;
      {};
      const controller=window.__incodexNativeActions;
      const factory=(()=>{{const module={{exports:{{}}}};const exports=module.exports;
        {};return module.exports.createMainActions;}})();
      controller.stage(controller.prepare(factory,{action_id},{release}));
      window.__incodexIncognito=false;
      window.__incodexRendererRequest={{protocol:1,id:{id}}};
      {};
      return {{actions:controller.stagedGeneration()}};
    }})()"#,
        bridge_source(),
        generation.action_source,
        generation.source
    ))
}
fn value(response: &Value) -> &Value {
    &response["result"]["result"]["value"]
}
fn evaluate(
    host: &NormalHost,
    port: u16,
    ws: &str,
    expression: String,
    context: Option<u64>,
) -> Result<Value, String> {
    let mut socket = connect_cdp_websocket(ws, port)?;
    let mut params = json!({"expression":expression,"returnByValue":true});
    if let Some(context) = context {
        params["contextId"] = json!(context);
    }
    let response = send_guarded_cdp(&mut socket, 1, "Runtime.evaluate", params, &|s| {
        host.require_connection_owner(s)
    })?;
    if response.pointer("/result/exceptionDetails").is_some() {
        return Err("CDP action evaluation failed".into());
    }
    Ok(response)
}
fn apply_generation(
    host: &NormalHost,
    port: u16,
    pages: &[(&str, &str)],
    generation: &UiGeneration,
    commit: bool,
) -> Result<bool, String> {
    if pages.is_empty() {
        return Ok(false);
    }
    for (_, ws) in pages {
        let response = evaluate(
            host,
            port,
            ws,
            generation_expression(generation, commit)?,
            None,
        )?;
        let action = &generation.files["incodex-main-actions.cjs"];
        if value(&response)["actions"]["protocol"] != 1
            || value(&response)["actions"]["id"] != *action
        {
            return Ok(false);
        }
        if !commit {
            let until = Instant::now() + UI_ACK;
            let injector = &generation.files["incodex-inject.js"];
            loop {
                let response = evaluate(
                    host,
                    port,
                    ws,
                    "(() => ({ui:window.__incodexRendererGeneration,button:Boolean(document.querySelector('[data-incodex-privacy-toggle]'))}))()".into(),
                    None,
                )?;
                let probe = value(&response);
                if probe["ui"]["protocol"] == 1
                    && probe["ui"]["id"] == *injector
                    && probe["ui"]["restartRequired"] == false
                    && probe["button"] == true
                {
                    break;
                }
                if Instant::now() >= until {
                    return Ok(false);
                }
                thread::sleep(POLL);
            }
        }
    }
    Ok(true)
}
fn connect_page(host: &NormalHost, port: u16, id: String, ws: String) -> Result<Page, String> {
    let mut events = connect_cdp_websocket(&ws, port)?;
    let guard = |s: &TcpStream| host.require_connection_owner(s);
    send_guarded_cdp(&mut events, 1, "Runtime.enable", json!({}), &guard)?;
    send_guarded_cdp(
        &mut events,
        2,
        "Runtime.addBinding",
        json!({"name":BINDING}),
        &guard,
    )?;
    events
        .get_mut()
        .set_nonblocking(true)
        .map_err(|e| e.to_string())?;
    Ok(Page { id, ws, events })
}
fn request_from_event(message: &Value) -> Option<NativeOpenBridgeRequest> {
    if message["method"] != "Runtime.bindingCalled" || message["params"]["name"] != BINDING {
        return None;
    }
    let payload: Value = serde_json::from_str(message["params"]["payload"].as_str()?).ok()?;
    if payload["action"] != "open" {
        return None;
    }
    let id = payload["requestId"].as_str()?;
    if !(8..=96).contains(&id.len())
        || !id.starts_with("incodex-")
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return None;
    }
    let release = match payload.get("runtimeRelease") {
        None => None,
        Some(Value::String(s)) if s.len() <= 256 => Some(s.clone()),
        _ => return None,
    };
    let bounds = payload["sourceBounds"]
        .as_str()
        .filter(|s| {
            let parts = s.split(',').collect::<Vec<_>>();
            parts.len() == 4
                && parts.iter().all(|p| p.parse::<i32>().is_ok())
                && parts[2].parse::<i32>().is_ok_and(|n| n > 0)
                && parts[3].parse::<i32>().is_ok_and(|n| n > 0)
        })
        .map(str::to_owned);
    Some(NativeOpenBridgeRequest {
        request_id: id.into(),
        execution_context_id: message["params"]["executionContextId"].as_u64()?,
        source_bounds: bounds,
        runtime_release: release,
    })
}
fn resolve(
    host: &NormalHost,
    port: u16,
    pages: &HashMap<String, Page>,
    pending: &mut VecDeque<Pending>,
    outcome: &NativeOpenOutcome,
) -> Result<(), String> {
    if matches!(outcome, NativeOpenOutcome::Pending) {
        return Ok(());
    }
    while let Some(p) = pending.pop_front() {
        let Some(page) = pages.get(&p.page) else {
            continue;
        };
        // 文档已销毁时丢弃旧回执，不在新页面伪造成功。
        let context = match evaluate(
            host,
            port,
            &page.ws,
            CONTEXT.into(),
            Some(p.request.execution_context_id),
        ) {
            Ok(v) => value(&v) == true,
            Err(e) if e.contains("Cannot find context") => continue,
            Err(e) => return Err(e),
        };
        if let Some(response) = native_open_bridge_response(&p.request, outcome, context) {
            evaluate(
                host,
                port,
                &page.ws,
                format!("window.__incodexResolveNativeAction?.({response})"),
                Some(p.request.execution_context_id),
            )?;
        }
    }
    Ok(())
}
fn child_alive(child: &mut Child) -> Result<bool, String> {
    child
        .try_wait()
        .map(|v| v.is_none())
        .map_err(|e| e.to_string())
}

pub(crate) fn supervise(
    host: &NormalHost,
    port: u16,
    app: &Path,
    cli: &Path,
    runtime: &mut LaunchRuntime,
    mut ready: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let startup = Instant::now() + STARTUP;
    let mut pages: HashMap<String, Page> = HashMap::new();
    let mut pending = VecDeque::new();
    let mut native = NativeOpenState::<Child>::default();
    let mut initial = true;
    let mut selection_changed = true;
    let mut last_error = None;
    let mut next_discovery = Instant::now();
    let mut retries = 0;
    let mut retry_at = Instant::now();
    while host.is_alive()? {
        // 只读target发现低频执行；有权限的CDP命令仍逐连接证明owner。
        if Instant::now() >= next_discovery {
            next_discovery = Instant::now() + Duration::from_secs(1);
            let targets = match list_targets(port) {
                Ok(targets) => targets,
                Err(_) if !host.is_alive()? => return Ok(()),
                Err(_) if initial && Instant::now() < startup => {
                    thread::sleep(POLL);
                    continue;
                }
                Err(error) => return Err(error),
            };
            let ids = targets
                .iter()
                .filter(|t| is_primary_codex_page(t))
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>();
            let previous_count = pages.len();
            pages.retain(|id, _| ids.contains(&id.as_str()));
            if pages.len() != previous_count {
                selection_changed = true;
                retries = 0;
                retry_at = Instant::now();
            }
            pending.retain(|p: &Pending| pages.contains_key(&p.page));
            for target in targets.into_iter().filter(is_primary_codex_page) {
                if let std::collections::hash_map::Entry::Vacant(entry) = pages.entry(target.id) {
                    let page = connect_page(host, port, entry.key().clone(), target.ws)?;
                    entry.insert(page);
                    runtime.renderer_invalidated();
                    selection_changed = true;
                    retries = 0;
                    retry_at = Instant::now();
                }
            }
        }
        let mut messages = Vec::new();
        let mut closed_pages = Vec::new();
        for page in pages.values_mut() {
            for _ in 0..64 {
                match page.events.read() {
                    Ok(Message::Text(text)) => {
                        let message: Value =
                            serde_json::from_str(&text).map_err(|_| "malformed CDP event")?;
                        if message["method"] == "Runtime.executionContextsCleared" {
                            pending.retain(|p: &Pending| p.page != page.id);
                            runtime.renderer_invalidated();
                            selection_changed = true;
                            retries = 0;
                            retry_at = Instant::now();
                        }
                        if let Some(request) = request_from_event(&message) {
                            messages.push((page.id.clone(), request));
                        }
                    }
                    Ok(Message::Ping(bytes)) => {
                        page.events
                            .send(Message::Pong(bytes))
                            .map_err(|e| e.to_string())?;
                    }
                    Ok(Message::Close(_)) => {
                        closed_pages.push(page.id.clone());
                        next_discovery = Instant::now();
                        break;
                    }
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e))
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        break;
                    }
                    Err(e) => return Err(format!("launcher CDP event channel: {e}")),
                }
            }
        }
        if !closed_pages.is_empty() {
            selection_changed = true;
            retries = 0;
            retry_at = Instant::now();
        }
        for id in closed_pages {
            pages.remove(&id);
        }
        pending.retain(|p| pages.contains_key(&p.page));
        if selection_changed && !pages.is_empty() && Instant::now() >= retry_at {
            let targets = pages
                .values()
                .map(|p| (p.id.as_str(), p.ws.as_str()))
                .collect::<Vec<_>>();
            match runtime.refresh(
                |g| apply_generation(host, port, &targets, g, false),
                |g| apply_generation(host, port, &targets, g, true),
            ) {
                Ok(()) => {
                    selection_changed = false;
                    retries = 0;
                    last_error = None;
                    if initial {
                        ready()?;
                        initial = false;
                    }
                }
                Err(error) => {
                    let status = runtime.status();
                    if last_error.as_ref() != Some(&error) {
                        eprintln!(
                            "Runtime retained ({}): {error}",
                            status["phase"].as_str().unwrap_or("unknown")
                        );
                        last_error = Some(error);
                    }
                    selection_changed = match refresh_retry_delay(&status, retries) {
                        Some(delay) => {
                            retries += 1;
                            retry_at = Instant::now() + delay;
                            true
                        }
                        None => false,
                    };
                }
            }
        }
        if initial && Instant::now() >= startup {
            return Err(
                "normal window CDP UI did not become ready; official app left running".into(),
            );
        }
        if let Some(outcome) = native.poll(child_alive) {
            resolve(host, port, &pages, &mut pending, &outcome)?;
        }
        for (page_id, request) in messages {
            let Some(page) = pages.get(&page_id) else {
                continue;
            };
            let context = evaluate(
                host,
                port,
                &page.ws,
                CONTEXT.into(),
                Some(request.execution_context_id),
            )?;
            if value(&context) != true {
                continue;
            }
            let request = match validate_native_open_request(request, |release| {
                runtime.validate_request_release(release)
            }) {
                Ok(request) => request,
                Err((request, error)) => {
                    let mut rejected = VecDeque::from([Pending {
                        page: page_id,
                        request,
                    }]);
                    resolve(
                        host,
                        port,
                        &pages,
                        &mut rejected,
                        &NativeOpenOutcome::Failure(error),
                    )?;
                    continue;
                }
            };
            let bounds = request.source_bounds.clone();
            pending.push_back(Pending {
                page: page_id,
                request,
            });
            let release = &runtime.active().release;
            let outcome = native.request(
                || launch_native_open_for_app(cli, bounds.as_deref(), app, release),
                child_alive,
            );
            resolve(host, port, &pages, &mut pending, &outcome)?;
        }
        if runtime.wait_for_change(POLL)? {
            selection_changed = true;
            retries = 0;
            retry_at = Instant::now();
        }
    }
    // native open保留自己的owner和清理；正常宿主退出不强杀私密窗口。
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_request_only_accepts_open_from_the_named_binding() {
        let event = json!({"method":"Runtime.bindingCalled","params":{"name":BINDING,"executionContextId":7,"payload":json!({"action":"open","requestId":"incodex-1234","sourceBounds":"1,2,800,600"}).to_string()}});
        let request = request_from_event(&event).unwrap();
        assert_eq!(request.execution_context_id, 7);
        assert_eq!(request.source_bounds.as_deref(), Some("1,2,800,600"));
        let mut wrong = event;
        wrong["params"]["name"] = json!("unrelated");
        assert!(request_from_event(&wrong).is_none());
    }
    #[test]
    fn bridge_reuses_the_shared_controller_and_not_session_code() {
        let source = bridge_source();
        assert!(source.contains("createNativeActionController"));
        assert!(source.contains("requestIncognitoAction"));
        assert!(!source.contains("create_session_home"));
    }

    #[test]
    fn new_document_and_failed_rollback_have_three_delayed_retries() {
        for status in [
            json!({"phase":"active","rendererAckId":null,"actionAckId":null}),
            json!({"phase":"rollback-failed","rendererAckId":"ui","actionAckId":"action"}),
        ] {
            assert_eq!(
                refresh_retry_delay(&status, 0),
                Some(Duration::from_millis(250))
            );
            assert_eq!(
                refresh_retry_delay(&status, 1),
                Some(Duration::from_secs(1))
            );
            assert_eq!(
                refresh_retry_delay(&status, 2),
                Some(Duration::from_secs(2))
            );
            assert_eq!(refresh_retry_delay(&status, 3), None);
        }
    }

    #[test]
    fn fully_acknowledged_runtime_and_native_migration_do_not_retry() {
        let mut status = json!({"phase":"active","published":{"release":"A"},"activeUi":{"release":"A"},"rendererAckId":"ui","actionAckId":"action"});
        assert_eq!(refresh_retry_delay(&status, 0), None);
        status["published"]["release"] = json!("B");
        assert!(refresh_retry_delay(&status, 0).is_some());
        status["restartRequired"] = json!(true);
        status["phase"] = json!("rollback-failed");
        assert_eq!(refresh_retry_delay(&status, 0), None);
    }
}

#[cfg(test)]
#[path = "macos_launch_bridge_tests.rs"]
mod transport_tests;
