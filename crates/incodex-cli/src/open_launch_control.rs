/**
 * [INPUT]: 依赖 launch 子进程 stdin、受管 CDP 页面与精确宿主 PID。
 * [OUTPUT]: 提供闭集 raise 请求、页面健康校验及系统置前确认。
 * [POS]: pinned Runtime 普通宿主 open 的私有控制端；不创建服务或新 session。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::io::{self, IsTerminal, Read, Write};
use std::net::TcpStream;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use serde_json::{json, Value};

use crate::cdp::{
    connect_cdp_websocket, is_primary_codex_page, list_targets, send_guarded_cdp, CdpTarget,
};
use crate::windows_installed_native_open::{
    NATIVE_OPEN_CONTROL_READY, NATIVE_OPEN_RAISED, NATIVE_OPEN_RAISE_FAILED,
};

const RAISE_REQUEST: &[u8] = b"raise\n";
const MAX_CONTROL_LINE: usize = 64;

/// Start the private stdin protocol only after open_command verified the pinned Runtime.
pub(crate) fn start_if_authorized(
    pinned_runtime_verified: bool,
    debug_port: u16,
    child_pid: u32,
    require_profile_mask: bool,
    process_alive: Arc<AtomicBool>,
    ui_ready: Arc<AtomicBool>,
    activate_pid: Arc<dyn Fn(u32) -> Result<(), String> + Send + Sync>,
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
                    raise_private_page(
                        debug_port,
                        child_pid,
                        require_profile_mask,
                        &process_alive,
                        activate_pid.as_ref(),
                    )
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

fn bring_to_front_command() -> (&'static str, Value) {
    ("Page.bringToFront", json!({}))
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

fn activation_command(pid: u32) -> Result<Command, String> {
    if pid == 0 {
        return Err("private open child PID is invalid".into());
    }
    let script = format!(
        "tell application \"System Events\" to set frontmost of (first process whose unix id is {pid}) to true"
    );
    let mut command = Command::new("/usr/bin/osascript");
    command.arg("-e").arg(script);
    Ok(command)
}

pub(crate) fn activate_exact_pid(pid: u32) -> Result<(), String> {
    let output = activation_command(pid)?
        .output()
        .map_err(|error| format!("cannot activate private Codex process {pid}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cannot activate private Codex process {pid}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

fn confirm_window_raise(
    page_raise: impl FnOnce() -> Result<(), String>,
    pid: u32,
    activate_pid: impl FnOnce(u32) -> Result<(), String>,
) -> Result<(), String> {
    page_raise()?;
    activate_pid(pid)
}

fn raise_private_page(
    debug_port: u16,
    child_pid: u32,
    require_profile_mask: bool,
    process_alive: &AtomicBool,
    activate_pid: &dyn Fn(u32) -> Result<(), String>,
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
    let (method, params) = bring_to_front_command();
    confirm_window_raise(
        || send_guarded_cdp(&mut socket, 2, method, params, &owner_guard).map(|_| ()),
        child_pid,
        |pid| {
            if !process_alive.load(Ordering::Acquire) {
                return Err("private open owner exited".into());
            }
            activate_pid(pid)
        },
    )?;
    if !process_alive.load(Ordering::Acquire) {
        return Err("private open owner exited".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        activation_command, bring_to_front_command, confirm_window_raise, is_raise_line,
        read_control_line, start_if_authorized, unique_primary_target,
    };
    use crate::cdp::CdpTarget;
    use std::cell::RefCell;
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
    fn raise_uses_the_page_bring_to_front_command() {
        let (method, params) = bring_to_front_command();

        assert_eq!(method, "Page.bringToFront");
        assert!(params.as_object().is_some_and(|value| value.is_empty()));
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
    fn activation_targets_the_guardians_exact_child_pid() {
        let command = activation_command(31_337).unwrap();
        let arguments = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(
            command.get_program().to_string_lossy(),
            "/usr/bin/osascript"
        );
        assert_eq!(
            arguments,
            [
                "-e",
                "tell application \"System Events\" to set frontmost of (first process whose unix id is 31337) to true"
            ]
        );
        assert!(activation_command(0).is_err());
    }

    #[test]
    fn raise_requires_page_ack_then_os_activation_ack() {
        let calls = RefCell::new(Vec::new());
        confirm_window_raise(
            || {
                calls.borrow_mut().push("page");
                Ok(())
            },
            31_337,
            |pid| {
                assert_eq!(pid, 31_337);
                calls.borrow_mut().push("system");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*calls.borrow(), ["page", "system"]);

        calls.borrow_mut().clear();
        assert!(confirm_window_raise(
            || Err("CDP ACK missing".into()),
            31_337,
            |_| {
                calls.borrow_mut().push("system");
                Ok(())
            },
        )
        .is_err());
        assert!(calls.borrow().is_empty());

        assert_eq!(
            confirm_window_raise(|| Ok(()), 31_337, |_| Err("System Events denied".into()))
                .unwrap_err(),
            "System Events denied"
        );
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
            Arc::new(|_| Ok(())),
        )
        .unwrap();

        assert!(worker.is_none());
    }
}
