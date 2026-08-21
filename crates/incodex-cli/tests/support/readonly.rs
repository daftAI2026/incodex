#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static HOME_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_incodex")
}

pub fn isolated_home() -> PathBuf {
    let sequence = HOME_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("incodex-ro-{}-{n}-{sequence}", std::process::id()));
    fs::create_dir_all(&dir).expect("home");
    dir
}

pub fn run(args: &[&str], home: &std::path::Path) -> (i32, String, String) {
    let output = Command::new(bin())
        .args(args)
        .env("HOME", home)
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .env("SHELL", "/bin/zsh")
        .output()
        .expect("spawn incodex");
    (
        output.status.code().unwrap_or(1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

pub fn run_with_stdout_redirected(args: &[&str], home: &std::path::Path) -> (i32, String, String) {
    let _pty_gate = crate::support::tty::acquire();
    let stdout_path = home.join("redirected.out");
    let script = r#"
import os, pty, select, sys, time
program, home, stdout_path, *args = sys.argv[1:]
env = os.environ.copy()
env["HOME"] = home
env["TERM"] = "xterm-256color"
env["NO_COLOR"] = "1"
pid, fd = pty.fork()
if pid == 0:
    output = os.open(stdout_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    os.dup2(output, 1)
    os.close(output)
    os.execvpe(program, [program, *args], env)
buf = bytearray()
deadline = time.time() + 5
status = 1
while time.time() < deadline:
    ready, _, _ = select.select([fd], [], [], 0.1)
    if ready:
        try:
            chunk = os.read(fd, 8192)
        except OSError:
            chunk = b""
        if chunk:
            buf.extend(chunk)
    done, wait_status = os.waitpid(pid, os.WNOHANG)
    if done == pid:
        status = os.waitstatus_to_exitcode(wait_status)
        break
else:
    os.kill(pid, 9)
    os.waitpid(pid, 0)
    status = 124
sys.stdout.buffer.write(("STATUS %d\n" % status).encode())
sys.stdout.buffer.write(bytes(buf))
"#;
    let output = Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(bin())
        .arg(home)
        .arg(&stdout_path)
        .args(args)
        .output()
        .expect("spawn redirected PTY harness");
    let raw = String::from_utf8_lossy(&output.stdout).into_owned();
    let (status, stderr) = raw.split_once('\n').unwrap_or((&raw, ""));
    (
        status
            .strip_prefix("STATUS ")
            .and_then(|value| value.parse().ok())
            .unwrap_or(1),
        fs::read_to_string(stdout_path).unwrap_or_default(),
        stderr.to_string(),
    )
}

pub fn parse_json(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout.trim_end()).expect("json")
}

pub fn top_level_json_keys(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("  \"")?;
            let (key, _) = rest.split_once("\":")?;
            Some(key)
        })
        .collect()
}

pub const DIAGNOSIS_KEYS: &[&str] = &[
    "target",
    "targetId",
    "exists",
    "patched",
    "bundleId",
    "appVersion",
    "appBuild",
    "architecture",
    "asarFileHash",
    "asarHeaderHash",
    "plistFileHash",
    "plistIntegrityHash",
    "runtimeVersion",
    "originalMain",
    "codesignOk",
    "backup",
    "stalePid",
    "orphanSessions",
    "leftoverChromium",
    "asarLoaderOnly",
    "externalRuntime",
    "signing",
    "spctl",
    "interruptedTransactions",
    "journalRecords",
    "checks",
    "findings",
];
