/*
 * [INPUT]: 依赖固定系统命令/已登记 native store 的 argv、超时与输出上限。
 * [OUTPUT]: 提供不继承 stdin、不泄露 stderr、期限内终止进程组的签名工具执行。
 * [POS]: 本机签名资产的执行隔离层；不解析秘密内容、不修改 identity metadata。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::{NATIVE_STORE_TIMEOUT, OPENSSL_TIMEOUT};

const MAX_HELPER_OUTPUT_BYTES: usize = 128;
const POLL_INTERVAL: Duration = Duration::from_millis(20);

pub(super) fn run_openssl(command: &mut Command, operation: &str) -> Result<(), String> {
    run_bounded(command, OPENSSL_TIMEOUT, 0, operation).map(|_| ())
}

pub(super) fn run_native_store(
    path: &Path,
    arguments: &[&str],
    expected_output: &[u8],
) -> Result<(), String> {
    let mut command = Command::new(path);
    command.args(arguments);
    let output = run_bounded(
        &mut command,
        NATIVE_STORE_TIMEOUT,
        MAX_HELPER_OUTPUT_BYTES,
        "run private macOS signing identity store",
    )?;
    if output != expected_output {
        return Err("private macOS signing identity store returned unexpected output".into());
    }
    Ok(())
}

pub(super) fn run_bounded(
    command: &mut Command,
    timeout: Duration,
    maximum_output: usize,
    operation: &str,
) -> Result<Vec<u8>, String> {
    command
        .stdin(Stdio::null())
        .stdout(if maximum_output == 0 {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = command
        .spawn()
        .map_err(|_| format!("cannot start {operation}"))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(POLL_INTERVAL.min(timeout.saturating_sub(started.elapsed())));
            }
            Ok(None) => {
                terminate_child(&mut child);
                return Err(format!("{operation} timed out"));
            }
            Err(_) => {
                terminate_child(&mut child);
                return Err(format!("cannot wait for {operation}"));
            }
        }
    };
    let mut output = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        stdout
            .take(maximum_output.saturating_add(1) as u64)
            .read_to_end(&mut output)
            .map_err(|_| format!("cannot read {operation} result"))?;
    }
    if output.len() > maximum_output {
        return Err(format!("{operation} exceeded its output bound"));
    }
    if !status.success() {
        return Err(match status.code() {
            Some(code) => format!("{operation} failed with status {code}"),
            None => format!("{operation} terminated by signal"),
        });
    }
    Ok(output)
}

fn terminate_child(child: &mut Child) {
    let pid = child.id() as i32;
    if pid > 0 {
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}
