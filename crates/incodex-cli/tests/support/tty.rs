use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Duration;

#[cfg(unix)]
use std::os::fd::AsRawFd;

pub struct PtyGate {
    file: File,
}

impl PtyGate {
    fn acquire() -> Self {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path())
            .expect("open PTY harness lock");

        #[cfg(unix)]
        loop {
            let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
            if result == 0 {
                break;
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                panic!("acquire PTY harness lock: {error}");
            }
        }

        Self { file }
    }
}

impl Drop for PtyGate {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}

#[allow(dead_code)]
pub fn acquire() -> PtyGate {
    PtyGate::acquire()
}

#[allow(dead_code)]
fn lock_path() -> PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root");
    let mut hasher = DefaultHasher::new();
    root.hash(&mut hasher);
    std::env::temp_dir().join(format!(
        "incodex-pty-harness-{hash:016x}.lock",
        hash = hasher.finish()
    ))
}

#[allow(dead_code)]
pub struct Probe {
    start: Barrier,
    active: AtomicUsize,
    maximum: AtomicUsize,
}

#[allow(dead_code)]
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

#[allow(dead_code)]
struct ProbeGuard<'a> {
    probe: &'a Probe,
}

impl Drop for ProbeGuard<'_> {
    fn drop(&mut self) {
        self.probe.active.fetch_sub(1, Ordering::SeqCst);
    }
}

#[allow(dead_code)]
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
    run_inner(
        program,
        prefix,
        args,
        home,
        wait_for,
        keys,
        None,
        Duration::from_secs(12),
        &[],
        80,
    )
}

#[allow(dead_code)]
pub fn run_with_columns(
    program: &str,
    prefix: &[&str],
    args: &[&str],
    home: &Path,
    wait_for: &str,
    keys: &str,
    columns: u16,
) -> Result {
    run_inner(
        program,
        prefix,
        args,
        home,
        wait_for,
        keys,
        None,
        Duration::from_secs(12),
        &[],
        columns,
    )
}

#[allow(dead_code)]
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
    run_inner(
        program,
        prefix,
        args,
        home,
        wait_for,
        keys,
        Some(probe),
        Duration::from_secs(12),
        &[],
        80,
    )
}

#[allow(dead_code)]
pub fn run_with_timeout(
    program: &str,
    prefix: &[&str],
    args: &[&str],
    home: &Path,
    wait_for: &str,
    keys: &str,
    timeout: Duration,
) -> Result {
    run_inner(
        program,
        prefix,
        args,
        home,
        wait_for,
        keys,
        None,
        timeout,
        &[],
        80,
    )
}

#[allow(dead_code)]
#[expect(
    clippy::too_many_arguments,
    reason = "PTY harness keeps process, interaction, timing, and environment inputs explicit"
)]
pub fn run_with_timeout_env(
    program: &str,
    prefix: &[&str],
    args: &[&str],
    home: &Path,
    wait_for: &str,
    keys: &str,
    timeout: Duration,
    extra_env: &[(&str, &str)],
) -> Result {
    run_inner(
        program, prefix, args, home, wait_for, keys, None, timeout, extra_env, 80,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "PTY harness keeps process, interaction, timing, and environment inputs explicit"
)]
fn run_inner(
    program: &str,
    prefix: &[&str],
    args: &[&str],
    home: &Path,
    wait_for: &str,
    keys: &str,
    probe: Option<&Probe>,
    timeout: Duration,
    extra_env: &[(&str, &str)],
    columns: u16,
) -> Result {
    let _guard = PtyGate::acquire();
    let _probe_guard = probe.map(Probe::enter);
    let script = r#"
import errno, fcntl, os, pty, select, struct, sys, termios, time
home, wait_for, keys = sys.argv[1], sys.argv[2].encode("utf-8"), sys.argv[3].encode("latin-1")
timeout = float(sys.argv[4])
columns = int(sys.argv[5])
program = sys.argv[6]
argv = sys.argv[6:]
env = os.environ.copy()
env["HOME"] = home
env["TERM"] = "xterm-256color"
env["NO_COLOR"] = "1"
env["SHELL"] = "/bin/zsh"
pid, fd = pty.fork()
if pid == 0:
    fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 24, columns, 0, 0))
    os.chdir(env["INCODEX_TEST_ROOT"])
    os.execvpe(program, argv, env)
buf = bytearray()
sent = False
child_status = None
pty_closed = False
deadline = time.monotonic() + timeout
while time.monotonic() < deadline:
    ready, _, _ = select.select([fd], [], [], 0.1)
    if not ready:
        done, status = os.waitpid(pid, os.WNOHANG)
        if done == pid:
            child_status = status
            break
        continue
    try:
        chunk = os.read(fd, 8192)
    except OSError:
        pty_closed = True
        break
    if not chunk:
        pty_closed = True
        break
    buf.extend(chunk)
    if not sent and wait_for in buf:
        try:
            offset = 0
            while offset < len(keys):
                written = os.write(fd, keys[offset:])
                if written <= 0:
                    raise OSError(errno.EIO, "PTY input closed")
                offset += written
        except OSError as error:
            pty_closed = True
            sys.stderr.write("PTY input write failed: %s\n" % error)
            break
        sent = True
    done, status = os.waitpid(pid, os.WNOHANG)
    if done == pid:
        child_status = status
        break
if child_status is None:
    while pty_closed and time.monotonic() < deadline:
        try:
            done, status = os.waitpid(pid, os.WNOHANG)
        except ChildProcessError:
            done = pid
            status = 0
        if done == pid:
            child_status = status
            break
        remaining = deadline - time.monotonic()
        if remaining > 0:
            select.select([], [], [], min(0.05, remaining))
if child_status is None:
    try:
        done, status = os.waitpid(pid, os.WNOHANG)
    except ChildProcessError:
        done = pid
        status = 0
    if done == pid:
        child_status = status
timed_out = child_status is None
if timed_out:
    try:
        os.kill(pid, 9)
    except ProcessLookupError:
        pass
    _, child_status = os.waitpid(pid, 0)
status = child_status
code = os.waitstatus_to_exitcode(status) if hasattr(os, "waitstatus_to_exitcode") else 1
if timed_out:
    code = 124
    sys.stderr.write("PTY harness timed out waiting for %r\n" % wait_for)
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
        .arg(timeout.as_secs_f64().to_string())
        .arg(columns.to_string())
        .arg(program)
        .args(prefix)
        .args(args)
        .current_dir(&root)
        .env("INCODEX_TEST_ROOT", &root);
    for (key, value) in extra_env {
        command.env(key, value);
    }
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
