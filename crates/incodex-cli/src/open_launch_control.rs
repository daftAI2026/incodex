/**
 * [INPUT]: 依赖子进程状态、loopback CDP目标与共享官方窗口焦点ACK。
 * [OUTPUT]: 提供闭集 raise、真实焦点回执、窗口几何与按需 primary-target 快照。
 * [POS]: open的私有CDP控制端；空目标确认仅由固定窗口空查询惰性触发。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::io::{self, IsTerminal, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use serde_json::{json, Value};

use crate::cdp::{
    connect_cdp_websocket, focus_official_window, is_primary_codex_page, list_targets,
    send_guarded_cdp, CdpTarget,
};
use crate::windows_installed_native_open::{
    NATIVE_OPEN_CONTROL_READY, NATIVE_OPEN_RAISED, NATIVE_OPEN_RAISE_FAILED,
};

const RAISE_REQUEST: &[u8] = b"raise\n";
const MAX_CONTROL_LINE: usize = 64;
const PRIMARY_BOUNDS_EXPRESSION: &str = r#"(() => {
  if (window !== window.top || location.href !== 'app://-/index.html' ||
      document.querySelectorAll('[data-incodex-privacy-toggle]').length !== 1) return null;
  const bounds = [window.screenX, window.screenY, window.outerWidth, window.outerHeight];
  return bounds.every(Number.isSafeInteger) ? bounds : null;
})()"#;
/// Start the private stdin protocol only after open_command verified the pinned Runtime.
pub(crate) fn start_if_authorized(
    pinned_runtime_verified: bool,
    debug_port: u16,
    child_pid: u32,
    require_profile_mask: bool,
    process_alive: Arc<AtomicBool>,
    ui_ready: Arc<AtomicBool>,
) -> Result<Option<JoinHandle<()>>, String> {
    if !pinned_runtime_verified || debug_port == 0 {
        return Ok(None);
    }
    if io::stdin().is_terminal() || io::stdout().is_terminal() {
        return Ok(None);
    }
    if child_pid == 0 {
        return Err("private open child PID is invalid".into());
    }
    thread::Builder::new()
        .name("incodex-open-raise-control".into())
        .spawn(move || {
            if write_response(NATIVE_OPEN_CONTROL_READY).is_err() {
                return;
            }
            let stdin = io::stdin();
            let mut input = stdin.lock();
            while process_alive.load(Ordering::Acquire) {
                let accepted = match read_control_line(&mut input) {
                    Ok(Some(accepted)) => accepted,
                    Ok(None) | Err(_) => return,
                };
                if !accepted {
                    continue;
                }
                let result = if !process_alive.load(Ordering::Acquire) {
                    Err("private open owner exited".into())
                } else if !ui_ready.load(Ordering::Acquire) {
                    Err("private window has not passed UI readiness".into())
                } else {
                    raise_private_page(debug_port, require_profile_mask, &process_alive)
                };
                let response = if result.is_ok() {
                    NATIVE_OPEN_RAISED
                } else {
                    NATIVE_OPEN_RAISE_FAILED
                };
                if write_response(response).is_err() {
                    return;
                }
            }
        })
        .map(Some)
        .map_err(|error| format!("cannot start private open raise control: {error}"))
}

fn read_control_line(reader: &mut impl Read) -> io::Result<Option<bool>> {
    let mut line = Vec::with_capacity(MAX_CONTROL_LINE);
    let mut overlong = false;
    let mut byte = [0_u8; 1];
    loop {
        let read = match reader.read(&mut byte) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        match read {
            0 if line.is_empty() && !overlong => return Ok(None),
            0 => return Ok(Some(false)),
            _ if byte[0] == b'\n' => {
                if !overlong {
                    line.push(b'\n');
                }
                return Ok(Some(!overlong && is_raise_line(&line)));
            }
            _ => {
                if line.len() < MAX_CONTROL_LINE {
                    line.push(byte[0]);
                } else {
                    overlong = true;
                }
            }
        }
    }
}

fn write_response(response: &str) -> io::Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    writeln!(output, "{response}")?;
    output.flush()
}

fn is_raise_line(line: &[u8]) -> bool {
    line == RAISE_REQUEST
}

fn unique_primary_target(targets: &[CdpTarget]) -> Result<&CdpTarget, String> {
    let mut primary = targets
        .iter()
        .filter(|target| is_primary_codex_page(target));
    let target = primary.next().ok_or("no private Codex page target")?;
    if primary.next().is_some() {
        return Err("private Codex page target is ambiguous".into());
    }
    Ok(target)
}

pub(super) fn primary_window_bounds(
    debug_port: u16,
    process_alive: &AtomicBool,
) -> Result<incodex_macos::WindowBounds, String> {
    let targets = list_targets(debug_port)?;
    let page = unique_primary_target(&targets)?;
    let mut socket = connect_cdp_websocket(&page.ws, debug_port)?;
    let reply = send_guarded_cdp(
        &mut socket,
        1,
        "Runtime.evaluate",
        json!({"expression": PRIMARY_BOUNDS_EXPRESSION, "returnByValue": true}),
        &|_: &TcpStream| ensure_owner_alive(process_alive),
    )?;
    ensure_owner_alive(process_alive)?;
    parse_primary_bounds(&reply).ok_or("private window bounds are not ready".into())
}

/// 固定窗口无WindowServer行时才读取新鲜primary目标快照。
pub(super) fn primary_page_absent(
    debug_port: u16,
    process_alive: &AtomicBool,
) -> Result<bool, String> {
    primary_page_absent_from_targets(process_alive, || list_targets(debug_port))
}

fn primary_page_absent_from_targets(
    process_alive: &AtomicBool,
    snapshot: impl FnOnce() -> Result<Vec<CdpTarget>, String>,
) -> Result<bool, String> {
    ensure_owner_alive(process_alive)?;
    let targets = snapshot();
    ensure_owner_alive(process_alive)?;
    Ok(!targets?.iter().any(is_primary_codex_page))
}

fn parse_primary_bounds(reply: &Value) -> Option<incodex_macos::WindowBounds> {
    let values = reply.pointer("/result/result/value")?.as_array()?;
    let [x, y, width, height] = values.as_slice() else {
        return None;
    };
    let integer = |v: &Value| i32::try_from(v.as_i64()?).ok();
    let bounds = incodex_macos::WindowBounds {
        x: integer(x)?,
        y: integer(y)?,
        width: integer(width)?,
        height: integer(height)?,
    };
    (bounds.width > 0 && bounds.height > 0).then_some(bounds)
}

fn ensure_owner_alive(process_alive: &AtomicBool) -> Result<(), String> {
    if process_alive.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err("private open owner exited".into())
    }
}

fn raise_private_page(
    debug_port: u16,
    require_profile_mask: bool,
    process_alive: &AtomicBool,
) -> Result<(), String> {
    if !process_alive.load(Ordering::Acquire) {
        return Err("private open owner exited".into());
    }
    let targets = list_targets(debug_port)?;
    let page = unique_primary_target(&targets)?;
    let mut socket = connect_cdp_websocket(&page.ws, debug_port)?;
    let owner_guard = |_: &TcpStream| {
        if process_alive.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err("private open owner exited".into())
        }
    };
    if require_profile_mask {
        let health = send_guarded_cdp(
            &mut socket,
            1,
            "Runtime.evaluate",
            json!({
                "expression": "window.__incodexRefreshProfileMaskHealth?.() === true",
                "returnByValue": true
            }),
            &owner_guard,
        )?;
        if health
            .pointer("/result/result/value")
            .and_then(Value::as_bool)
            != Some(true)
        {
            return Err("private profile mask is not healthy".into());
        }
    }
    focus_official_window(&mut socket, 2, &owner_guard)?;
    ensure_owner_alive(process_alive)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        is_raise_line, parse_primary_bounds, primary_page_absent_from_targets, read_control_line,
        start_if_authorized, unique_primary_target,
    };
    use crate::cdp::CdpTarget;
    use std::io::Cursor;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    #[test]
    fn control_accepts_only_the_closed_raise_line() {
        for request in [
            b"".as_slice(),
            b"raise",
            b"raise\r\n",
            b"raise now\n",
            b"Raise\n",
        ] {
            assert!(!is_raise_line(request), "unexpected request: {request:?}");
        }
        assert!(is_raise_line(b"raise\n"));
    }

    #[test]
    fn control_reader_accepts_only_one_bounded_raise_line() {
        assert_eq!(
            read_control_line(&mut Cursor::new(b"raise\n")).unwrap(),
            Some(true)
        );
        assert_eq!(
            read_control_line(&mut Cursor::new(b"open\n")).unwrap(),
            Some(false)
        );
        assert_eq!(
            read_control_line(&mut Cursor::new(vec![b'x'; 1024])).unwrap(),
            Some(false)
        );
    }

    #[test]
    fn target_selection_rejects_missing_or_ambiguous_primary_pages() {
        let target = |id: &str, url: &str| CdpTarget {
            id: id.into(),
            r#type: "page".into(),
            url: url.into(),
            ws: format!("ws://127.0.0.1/devtools/{id}"),
        };
        let primary = target("primary", "app://-/index.html");
        let unrelated = target("other", "https://example.test/");

        assert!(unique_primary_target(std::slice::from_ref(&unrelated)).is_err());
        assert_eq!(
            unique_primary_target(std::slice::from_ref(&primary))
                .unwrap()
                .id
                .as_str(),
            "primary"
        );
        assert!(unique_primary_target(&[
            primary.clone(),
            target("duplicate", "app://-/index.html")
        ])
        .is_err());
    }

    #[test]
    fn close_confirmation_treats_any_primary_target_as_present() {
        let target = |id: &str, url: &str| CdpTarget {
            id: id.into(),
            r#type: "page".into(),
            url: url.into(),
            ws: format!("ws://127.0.0.1/devtools/{id}"),
        };
        let alive = AtomicBool::new(true);
        let replacement = target("replacement", "app://-/index.html");
        let non_primary = target("other", "https://example.test/");

        assert!(primary_page_absent_from_targets(&alive, || Ok(vec![])).unwrap());
        assert!(
            primary_page_absent_from_targets(&alive, || Ok(vec![non_primary.clone()])).unwrap()
        );
        assert!(
            !primary_page_absent_from_targets(&alive, || Ok(vec![replacement.clone()])).unwrap()
        );
        assert!(!primary_page_absent_from_targets(&alive, || Ok(vec![
            replacement.clone(),
            replacement.clone()
        ]))
        .unwrap());
    }

    #[test]
    fn close_confirmation_checks_owner_before_and_after_target_snapshot() {
        let alive = AtomicBool::new(false);
        let mut called = false;
        assert!(primary_page_absent_from_targets(&alive, || {
            called = true;
            Ok(vec![])
        })
        .is_err());
        assert!(!called, "dead owners must not trigger a CDP request");

        alive.store(true, std::sync::atomic::Ordering::Release);
        assert!(primary_page_absent_from_targets(&alive, || {
            alive.store(false, std::sync::atomic::Ordering::Release);
            Ok(vec![])
        })
        .is_err());
    }

    #[test]
    fn close_confirmation_preserves_target_query_errors() {
        let alive = AtomicBool::new(true);
        assert!(
            primary_page_absent_from_targets(&alive, || Err("target query failed".into()))
                .unwrap_err()
                .contains("target query failed")
        );
    }

    #[test]
    fn initial_window_bounds_require_exact_positive_integer_geometry() {
        let parse = |v| parse_primary_bounds(&serde_json::json!({"result":{"result":{"value":v}}}));
        assert_eq!(
            parse(serde_json::json!([-1200, 20, 320, 212])).unwrap(),
            incodex_macos::WindowBounds {
                x: -1200,
                y: 20,
                width: 320,
                height: 212
            }
        );
        for value in [
            serde_json::json!([0, 0, 0, 100]),
            serde_json::json!([0, 0, 100, -1]),
            serde_json::json!([0.5, 0, 100, 100]),
            serde_json::json!([0, 0, 100]),
            serde_json::json!([0, 0, 2147483648_i64, 100]),
            serde_json::Value::Null,
        ] {
            assert!(parse(value).is_none());
        }
    }

    #[test]
    fn unpinned_launch_does_not_start_a_stdin_reader() {
        let worker = start_if_authorized(
            false,
            1234,
            31_337,
            false,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();

        assert!(worker.is_none());
    }
}
