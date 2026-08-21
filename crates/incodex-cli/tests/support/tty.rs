use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex, OnceLock};

static PTY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub struct Probe {
    start: Barrier,
    active: AtomicUsize,
    maximum: AtomicUsize,
}

impl Probe {
    pub fn new(contenders: usize) -> Arc<Self> {
        assert!(contenders > 0);
        Arc::new(Self {
            start: Barrier::new(contenders),
            active: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
        })
    }

    fn wait_for_start(&self) {
        self.start.wait();
    }

    fn enter(&self) -> ProbeGuard<'_> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(active, Ordering::SeqCst);
        ProbeGuard { probe: self }
    }

    pub fn max_concurrency(&self) -> usize {
        self.maximum.load(Ordering::SeqCst)
    }
}

struct ProbeGuard<'a> {
    probe: &'a Probe,
}

impl Drop for ProbeGuard<'_> {
    fn drop(&mut self) {
        self.probe.active.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Debug)]
pub struct Result {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

#[allow(dead_code)]
pub fn run(
    program: &str,
    prefix: &[&str],
    args: &[&str],
    home: &Path,
    wait_for: &str,
    keys: &str,
) -> Result {
    run_inner(program, prefix, args, home, wait_for, keys, None)
}

pub fn run_with_probe(
    program: &str,
    prefix: &[&str],
    args: &[&str],
    home: &Path,
    wait_for: &str,
    keys: &str,
    probe: &Probe,
) -> Result {
    probe.wait_for_start();
    run_inner(program, prefix, args, home, wait_for, keys, Some(probe))
}

fn run_inner(
    program: &str,
    prefix: &[&str],
    args: &[&str],
    home: &Path,
    wait_for: &str,
    keys: &str,
    probe: Option<&Probe>,
) -> Result {
    let _guard = PTY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("PTY harness lock");
    let _probe_guard = probe.map(Probe::enter);
    let script = r#"
import os, pty, select, sys, time
home, wait_for, keys = sys.argv[1], sys.argv[2].encode("utf-8"), sys.argv[3].encode("latin-1")
program = sys.argv[4]
argv = sys.argv[4:]
env = os.environ.copy()
env["HOME"] = home
env["TERM"] = "xterm-256color"
env["NO_COLOR"] = "1"
env["SHELL"] = "/bin/zsh"
pid, fd = pty.fork()
if pid == 0:
    os.chdir(env["INCODEX_TEST_ROOT"])
    os.execvpe(program, argv, env)
buf = bytearray()
sent = False
deadline = time.time() + 12
while time.time() < deadline:
    ready, _, _ = select.select([fd], [], [], 0.1)
    if not ready:
        continue
    try:
        chunk = os.read(fd, 8192)
    except OSError:
        break
    if not chunk:
        break
    buf.extend(chunk)
    if not sent and wait_for in buf:
        os.write(fd, keys)
        sent = True
try:
    _, status = os.waitpid(pid, 0)
except ChildProcessError:
    status = 0
code = os.waitstatus_to_exitcode(status) if hasattr(os, "waitstatus_to_exitcode") else 1
sys.stdout.buffer.write(("STATUS %d\n" % code).encode())
sys.stdout.buffer.write(bytes(buf))
"#;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root");
    let mut command = Command::new("python3");
    command
        .arg("-c")
        .arg(script)
        .arg(home)
        .arg(wait_for)
        .arg(keys)
        .arg(program)
        .args(prefix)
        .args(args)
        .current_dir(&root)
        .env("INCODEX_TEST_ROOT", &root);
    let output = command.output().expect("spawn PTY harness");
    let raw = String::from_utf8_lossy(&output.stdout).into_owned();
    let (status_line, stdout) = raw.split_once('\n').unwrap_or((&raw, ""));
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let stderr = if output.status.success() {
        stderr
    } else if stderr.is_empty() {
        format!("PTY harness exited with {}", output.status)
    } else {
        format!("{stderr} (PTY harness exited with {})", output.status)
    };
    Result {
        status: status_line
            .strip_prefix("STATUS ")
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(1),
        stdout: stdout.to_string(),
        stderr,
    }
}
