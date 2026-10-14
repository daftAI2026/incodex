#![cfg(target_os = "macos")]

/**
 * [INPUT]: 编译 Coordinator 原生源，并驱动隔离宿主退出场景。
 * [OUTPUT]: 验证诊断默认关闭及缺失 pending、普通退出事件。
 * [POS]: CLI macOS Coordinator 的原生诊断回归，不启动官方 App。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
const TRACE_PREFIX: &str = "[incodex-update-diagnostic] ";
const TRACE_FLAG: &str = "INCODEX_DIAGNOSTIC_UPDATE_TRACE";
const TRACE_TEST_DEFINE: &str = "INCODEX_DIAGNOSTIC_TRACE_TEST_STDERR=1";

struct RunRoot {
    path: PathBuf,
    parent_mode_before: Option<u32>,
    parent_mode_after: u32,
    child_mode: u32,
    keep: bool,
}

impl RunRoot {
    fn create() -> Self {
        let keep = std::env::var_os("INCODEX_NATIVE_TRACE_EVIDENCE_DIR").is_some();
        let parent = std::env::var_os("INCODEX_NATIVE_TRACE_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let parent_mode_before = parent.exists().then(|| directory_mode(&parent));
        fs::create_dir_all(&parent)
            .unwrap_or_else(|error| panic!("create evidence parent {}: {error}", parent.display()));
        let parent_mode_after = directory_mode(&parent);
        if let Some(before) = parent_mode_before {
            assert_eq!(
                parent_mode_after, before,
                "RunRoot must not change existing evidence parent permissions"
            );
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = parent.join(format!(
            "coordinator-trace.{}.{}.{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
            nonce
        ));
        private_dir(&path);
        let child_mode = directory_mode(&path);
        assert_eq!(
            child_mode, 0o700,
            "only the unique run directory is private"
        );
        Self {
            path,
            parent_mode_before,
            parent_mode_after,
            child_mode,
            keep,
        }
    }
}

impl Drop for RunRoot {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

struct OwnedChild {
    child: Child,
    finished: bool,
}

impl OwnedChild {
    fn new(child: Child) -> Self {
        Self {
            child,
            finished: false,
        }
    }

    fn id(&self) -> u32 {
        self.child.id()
    }

    fn request_term(&self) -> io::Result<()> {
        let result = unsafe { libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn wait_timeout(&mut self, timeout: Duration) -> io::Result<ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                self.finished = true;
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "owned fixture process timed out",
                ));
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

struct CasePaths {
    root: PathBuf,
    home: PathBuf,
    tmp: PathBuf,
    incodex: PathBuf,
    coordinator_executable: PathBuf,
    host_app: PathBuf,
    ready: PathBuf,
    pending: PathBuf,
}

#[derive(Clone)]
struct ProcessResult {
    name: &'static str,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    subject_pid: Option<u32>,
    pending_exists: bool,
}

#[test]
fn native_coordinator_diagnostics_are_opt_in_and_cover_absent_pending_and_ordinary_exit() {
    let run = RunRoot::create();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = repository.join("crates/incodex-cli/native/macos_update_coordinator.m");
    let source_bytes = fs::read(&source).expect("read native Coordinator source");
    let source_sha256 = hex(&Sha256::digest(&source_bytes));
    let clang = xcrun(&["--find", "clang"]);
    let sdk = xcrun(&["--sdk", "macosx", "--show-sdk-path"]);
    let binary = run.path.join("incodex-update-coordinator");
    let compile_stdout = run.path.join("compile.stdout");
    let compile_stderr = run.path.join("compile.stderr");
    let compile_command = format!(
        "{} -isysroot {} -I {} -fobjc-arc -D{} {} -framework AppKit -framework Foundation -o {}",
        clang.display(),
        sdk.display(),
        repository.join("crates/incodex-cli/native").display(),
        TRACE_TEST_DEFINE,
        source.display(),
        binary.display()
    );
    fs::write(run.path.join("compile-command.txt"), &compile_command)
        .expect("write compile command");
    let compile = Command::new(&clang)
        .arg("-isysroot")
        .arg(&sdk)
        .arg("-I")
        .arg(repository.join("crates/incodex-cli/native"))
        .arg("-fobjc-arc")
        .arg(format!("-D{TRACE_TEST_DEFINE}"))
        .arg(&source)
        .args(["-framework", "AppKit", "-framework", "Foundation", "-o"])
        .arg(&binary)
        .output()
        .expect("start xcrun-selected clang");
    write_private(&compile_stdout, &compile.stdout);
    write_private(&compile_stderr, &compile.stderr);
    assert!(
        compile.status.success(),
        "native Coordinator compile failed before behavioral assertions; status={:?}; stdout={}; stderr={}",
        compile.status.code(),
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let binary_bytes = fs::read(&binary).expect("read compiled Coordinator");
    let binary_sha256 = hex(&Sha256::digest(&binary_bytes));
    let absent_default = prepare_case(&run.path, "absent-default", &binary, &binary_sha256);
    let absent_trace = prepare_case(&run.path, "absent-trace", &binary, &binary_sha256);
    let ordinary_default = prepare_case(&run.path, "ordinary-default", &binary, &binary_sha256);
    let ordinary_trace = prepare_case(&run.path, "ordinary-trace", &binary, &binary_sha256);

    let absent_default_result = run_recovery(&absent_default, false);
    let absent_trace_result = run_recovery(&absent_trace, true);
    let ordinary_default_result = run_ordinary_exit(&ordinary_default, false);
    let ordinary_trace_result = run_ordinary_exit(&ordinary_trace, true);
    let cases = [
        &absent_default_result,
        &absent_trace_result,
        &ordinary_default_result,
        &ordinary_trace_result,
    ];

    let result = json!({
        "nativeSourceSha256": source_sha256,
        "nativeBinarySha256": binary_sha256,
        "evidenceParentModeBefore": run.parent_mode_before.map(mode_string),
        "evidenceParentModeAfter": mode_string(run.parent_mode_after),
        "runDirectoryMode": mode_string(run.child_mode),
        "compileExitCode": compile.status.code(),
        "compileSucceeded": compile.status.success(),
        "testMacro": TRACE_TEST_DEFINE,
        "runtimeFlag": TRACE_FLAG,
        "cases": cases.iter().map(|case| json!({
            "name": case.name,
            "exitCode": case.exit_code,
            "subjectPid": case.subject_pid,
            "pendingExists": case.pending_exists,
            "traceLines": trace_lines(&case.stderr),
            "stdout": case.stdout,
            "stderr": case.stderr,
        })).collect::<Vec<_>>(),
        "expectedTraceCodes": {
            "absent-trace": ["recovery-pending-absent"],
            "ordinary-trace": ["host-exit-ordinary-cleanup", "pending-remove-result"],
        },
        "claim": "native Coordinator source compiled and isolated synthetic processes recorded their actual exits, pending state, and opt-in trace output"
    });
    write_private(
        &run.path.join("result.json"),
        serde_json::to_vec_pretty(&result)
            .expect("serialize test receipt")
            .as_slice(),
    );
    println!("COORDINATOR_NATIVE_TRACE_EVIDENCE={}", run.path.display());
    println!("COORDINATOR_NATIVE_TRACE_RESULT={}", result);

    for case in &cases {
        assert_eq!(
            case.exit_code,
            Some(0),
            "{} process must exit successfully",
            case.name
        );
    }
    assert!(
        !absent_default_result.pending_exists && !absent_trace_result.pending_exists,
        "both absent-pending cases must remain absent"
    );
    assert!(
        !ordinary_default_result.pending_exists && !ordinary_trace_result.pending_exists,
        "ordinary cleanup must remove its owned pending handoff"
    );
    assert!(
        !absent_default_result.stderr.contains(TRACE_PREFIX)
            && !ordinary_default_result.stderr.contains(TRACE_PREFIX),
        "diagnostic output must remain absent by default"
    );
    assert_eq!(
        codes(&absent_trace_result.stderr),
        vec!["recovery-pending-absent".to_owned()],
        "flag-on second-stage startup must report that no pending handoff exists"
    );
    assert_eq!(
        codes(&ordinary_trace_result.stderr),
        vec![
            "host-exit-ordinary-cleanup".to_owned(),
            "pending-remove-result".to_owned(),
        ],
        "flag-on ordinary host exit must report the selected branch and actual pending removal result"
    );
    assert!(
        ordinary_trace_result.stderr.lines().any(|line| {
            line.starts_with(TRACE_PREFIX)
                && line.contains(&format!(
                    "subject_pid={}",
                    ordinary_trace_result.subject_pid.unwrap_or_default()
                ))
                && line.ends_with("value=1")
        }),
        "ordinary cleanup must bind its removal result to the synthetic host PID"
    );
}

fn xcrun(arguments: &[&str]) -> PathBuf {
    let output = Command::new("xcrun")
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("start xcrun {arguments:?}: {error}"));
    assert!(
        output.status.success(),
        "xcrun {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
}

fn prepare_case(base: &Path, name: &str, binary: &Path, binary_sha256: &str) -> CasePaths {
    let root = base.join(name);
    let home = root.join("home");
    let tmp = root.join("tmp");
    let incodex = home.join(".incodex");
    let coordinator_app = incodex
        .join("helpers/macos-update")
        .join(binary_sha256)
        .join("Incodex Update Coordinator.app");
    let coordinator_contents = coordinator_app.join("Contents");
    let coordinator_macos = coordinator_contents.join("MacOS");
    let coordinator_executable = coordinator_macos.join("incodex-update-coordinator");
    let host_app = root.join("FakeHost.app");
    let ready = root.join("ready.json");
    let pending = incodex.join("macos-update/pending.json");

    let directories = [
        root.clone(),
        home.clone(),
        tmp.clone(),
        incodex.clone(),
        incodex.join("helpers"),
        incodex.join("helpers/macos-update"),
        coordinator_app.clone(),
        coordinator_contents.clone(),
        coordinator_macos.clone(),
        host_app.join("Contents"),
        incodex.join("macos-update"),
    ];
    for directory in directories {
        private_dir(&directory);
    }
    fs::copy(binary, &coordinator_executable)
        .expect("copy Coordinator into its isolated app bundle");
    fs::set_permissions(&coordinator_executable, fs::Permissions::from_mode(0o700))
        .expect("make isolated Coordinator executable private");
    let coordinator_info = plist(
        &format!(
            "com.daftai.incodex.test.coordinator.{}.{}",
            std::process::id(),
            name
        ),
        "incodex-update-coordinator",
        "1",
    );
    write_private(
        &coordinator_contents.join("Info.plist"),
        coordinator_info.as_bytes(),
    );
    let host_info = plist(
        &format!(
            "com.daftai.incodex.test.host.{}.{}",
            std::process::id(),
            name
        ),
        "fake-host-not-launched",
        "10001",
    );
    write_private(&host_app.join("Contents/Info.plist"), host_info.as_bytes());

    CasePaths {
        root,
        home,
        tmp,
        incodex,
        coordinator_executable,
        host_app,
        ready,
        pending,
    }
}

fn run_recovery(case: &CasePaths, trace_enabled: bool) -> ProcessResult {
    let stdout_path = case.root.join("recovery.stdout");
    let stderr_path = case.root.join("recovery.stderr");
    let stdout = create_private_file(&stdout_path);
    let stderr = create_private_file(&stderr_path);
    let mut command = Command::new(&case.coordinator_executable);
    clean_environment(&mut command, case);
    if trace_enabled {
        command.env(TRACE_FLAG, "1");
    }
    let mut child = OwnedChild::new(
        command
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("start isolated second-stage Coordinator with no pending file"),
    );
    let status = child
        .wait_timeout(Duration::from_secs(10))
        .expect("second-stage Coordinator must exit without pending state");
    let stdout = read_text(&stdout_path);
    let stderr = read_text(&stderr_path);
    ProcessResult {
        name: if trace_enabled {
            "absent-trace"
        } else {
            "absent-default"
        },
        exit_code: status.code(),
        stdout,
        stderr,
        subject_pid: None,
        pending_exists: case.pending.exists(),
    }
}

fn run_ordinary_exit(case: &CasePaths, trace_enabled: bool) -> ProcessResult {
    let host = Command::new("/bin/sleep")
        .arg("120")
        .env_clear()
        .env("HOME", &case.home)
        .env("TMPDIR", &case.tmp)
        .env("PATH", "/usr/bin:/bin")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start owned synthetic host process");
    let mut host = OwnedChild::new(host);
    let host_pid = host.id();
    let stdout_path = case.root.join("ordinary.stdout");
    let stderr_path = case.root.join("ordinary.stderr");
    let stdout = create_private_file(&stdout_path);
    let stderr = create_private_file(&stderr_path);
    let mut command = Command::new(&case.coordinator_executable);
    clean_environment(&mut command, case);
    command
        .env("INCODEX_MACOS_UPDATE_COORDINATOR", "1")
        .env("INCODEX_MACOS_UPDATE_HOST_PID", host_pid.to_string())
        .env("INCODEX_MACOS_UPDATE_INSTALL_ID", "synthetic-install-id")
        .env("INCODEX_MACOS_UPDATE_HOST_APP", &case.host_app)
        .env(
            "INCODEX_MACOS_UPDATE_HELPER_PATH",
            case.incodex.join("synthetic-helper"),
        )
        .env("INCODEX_MACOS_UPDATE_HELPER_SHA256", "a".repeat(64))
        .env("INCODEX_MACOS_UPDATE_HANDOFF_ID", "synthetic-handoff-id")
        .env("INCODEX_MACOS_UPDATE_READY_PATH", &case.ready)
        .env("INCODEX_MACOS_UPDATE_PENDING_PATH", &case.pending);
    if trace_enabled {
        command.env(TRACE_FLAG, "1");
    }
    let mut coordinator = OwnedChild::new(
        command
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("start isolated initial Coordinator with synthetic host"),
    );
    wait_for_file(&case.ready, &mut coordinator, Duration::from_secs(10))
        .expect("Coordinator must publish ready before the synthetic host exits");
    host.request_term()
        .expect("request exit of exact owned synthetic host PID");
    host.wait_timeout(Duration::from_secs(5))
        .expect("synthetic host must exit after the test-only signal");
    let status = coordinator
        .wait_timeout(Duration::from_secs(10))
        .expect("Coordinator must finish ordinary cleanup after host exit");
    ProcessResult {
        name: if trace_enabled {
            "ordinary-trace"
        } else {
            "ordinary-default"
        },
        exit_code: status.code(),
        stdout: read_text(&stdout_path),
        stderr: read_text(&stderr_path),
        subject_pid: Some(host_pid),
        pending_exists: case.pending.exists(),
    }
}

fn clean_environment(command: &mut Command, case: &CasePaths) {
    command
        .env_clear()
        .env("HOME", &case.home)
        .env("TMPDIR", &case.tmp)
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "en_US.UTF-8");
}

fn wait_for_file(path: &Path, child: &mut OwnedChild, timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if path.is_file() {
            return Ok(());
        }
        if let Some(status) = child.child.try_wait()? {
            child.finished = true;
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Coordinator exited before ready with {status}"),
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

fn trace_lines(stderr: &str) -> Vec<String> {
    stderr
        .lines()
        .filter(|line| line.starts_with(TRACE_PREFIX))
        .map(str::to_owned)
        .collect()
}

fn codes(stderr: &str) -> Vec<String> {
    trace_lines(stderr)
        .iter()
        .filter_map(|line| {
            line.strip_prefix(TRACE_PREFIX)
                .and_then(|rest| {
                    rest.split_whitespace()
                        .find_map(|field| field.strip_prefix("code="))
                })
                .map(str::to_owned)
        })
        .collect()
}

fn plist(bundle_id: &str, executable: &str, build: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>{bundle_id}</string><key>CFBundleExecutable</key><string>{executable}</string><key>CFBundleName</key><string>Incodex Isolated Fixture</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>{build}</string><key>LSBackgroundOnly</key><true/><key>NSPrincipalClass</key><string>NSApplication</string></dict></plist>\n"
    )
}

fn directory_mode(path: &Path) -> u32 {
    fs::metadata(path)
        .unwrap_or_else(|error| {
            panic!(
                "read fixture directory metadata {}: {error}",
                path.display()
            )
        })
        .permissions()
        .mode()
        & 0o7777
}

fn mode_string(mode: u32) -> String {
    format!("{:04o}", mode)
}

fn private_dir(path: &Path) {
    fs::create_dir_all(path)
        .unwrap_or_else(|error| panic!("create private fixture dir {}: {error}", path.display()));
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("set fixture dir permissions {}: {error}", path.display()));
}

fn create_private_file(path: &Path) -> File {
    let file = File::create(path)
        .unwrap_or_else(|error| panic!("create fixture output {}: {error}", path.display()));
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .unwrap_or_else(|error| panic!("set fixture file permissions {}: {error}", path.display()));
    file
}

fn write_private(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes)
        .unwrap_or_else(|error| panic!("write fixture file {}: {error}", path.display()));
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .unwrap_or_else(|error| panic!("set fixture file permissions {}: {error}", path.display()));
}

fn read_text(path: &Path) -> String {
    String::from_utf8_lossy(&fs::read(path).unwrap_or_default()).into_owned()
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(DIGITS[(byte >> 4) as usize] as char);
        encoded.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    encoded
}
