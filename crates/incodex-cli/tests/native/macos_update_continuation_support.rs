/**
 * [INPUT]: 接收续接集成测试的路径、子进程与有限等待请求。
 * [OUTPUT]: 提供隔离目录、权限、PID 身份和超时工具。
 * [POS]: macOS Coordinator 回归的私有测试支撑，不触碰产品状态。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use serde_json::Value;
use std::ffi::CStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) struct RunRoot {
    pub(crate) path: PathBuf,
    keep: bool,
}

impl RunRoot {
    pub(crate) fn create() -> Self {
        let keep = std::env::var_os("INCODEX_NATIVE_CONTINUATION_EVIDENCE_DIR").is_some();
        let parent = std::env::var_os("INCODEX_NATIVE_CONTINUATION_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        fs::create_dir_all(&parent)
            .unwrap_or_else(|error| panic!("create evidence parent {}: {error}", parent.display()));
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = parent.join(format!(
            "continuation.{}.{}.{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
            nonce
        ));
        private_dir(&path);
        Self { path, keep }
    }
}

impl Drop for RunRoot {
    fn drop(&mut self) {
        if self.keep || thread::panicking() {
            eprintln!("CONTINUATION_EVIDENCE_PRESERVED={}", self.path.display());
        } else {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub(crate) struct OwnedChild {
    child: Child,
    finished: bool,
}

impl OwnedChild {
    pub(crate) fn spawn(command: &mut Command, role: &str) -> Self {
        Self {
            child: command
                .spawn()
                .unwrap_or_else(|error| panic!("start owned {role}: {error}")),
            finished: false,
        }
    }

    pub(crate) fn id(&self) -> u32 {
        self.child.id()
    }

    pub(crate) fn request_normal_exit(&self) -> io::Result<()> {
        let result = unsafe { libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(crate) fn wait_timeout(&mut self, timeout: Duration) -> io::Result<ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                self.finished = true;
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "owned process timed out",
                ));
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let _ = unsafe { libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                self.finished = true;
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.finished = true;
    }
}

pub(crate) fn xcrun(arguments: &[&str]) -> PathBuf {
    let result = Command::new("xcrun")
        .args(arguments)
        .output()
        .expect("start xcrun");
    assert!(
        result.status.success(),
        "xcrun {arguments:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    PathBuf::from(String::from_utf8_lossy(&result.stdout).trim())
}

pub(crate) fn wait_for_json_pid(
    path: &Path,
    child: &mut OwnedChild,
    timeout: Duration,
) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = wait_for_json(path, Duration::from_millis(1)) {
            return if value["pid"].as_u64() == Some(u64::from(child.id())) {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ready PID mismatch",
                ))
            };
        }
        if let Some(status) = child.child.try_wait()? {
            child.finished = true;
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Coordinator exited before ready: {status}"),
            ));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Coordinator ready timed out",
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

pub(crate) fn wait_for_file(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.is_file() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    path.is_file()
}

pub(crate) fn wait_for_json(path: &Path, timeout: Duration) -> Option<Value> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(value) = read_json(path) {
            return Some(value);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn wait_for_process_exit(
    pid: u32,
    expected_executable: &Path,
    timeout: Duration,
) -> bool {
    let expected = canonical(expected_executable);
    let deadline = Instant::now() + timeout;
    loop {
        if process_path(pid).as_deref().map(canonical).as_ref() != Some(&expected) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

pub(crate) fn terminate_exact_process(pid: u32, executable: &Path) {
    let expected = canonical(executable);
    if process_path(pid).as_deref().map(canonical).as_ref() != Some(&expected) {
        return;
    }
    let _ = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    wait_for_process_exit(pid, executable, Duration::from_secs(3));
}

pub(crate) fn release_helper(gate: &Path, timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let writer = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(gate);
        match writer {
            Ok(mut writer) => {
                writer.write_all(b"GO\n")?;
                writer.flush()?;
                return Ok(());
            }
            Err(error) if error.raw_os_error() == Some(libc::ENXIO) => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "helper did not open its FIFO reader",
                    ));
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error),
        }
    }
}

pub(crate) fn read_json(path: &Path) -> Result<Value, serde_json::Error> {
    serde_json::from_slice(&fs::read(path).unwrap_or_default())
}

pub(crate) fn read_lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .take(1024)
        .map(str::to_owned)
        .collect()
}

pub(crate) fn process_path(pid: u32) -> Option<PathBuf> {
    let mut buffer = vec![0i8; 4096];
    let length = unsafe {
        libc::proc_pidpath(
            pid as libc::c_int,
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
        )
    };
    if length <= 0 {
        return None;
    }
    let path = unsafe { CStr::from_ptr(buffer.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    Some(PathBuf::from(path))
}

pub(crate) fn copy_private_executable(source: &Path, destination: &Path) {
    fs::copy(source, destination)
        .unwrap_or_else(|error| panic!("copy {}: {error}", source.display()));
    set_mode(destination, 0o700);
}

pub(crate) fn make_fifo(path: &Path) {
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    let result = unsafe { libc::mkfifo(path.as_ptr(), 0o600) };
    assert_eq!(
        result,
        0,
        "create owned helper release FIFO: {}",
        io::Error::last_os_error()
    );
}

pub(crate) fn create_private_file(path: &Path) -> File {
    let file =
        File::create(path).unwrap_or_else(|error| panic!("create {}: {error}", path.display()));
    set_mode(path, 0o600);
    file
}

pub(crate) fn write_private(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    set_mode(path, 0o600);
}

pub(crate) fn private_dir(path: &Path) {
    fs::create_dir_all(path)
        .unwrap_or_else(|error| panic!("create dir {}: {error}", path.display()));
    set_mode(path, 0o700);
}

pub(crate) fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .unwrap_or_else(|error| panic!("set mode {:04o} on {}: {error}", mode, path.display()));
}

pub(crate) fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

pub(crate) fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn c_string_literal(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub(crate) fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
