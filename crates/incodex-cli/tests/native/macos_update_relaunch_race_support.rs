/**
 * [INPUT]: 接收竞态测试的源码、内容寻址路径与隔离进程请求。
 * [OUTPUT]: 构建 Coordinator/假宿主并提供有序探针门与 CAS 回执。
 * [POS]: macOS 更新竞态测试支撑；核对隔离进程，不启动真实宿主。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::support::{
    canonical, copy_private_executable, create_private_file, mode, private_dir, process_path,
    read_json, set_mode, sha256, wait_for_file, wait_for_json, wait_for_process_exit,
    write_private, xcrun, xml_escape,
};
use super::*;
use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub(super) const CHILD_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const HELPER_QUIET_WINDOW: Duration = Duration::from_millis(300);
pub(super) const RETRY_WINDOW: Duration = Duration::from_secs(8);
pub(super) const CANCEL_WINDOW: Duration = Duration::from_secs(3);
pub(super) const HOST_NO_RELAUNCH_WINDOW: Duration = Duration::from_secs(3);
const HOST_SOURCE: &str = include_str!("macos-update-continuation-host.c");
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn prepare_case(
    root: &Path,
    name: &str,
    coordinator_binary: &Path,
    coordinator_hash: &str,
    test_binary: &Path,
) -> CasePaths {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let case_root = root.join(format!("case-{name}-{sequence}"));
    let home = case_root.join("home");
    let tmp = case_root.join("tmp");
    let events = case_root.join("events");
    let incodex = home.join(".incodex");
    let coordinator_app = incodex
        .join("helpers/macos-update")
        .join(coordinator_hash)
        .join("Incodex Update Coordinator.app");
    let coordinator_executable = coordinator_app.join("Contents/MacOS/incodex-update-coordinator");
    let host_app = case_root.join("Applications/FakeHost.app");
    let host_executable = host_executable(&host_app);
    let host_marker = events.join("host-relaunch.json");
    let pending = incodex.join("macos-update/pending.json");
    let install_id = format!("race-install-{}-{sequence}", std::process::id());
    let handoff_id = format!("race-handoff-{}-{sequence}", std::process::id());

    for directory in [
        case_root.clone(),
        home.clone(),
        tmp.clone(),
        events.clone(),
        incodex.clone(),
        incodex.join("helpers"),
        incodex.join("helpers/macos-update"),
        coordinator_app.join("Contents/MacOS"),
        incodex.join("macos-update"),
        host_app.join("Contents/MacOS"),
    ] {
        private_dir(&directory);
    }
    copy_private_executable(coordinator_binary, &coordinator_executable);
    write_private(
        &coordinator_app.join("Contents/Info.plist"),
        coordinator_plist(coordinator_hash, &home, &tmp).as_bytes(),
    );
    write_host_plist(
        &host_app,
        &format!(
            "com.daftai.incodex.test.race.{}.{sequence}",
            std::process::id()
        ),
    );

    let helper_bytes = helper_script(test_binary).into_bytes();
    let helper_hash = sha256(&helper_bytes);
    let helper = incodex
        .join("helpers/macos-update")
        .join(&helper_hash)
        .join("incodex");
    private_dir(helper.parent().unwrap());
    write_private(&helper, &helper_bytes);
    set_mode(&helper, 0o700);
    compile_host_binary(&case_root, &host_executable, &host_marker);

    let registration = json!({
        "schemaVersion": 2,
        "installId": install_id,
        "appPath": host_app.display().to_string(),
        "helperPath": helper.display().to_string(),
        "helperSha256": helper_hash,
    });
    write_json_private(
        &incodex.join("macos-update/registration.json"),
        &registration,
    );
    let lock_path = pending.parent().unwrap().join(".pending.lock");
    write_private(&lock_path, b"");
    set_mode(&lock_path, 0o600);
    write_json_private(
        &pending,
        &json!({
            "schemaVersion": 2,
            "installId": install_id,
            "appPath": host_app.display().to_string(),
            "helperPath": helper.display().to_string(),
            "helperSha256": helper_hash,
            "handoffId": handoff_id,
            "coordinatorPid": 0,
            "sourceBuild": "10001",
            "installQueued": true,
            "relaunchHost": true,
        }),
    );
    CasePaths {
        root: case_root,
        home,
        tmp,
        events,
        coordinator_executable,
        host_app,
        host_executable,
        host_marker,
        pending,
        install_id,
        handoff_id,
    }
}

pub(super) fn start_coordinator(paths: &CasePaths, helper_kind: &str) -> TestChild {
    let mut command = Command::new(&paths.coordinator_executable);
    command
        .env_clear()
        .env("HOME", &paths.home)
        .env("TMPDIR", &paths.tmp)
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "en_US.UTF-8")
        .env("INCODEX_TEST_RACE_EVENTS", &paths.events)
        .env("INCODEX_TEST_RACE_HOST_APP", &paths.host_app)
        .env("INCODEX_TEST_RACE_HELPER_KIND", helper_kind)
        .stdout(Stdio::from(create_private_file(
            &paths.root.join("coordinator.stdout"),
        )))
        .stderr(Stdio::from(create_private_file(
            &paths.root.join("coordinator.stderr"),
        )));
    TestChild::spawn(
        &mut command,
        &paths.coordinator_executable,
        "recovery Coordinator",
    )
}

pub(super) fn start_host(paths: &CasePaths) -> TestChild {
    let mut command = Command::new(&paths.host_executable);
    command
        .env_clear()
        .env("HOME", &paths.home)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let host = TestChild::spawn(&mut command, &paths.host_executable, "fake Host");
    assert!(
        wait_for_file(&paths.host_marker, CHILD_TIMEOUT),
        "fake Host marker timed out"
    );
    host
}

pub(super) fn host_quiescence_positive_control(paths: &CasePaths) -> Value {
    let mut host = start_host(paths);
    let pid = host.id();
    let path_matches = process_path(pid).as_deref().map(canonical).as_ref()
        == Some(&canonical(&paths.host_executable));
    let visible = incodex_macos::AppQuiescence::for_app(&paths.host_app)
        .and_then(|target| target.running_pids())
        .is_ok_and(|pids| pids.contains(&(pid as i32)));
    let normal_exit = host.normal_exit(CHILD_TIMEOUT);
    let quiescent_after_exit = incodex_macos::AppQuiescence::for_app(&paths.host_app)
        .and_then(|target| target.ensure_quiescent())
        .is_ok();
    let marker_removed = fs::remove_file(&paths.host_marker).is_ok();
    json!({
        "pid": pid,
        "executableMatches": path_matches,
        "realProbeFoundExactHost": visible,
        "ownedExitObserved": normal_exit,
        "quiescentAfterExit": quiescent_after_exit,
        "markerRemoved": marker_removed,
    })
}

pub(super) fn compile_coordinator(
    clang: &Path,
    sdk: &Path,
    native_dir: &Path,
    source: &Path,
    output: &Path,
    evidence: &Path,
) {
    let result = Command::new(clang)
        .args(["-isysroot"])
        .arg(sdk)
        .arg("-I")
        .arg(native_dir)
        .arg("-fobjc-arc")
        .arg(source)
        .args(["-framework", "AppKit", "-framework", "Foundation", "-o"])
        .arg(output)
        .output()
        .expect("start clang for current Coordinator");
    write_private(&evidence.join("coordinator-compile.stdout"), &result.stdout);
    write_private(&evidence.join("coordinator-compile.stderr"), &result.stderr);
    assert!(
        result.status.success(),
        "current Coordinator compile failed: status={:?}; stderr={}",
        result.status.code(),
        String::from_utf8_lossy(&result.stderr)
    );
    set_mode(output, 0o700);
}

pub(super) fn wait_for_start(events: &Path, count: usize, timeout: Duration) -> Option<Value> {
    wait_for_marker_count(events, "helper-start-", count, timeout)
}

pub(super) fn wait_for_end(events: &Path, count: usize, timeout: Duration) -> Option<Value> {
    wait_for_marker_count(events, "helper-end-", count, timeout)
}

pub(super) fn wait_for_probe(events: &Path, count: usize, timeout: Duration) -> Option<Value> {
    wait_for_marker_count(events, "helper-probe-", count, timeout)
}

pub(super) fn read_start_markers(events: &Path) -> Vec<Value> {
    read_marker_group(events, "helper-start-")
}

pub(super) fn mutate_pending(paths: &CasePaths, mutation: PendingMutation, owner: u32) -> bool {
    let Ok(lock) = acquire_pending_lock(&paths.pending) else {
        return false;
    };
    let mut pending = match read_json(&paths.pending) {
        Ok(value) => value,
        Err(_) => {
            release_pending_lock(lock);
            return false;
        }
    };
    if pending["coordinatorPid"] != json!(owner) || pending["installId"] != paths.install_id {
        release_pending_lock(lock);
        return false;
    }
    let result = match mutation {
        PendingMutation::Remove => fs::remove_file(&paths.pending).is_ok(),
        PendingMutation::ChangeHandoff => {
            pending["handoffId"] = json!(format!("{}-replacement", paths.handoff_id));
            write_json_atomic(&paths.pending, &pending).is_ok()
        }
        PendingMutation::Keep => false,
    };
    release_pending_lock(lock);
    result
}

pub(super) fn read_pending_locked(path: &Path) -> std::io::Result<Value> {
    let lock = acquire_pending_lock(path)?;
    let result = read_json(path).map_err(|error| std::io::Error::other(error.to_string()));
    release_pending_lock(lock);
    result
}

pub(super) fn write_json_private(path: &Path, value: &Value) {
    write_private(
        path,
        &serde_json::to_vec_pretty(value).expect("serialize private JSON"),
    );
}

pub(super) fn write_case_result(paths: &CasePaths, result: Value) -> Value {
    write_json_private(&paths.root.join("receipt.json"), &result);
    result
}

pub(super) fn wait_for_absence(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    !path.exists()
}

pub(super) fn cleanup_launch_services_host(paths: &CasePaths) {
    let Some(marker) = wait_for_json(&paths.host_marker, Duration::from_millis(50)) else {
        return;
    };
    let Some(pid) = marker["pid"]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
    else {
        return;
    };
    if process_path(pid).as_deref().map(canonical).as_ref()
        == Some(&canonical(&paths.host_executable))
    {
        let _ = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        let _ = wait_for_process_exit(pid, &paths.host_executable, CHILD_TIMEOUT);
    }
}

pub(super) fn host_executable(app: &Path) -> PathBuf {
    app.join("Contents/MacOS/fake-host")
}

pub(super) fn git_head(repository: &Path) -> String {
    Command::new("git")
        .args(["-C", repository.to_str().unwrap(), "rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".into())
}

pub(super) struct TestChild {
    child: Child,
    executable: PathBuf,
    finished: bool,
    status: Option<ExitSnapshot>,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(super) struct ExitSnapshot {
    pub(super) code: Option<i32>,
    pub(super) signal: Option<i32>,
    pub(super) success: bool,
}

impl TestChild {
    fn spawn(command: &mut Command, executable: &Path, role: &str) -> Self {
        let child = command
            .spawn()
            .unwrap_or_else(|error| panic!("start {role}: {error}"));
        Self {
            child,
            executable: executable.to_path_buf(),
            finished: false,
            status: None,
        }
    }

    pub(super) fn id(&self) -> u32 {
        self.child.id()
    }

    pub(super) fn is_running(&mut self) -> bool {
        self.poll().is_none()
    }

    pub(super) fn poll(&mut self) -> Option<ExitSnapshot> {
        if let Some(status) = self.status {
            return Some(status);
        }
        match self.child.try_wait() {
            Ok(Some(status)) => {
                let snapshot = ExitSnapshot {
                    code: status.code(),
                    signal: status.signal(),
                    success: status.success(),
                };
                self.finished = true;
                self.status = Some(snapshot);
                Some(snapshot)
            }
            _ => None,
        }
    }

    pub(super) fn wait_timeout(&mut self, timeout: Duration) -> Option<ExitSnapshot> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.poll() {
                return Some(status);
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    pub(super) fn normal_exit(&mut self, timeout: Duration) -> bool {
        if !self.request_normal_exit() {
            return false;
        }
        self.wait_timeout(timeout)
            .is_some_and(|status| status.signal == Some(libc::SIGTERM))
    }

    pub(super) fn request_normal_exit(&mut self) -> bool {
        if self.poll().is_some() {
            return false;
        }
        let exact = process_path(self.id()).as_deref().map(canonical).as_ref()
            == Some(&canonical(&self.executable));
        exact && unsafe { libc::kill(self.id() as libc::pid_t, libc::SIGTERM) } == 0
    }

    pub(super) fn exit_snapshot(&mut self) -> Option<ExitSnapshot> {
        self.poll()
    }
}

impl Drop for TestChild {
    fn drop(&mut self) {
        if self.poll().is_some() {
            return;
        }
        let exact = process_path(self.id()).as_deref().map(canonical).as_ref()
            == Some(&canonical(&self.executable));
        if exact {
            let _ = unsafe { libc::kill(self.id() as libc::pid_t, libc::SIGTERM) };
        }
        if self.wait_timeout(Duration::from_secs(2)).is_none() && exact {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.finished = true;
        }
    }
}

fn wait_for_marker_count(
    events: &Path,
    prefix: &str,
    count: usize,
    timeout: Duration,
) -> Option<Value> {
    let deadline = Instant::now() + timeout;
    loop {
        let values = read_marker_group(events, prefix);
        if values.len() >= count {
            return values.get(count - 1).cloned();
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn read_marker_group(events: &Path, prefix: &str) -> Vec<Value> {
    let Ok(entries) = fs::read_dir(events) else {
        return Vec::new();
    };
    let mut values = entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
        .filter_map(|entry| read_json(&entry.path()).ok())
        .collect::<Vec<_>>();
    values.sort_by_key(|value| {
        value["attempt"]
            .as_u64()
            .unwrap_or_else(|| value["pid"].as_u64().unwrap_or_default())
    });
    values
}

fn acquire_pending_lock(path: &Path) -> std::io::Result<File> {
    let lock_path = path.parent().unwrap().join(".pending.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&lock_path)?;
    if mode(&lock_path) != 0o600 || unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(lock)
}

fn release_pending_lock(lock: File) {
    unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_UN) };
}

fn write_json_atomic(path: &Path, value: &Value) -> std::io::Result<()> {
    let temporary = path.with_extension("race-write");
    write_private(
        &temporary,
        &serde_json::to_vec(value).expect("serialize pending"),
    );
    fs::rename(temporary, path)
}

fn compile_host_binary(root: &Path, output: &Path, marker: &Path) {
    let source = root.join("fake-host.c");
    write_private(&source, HOST_SOURCE.as_bytes());
    let clang = xcrun(&["--find", "clang"]);
    let sdk = xcrun(&["--sdk", "macosx", "--show-sdk-path"]);
    let define = format!(
        "-DINCODEX_TEST_RELAUNCH_MARKER={}",
        super::support::c_string_literal(&marker.to_string_lossy())
    );
    let result = Command::new(clang)
        .arg("-isysroot")
        .arg(sdk)
        .args(["-Wall", "-Wextra", "-O0"])
        .arg(define)
        .arg(source)
        .arg("-o")
        .arg(output)
        .output()
        .expect("start clang for synthetic Host");
    write_private(&root.join("host-compile.stderr"), &result.stderr);
    assert!(
        result.status.success(),
        "fake Host compile failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    set_mode(output, 0o700);
}

fn helper_script(test_binary: &Path) -> String {
    let binary = shell_quote(&test_binary.to_string_lossy());
    format!(
        "#!/bin/sh\n\
umask 077\n\
events=\"$INCODEX_TEST_RACE_EVENTS\"\n\
pid=$$\n\
kind=\"${{INCODEX_TEST_RACE_HELPER_KIND:-live}}\"\n\
attempt=1\n\
if [ \"$kind\" != generic ]; then\n\
  if (set -C; : > \"$events/first-helper-seen\") 2>/dev/null; then attempt=1; else attempt=2; fi\n\
fi\n\
printf '{{\"attempt\":%s,\"pid\":%s,\"ppid\":%s}}\\n' \"$attempt\" \"$pid\" \"$PPID\" > \"$events/helper-start-$pid.json\"\n\
status=0\n\
if [ \"$kind\" = generic ]; then status=23; else\n\
  gate=release-first-helper\n\
  if [ \"$attempt\" != 1 ]; then gate=release-retry-helper; fi\n\
  while [ ! -e \"$events/$gate\" ]; do\n\
    kill -0 \"$PPID\" 2>/dev/null || exit 70\n\
    /bin/sleep 0.01\n\
  done\n\
  if [ \"$attempt\" = 1 ]; then\n\
    while [ ! -f \"$events/host-pid\" ]; do\n\
      kill -0 \"$PPID\" 2>/dev/null || exit 70\n\
      /bin/sleep 0.01\n\
    done\n\
    host_pid=$(cat \"$events/host-pid\")\n\
    export INCODEX_TEST_RACE_EXPECTED_HOST_PID=\"$host_pid\"\n\
    export INCODEX_TEST_RACE_BUSY_EXIT=75\n\
    if [ \"$kind\" = host-exits-before-failure ]; then\n\
      export INCODEX_TEST_RACE_PROBE_RELEASE=\"$events/release-first-probe\"\n\
    fi\n\
    {binary} --exact helper_quiescence_probe --nocapture >/dev/null 2>&1 &\n\
  else\n\
    {binary} --exact helper_quiescence_probe --nocapture >/dev/null 2>&1 &\n\
  fi\n\
  child=$!\n\
  wait \"$child\"\n\
  status=$?\n\
fi\n\
printf '{{\"attempt\":%s,\"pid\":%s,\"ppid\":%s,\"childPid\":%s,\"exitCode\":%s}}\\n' \\
  \"$attempt\" \"$pid\" \"$PPID\" \"${{child:-0}}\" \"$status\" > \"$events/helper-end-$pid.json\"\n\
exit \"$status\"\n"
    )
}

fn write_host_plist(app: &Path, bundle_id: &str) {
    let bundle_id = xml_escape(bundle_id);
    let contents = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>{bundle_id}</string><key>CFBundleExecutable</key><string>fake-host</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>10002</string><key>LSBackgroundOnly</key><true/><key>NSPrincipalClass</key><string>NSApplication</string></dict></plist>\n"
    );
    write_private(&app.join("Contents/Info.plist"), contents.as_bytes());
}

fn coordinator_plist(hash: &str, home: &Path, tmp: &Path) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>com.daftai.incodex.test.race.coordinator.{hash}</string><key>CFBundleExecutable</key><string>incodex-update-coordinator</string><key>CFBundleName</key><string>Incodex Update Race Test</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>1</string><key>LSBackgroundOnly</key><true/><key>NSPrincipalClass</key><string>NSApplication</string><key>LSEnvironment</key><dict><key>HOME</key><string>{}</string><key>TMPDIR</key><string>{}</string><key>PATH</key><string>/usr/bin:/bin</string></dict></dict></plist>\n",
        xml_escape(&home.to_string_lossy()),
        xml_escape(&tmp.to_string_lossy())
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
