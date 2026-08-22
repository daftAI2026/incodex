//! codesign, plist, ditto, Launch Services, and quit.

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

mod entitlements;
mod signing;
mod signing_outer;
mod signing_policy;
pub use signing::*;
pub use signing_outer::inspect_outer_signing;
pub use signing_policy::validate_generic_nested_components;

#[derive(Debug, Clone, Default)]
pub struct PlistInfo {
    pub bundle_identifier: String,
    pub app_version: String,
    pub app_build: String,
    pub executable: String,
}

/// 官方应用退出后的最大等待时间。安装事务不能在进程仍持有 app 文件时开始。
pub const QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(60);
/// 退出轮询间隔；足够短以避免把正常退出误判成超时，又不会忙等。
pub const QUIESCENCE_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// 进程探针只返回 PID 与完整 executable path，不接受 basename 猜测。
pub trait ProcessProbe {
    fn process_paths(&self) -> Result<Vec<(i32, PathBuf)>, String>;
}

/// 发送官方 Codex 退出请求的最小接口，测试可替换而不触碰真实 osascript。
pub trait QuitRequester {
    fn request_quit(&mut self) -> Result<(), String>;
}

/// 可注入的单调时钟，避免超时测试依赖真实 60 秒。
pub trait QuiescenceClock {
    fn now(&self) -> Instant;
    fn sleep(&mut self, duration: Duration);
}

/// 一个 app 的严格 executable 身份与存活检测。
///
/// 这个类型故意不保存“已经检查过”的状态；每次检查都重新扫描进程表。
/// 这样一次 quiescent 观察不会变成永久事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppQuiescence {
    executable: PathBuf,
}

impl AppQuiescence {
    /// 从 app 的 CFBundleExecutable 构造严格 executable path。
    pub fn for_app(app: &Path) -> Result<Self, String> {
        // CLI 接受相对 `--app`；先绑定现有 bundle 的真实路径，再构造进程身份。
        let app = fs::canonicalize(app)
            .map_err(|error| format!("cannot resolve Codex app {}: {error}", app.display()))?;
        let quiescence = Self::for_bundle_at(&app, &app)?;
        let executable = quiescence.executable;
        if !executable.is_file() {
            return Err(format!(
                "Codex executable from CFBundleExecutable not found: {}",
                executable.display()
            ));
        }
        let executable = fs::canonicalize(&executable).map_err(|error| {
            format!(
                "cannot resolve Codex executable {}: {error}",
                executable.display()
            )
        })?;
        Self::from_executable(executable)
    }

    /// 从一个已知 bundle 的严格 executable 名称构造另一个 bundle 位置的预期路径。
    ///
    /// recover 可能面对已经被移出的 live target：这时只能读取 backup 的 plist，
    /// 但进程探针仍必须盯住 journal target，而不是 backup 的 executable。
    pub fn for_bundle_at(bundle: &Path, target: &Path) -> Result<Self, String> {
        let name = read_plist_executable(bundle)?;
        let name_path = Path::new(&name);
        let mut components = name_path.components();
        if !matches!(components.next(), Some(std::path::Component::Normal(_)))
            || components.next().is_some()
        {
            return Err(format!(
                "CFBundleExecutable is not a plain executable name: {name}"
            ));
        }
        Self::from_executable(target.join("Contents").join("MacOS").join(name_path))
    }

    /// 构造测试或已解析路径的身份。路径必须是绝对路径，但不在这里猜测 basename。
    pub fn from_executable(executable: PathBuf) -> Result<Self, String> {
        if !executable.is_absolute() {
            return Err(format!(
                "executable path must be absolute: {}",
                executable.display()
            ));
        }
        Ok(Self { executable })
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// 在当前系统进程表中检查 exact executable path。
    pub fn ensure_quiescent(&self) -> Result<(), String> {
        self.ensure_quiescent_with(&SystemProcessProbe)
    }

    pub fn ensure_quiescent_with<P: ProcessProbe>(&self, probe: &P) -> Result<(), String> {
        let pids = self.running_pids_with(probe)?;
        if let Some(pid) = pids.first() {
            return Err(format!(
                "target executable is still running (pid {pid}): {}; refusing to modify a live app",
                self.executable.display()
            ));
        }
        Ok(())
    }

    fn running_pids_with<P: ProcessProbe>(&self, probe: &P) -> Result<Vec<i32>, String> {
        Ok(probe
            .process_paths()?
            .into_iter()
            .filter_map(|(pid, path)| (path == self.executable).then_some(pid))
            .collect())
    }

    /// 对官方默认 app 发送 bundle-id AppleScript quit，并等待真实 executable 退出。
    pub fn quit_official_app_and_wait(&self) -> Result<(), String> {
        let mut requester = SystemQuitRequester;
        let mut clock = SystemQuiescenceClock;
        self.quit_official_app_and_wait_with(&SystemProcessProbe, &mut requester, &mut clock)
    }

    /// 可注入的官方退出流程；只替换探针、quit 请求和时钟，不改变安全语义。
    pub fn quit_official_app_and_wait_with<P, Q, C>(
        &self,
        probe: &P,
        requester: &mut Q,
        clock: &mut C,
    ) -> Result<(), String>
    where
        P: ProcessProbe,
        Q: QuitRequester,
        C: QuiescenceClock,
    {
        // 没有精确匹配的 executable 时，osascript 可能启动一个原本未运行的 App；
        // 先观察、后退出，避免无谓闪现与启动副作用。
        if self.running_pids_with(probe)?.is_empty() {
            return Ok(());
        }
        requester
            .request_quit()
            .map_err(|error| format!("failed to ask official Codex to quit: {error}"))?;
        let deadline = clock.now() + QUIESCENCE_TIMEOUT;
        loop {
            let pids = self.running_pids_with(probe)?;
            if pids.is_empty() {
                return Ok(());
            }
            if clock.now() >= deadline {
                return Err(format!(
                    "timed out waiting for official Codex executable to exit after {} seconds: {}",
                    QUIESCENCE_TIMEOUT.as_secs(),
                    self.executable.display()
                ));
            }
            clock.sleep(QUIESCENCE_POLL_INTERVAL);
        }
    }
}

/// 默认系统进程探针：先用 /bin/ps 取得 PID，再用 proc_pidpath 做完整路径匹配。
pub struct SystemProcessProbe;

impl ProcessProbe for SystemProcessProbe {
    fn process_paths(&self) -> Result<Vec<(i32, PathBuf)>, String> {
        let output = Command::new("/bin/ps")
            .args(["-axo", "pid="])
            .output()
            .map_err(|error| format!("cannot list macOS processes with /bin/ps: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "/bin/ps could not list macOS processes: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let mut processes = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let raw_pid = line.trim();
            if raw_pid.is_empty() {
                continue;
            }
            let pid = raw_pid
                .parse::<i32>()
                .map_err(|error| format!("/bin/ps returned an invalid PID {raw_pid:?}: {error}"))?;
            if let Some(path) = process_executable_path(pid) {
                processes.push((pid, path));
            }
        }
        Ok(processes)
    }
}

struct SystemQuitRequester;

impl QuitRequester for SystemQuitRequester {
    fn request_quit(&mut self) -> Result<(), String> {
        quit_official_app()
    }
}

struct SystemQuiescenceClock;

impl QuiescenceClock for SystemQuiescenceClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

#[cfg(target_os = "macos")]
fn process_executable_path(pid: i32) -> Option<PathBuf> {
    const MAX_PATH: usize = 4096;
    let mut buffer = vec![0u8; MAX_PATH];
    let size = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if size <= 0 || size as usize > buffer.len() {
        return None;
    }
    let mut length = size as usize;
    while length > 0 && buffer[length - 1] == 0 {
        length -= 1;
    }
    (length > 0).then(|| PathBuf::from(OsString::from_vec(buffer[..length].to_vec())))
}

#[cfg(not(target_os = "macos"))]
fn process_executable_path(_pid: i32) -> Option<PathBuf> {
    None
}

pub fn ditto(src: &Path, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        fs::remove_dir_all(dest).map_err(|err| err.to_string())?;
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let output = Command::new("ditto")
        .args([src, dest])
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

pub fn restore_original(source: &Path, dest: &Path) -> Result<(), String> {
    if !source.exists() {
        return Err(format!("original snapshot missing: {}", source.display()));
    }
    let staged = PathBuf::from(format!("{}.incodex-restore", dest.display()));
    ditto(source, &staged)?;
    let trash = PathBuf::from(format!("{}.incodex-uninstall", dest.display()));
    if trash.exists() {
        fs::remove_dir_all(&trash).map_err(|err| err.to_string())?;
    }
    if dest.exists() {
        fs::rename(dest, &trash).map_err(|err| err.to_string())?;
    }
    if let Err(err) = fs::rename(&staged, dest) {
        if trash.exists() {
            let _ = fs::rename(&trash, dest);
        }
        return Err(err.to_string());
    }
    if trash.exists() {
        fs::remove_dir_all(&trash).map_err(|err| err.to_string())?;
    }
    Ok(())
}

pub fn read_plist_info(app: &Path) -> Option<PlistInfo> {
    let plist = app.join("Contents").join("Info.plist");
    if !plist.exists() {
        return None;
    }
    let output = Command::new("plutil")
        .args(["-convert", "json", "-o", "-", "--"])
        .arg(&plist)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    Some(PlistInfo {
        bundle_identifier: json_string(&raw, "CFBundleIdentifier"),
        app_version: json_string(&raw, "CFBundleShortVersionString"),
        app_build: json_string(&raw, "CFBundleVersion"),
        executable: raw
            .get("CFBundleExecutable")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("ChatGPT")
            .to_string(),
    })
}

/// 读取 `CFBundleExecutable` 的严格版本，供启动路径使用。
pub fn read_plist_executable(app: &Path) -> Result<String, String> {
    let plist = app.join("Contents").join("Info.plist");
    if !plist.is_file() {
        return Err(format!("Info.plist not found: {}", plist.display()));
    }
    let output = Command::new("plutil")
        .args(["-convert", "json", "-o", "-", "--"])
        .arg(&plist)
        .output()
        .map_err(|error| format!("cannot read {}: {error}", plist.display()))?;
    if !output.status.success() {
        return Err(format!("Info.plist is invalid: {}", plist.display()));
    }
    let raw: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Info.plist is not valid JSON: {error}"))?;
    raw.get("CFBundleExecutable")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "Info.plist has no valid CFBundleExecutable: {}",
                plist.display()
            )
        })
}

pub fn read_architecture(app: &Path, executable: &str) -> Option<String> {
    let binary = app.join("Contents").join("MacOS").join(executable);
    let output = Command::new("lipo")
        .arg("-archs")
        .arg(binary)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut architectures: Vec<_> = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(str::to_string)
        .collect();
    architectures.sort();
    let joined = architectures.join(" ");
    (!joined.is_empty()).then_some(joined)
}

pub fn read_asar_integrity(app: &Path) -> Option<String> {
    let plist = app.join("Contents").join("Info.plist");
    let output = Command::new("plutil")
        .args(["-convert", "json", "-o", "-", "--"])
        .arg(plist)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    raw.pointer("/ElectronAsarIntegrity/Resources~1app.asar/hash")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub fn diagnose_spctl(app: &Path) -> serde_json::Value {
    let output = Command::new("spctl")
        .args(["--assess", "--verbose=4", "--"])
        .arg(app)
        .output();
    match output {
        Ok(output) => {
            let status = output.status.code().unwrap_or(1);
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .trim()
            .to_string();
            serde_json::json!({
                "status": status,
                "output": text,
                "accepted": output.status.success(),
                "usedAsSuccessGate": false,
            })
        }
        Err(error) => serde_json::json!({
            "status": 1,
            "output": error.to_string(),
            "accepted": false,
            "usedAsSuccessGate": false,
        }),
    }
}

fn json_string(raw: &serde_json::Value, key: &str) -> String {
    match raw.get(key) {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

pub fn write_asar_integrity(app: &Path, hash: &str) -> Result<(), String> {
    let plist = app.join("Contents").join("Info.plist");
    if !plist.exists() {
        return Ok(());
    }
    let payload = serde_json::json!({
        "Resources/app.asar": { "algorithm": "SHA256", "hash": hash }
    });
    let json = serde_json::to_string(&payload).map_err(|err| err.to_string())?;
    let mut failures = Vec::new();
    for flag in ["-replace", "-insert"] {
        let result = Command::new("plutil")
            .args([flag, "ElectronAsarIntegrity", "-json", &json])
            .arg(&plist)
            .status();
        match result {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => failures.push(format!("plutil {flag} exited with {status}")),
            Err(err) => failures.push(format!("plutil {flag} failed: {err}")),
        }
    }
    Err(format!(
        "failed to update ElectronAsarIntegrity in {}: {}",
        plist.display(),
        failures.join("; ")
    ))
}

pub fn quit_official_app() -> Result<(), String> {
    let script = r#"tell application id "com.openai.codex" to quit"#;
    let output = Command::new("osascript")
        .args(["-e", script])
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

pub fn front_codex_window_bounds() -> Option<(i32, i32, i32, i32)> {
    let script = r#"tell application "System Events" to tell first process whose bundle identifier is "com.openai.codex" to get {position, size} of front window"#;
    let output = Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_window_bounds_output(&String::from_utf8_lossy(&output.stdout))
}

pub fn tile_process_front_window(
    pid: u32,
    source: (i32, i32, i32, i32),
    offset: i32,
) -> Result<(), String> {
    let desired = (source.0 + offset, source.1 + offset, source.2, source.3);
    set_process_front_window_bounds(pid, desired)?;
    let actual = process_front_window_bounds(pid).ok_or("child window bounds unavailable")?;
    if actual.2 != source.2 || actual.3 != source.3 {
        set_process_front_window_bounds(pid, source)?;
    }
    Ok(())
}

fn process_front_window_bounds(pid: u32) -> Option<(i32, i32, i32, i32)> {
    let script = format!(
        "tell application \"System Events\" to tell first process whose unix id is {pid} to get {{position, size}} of front window"
    );
    let output = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_window_bounds_output(&String::from_utf8_lossy(&output.stdout))
}

fn set_process_front_window_bounds(pid: u32, bounds: (i32, i32, i32, i32)) -> Result<(), String> {
    let script = format!(
        "tell application \"System Events\" to tell first process whose unix id is {pid} to tell front window to set {{position, size}} to {{{{{}, {}}}, {{{}, {}}}}}",
        bounds.0, bounds.1, bounds.2, bounds.3
    );
    let output = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

fn parse_window_bounds_output(raw: &str) -> Option<(i32, i32, i32, i32)> {
    let values: Vec<i32> = raw
        .trim()
        .split(',')
        .map(str::trim)
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    (values.len() == 4).then(|| (values[0], values[1], values[2], values[3]))
}

pub fn notify_launch_services(app: &Path) -> Result<(), String> {
    let _ = Command::new("lsregister")
        .args(["-f", "-R", "-trusted"])
        .arg(app)
        .output();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn crate_compiles() {}

    #[test]
    fn parses_system_events_position_and_size() {
        assert_eq!(
            parse_window_bounds_output("0, 34, 1710, 1073\n"),
            Some((0, 34, 1710, 1073))
        );
        assert_eq!(parse_window_bounds_output("missing"), None);
    }

    #[test]
    fn notify_launch_services_captures_tool_output() {
        const CHILD: &str = "INCODEX_NOTIFY_LS_CHILD";
        if std::env::var_os(CHILD).is_some() {
            notify_launch_services(Path::new("/tmp/ChatGPT.app")).unwrap();
            return;
        }

        static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _path_lock = PATH_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "incodex-lsregister-output-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let fake = root.join("lsregister");
        std::fs::write(
            &fake,
            "#!/bin/sh\nprintf '%s\\n' LSREGISTER-OUT\nprintf '%s\\n' LSREGISTER-ERR >&2\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&fake).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake, permissions).unwrap();

        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::notify_launch_services_captures_tool_output",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("PATH", &root)
            .output()
            .unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stdout.contains("LSREGISTER-OUT"), "{stdout:?}");
        assert!(!stderr.contains("LSREGISTER-ERR"), "{stderr:?}");
    }
}

#[cfg(test)]
mod quiescence_tests {
    use super::*;
    use std::collections::VecDeque;
    use std::time::{Duration, Instant};

    struct FixtureProbe {
        paths: Vec<(i32, PathBuf)>,
    }

    impl ProcessProbe for FixtureProbe {
        fn process_paths(&self) -> Result<Vec<(i32, PathBuf)>, String> {
            Ok(self.paths.clone())
        }
    }

    struct FailingQuit;

    impl QuitRequester for FailingQuit {
        fn request_quit(&mut self) -> Result<(), String> {
            Err("fixture quit failed".into())
        }
    }

    struct FakeClock {
        now: Instant,
        sleeps: VecDeque<Duration>,
    }

    impl QuiescenceClock for FakeClock {
        fn now(&self) -> Instant {
            self.now
        }

        fn sleep(&mut self, duration: Duration) {
            self.now += duration;
            self.sleeps.push_back(duration);
        }
    }

    struct SuccessfulQuit;

    impl QuitRequester for SuccessfulQuit {
        fn request_quit(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn explicit_app_uses_future_cf_bundle_executable() {
        let root = std::env::temp_dir().join(format!(
            "incodex-quiescence-plist-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let app = root.join("Future.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::write(
            app.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>CFBundleExecutable</key><string>FutureCodex</string></dict></plist>
"#,
        )
        .unwrap();
        fs::write(app.join("Contents/MacOS/FutureCodex"), b"fixture").unwrap();

        let quiescence = AppQuiescence::for_app(&app).unwrap();
        assert_eq!(
            quiescence.executable(),
            fs::canonicalize(app.join("Contents/MacOS/FutureCodex")).unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exact_executable_path_does_not_match_same_name_elsewhere() {
        let expected = PathBuf::from("/tmp/incodex/ChatGPT.app/Contents/MacOS/ChatGPT");
        let quiescence = AppQuiescence::from_executable(expected.clone()).unwrap();
        let probe = FixtureProbe {
            paths: vec![(
                42,
                PathBuf::from("/tmp/other/ChatGPT.app/Contents/MacOS/ChatGPT"),
            )],
        };
        quiescence.ensure_quiescent_with(&probe).unwrap();
    }

    #[test]
    fn official_quit_error_is_propagated() {
        let expected = PathBuf::from("/tmp/incodex/ChatGPT.app/Contents/MacOS/ChatGPT");
        let quiescence = AppQuiescence::from_executable(expected).unwrap();
        let probe = FixtureProbe {
            paths: vec![(42, quiescence.executable().to_path_buf())],
        };
        let mut requester = FailingQuit;
        let mut clock = FakeClock {
            now: Instant::now(),
            sleeps: VecDeque::new(),
        };
        let error = quiescence
            .quit_official_app_and_wait_with(&probe, &mut requester, &mut clock)
            .unwrap_err();
        assert!(error.contains("fixture quit failed"), "{error}");
        assert!(clock.sleeps.is_empty());
    }

    #[test]
    fn official_quit_timeout_is_reported() {
        let expected = PathBuf::from("/tmp/incodex/ChatGPT.app/Contents/MacOS/ChatGPT");
        let quiescence = AppQuiescence::from_executable(expected).unwrap();
        let probe = FixtureProbe {
            paths: vec![(42, quiescence.executable().to_path_buf())],
        };
        let mut requester = SuccessfulQuit;
        let start = Instant::now();
        let mut clock = FakeClock {
            now: start,
            sleeps: VecDeque::new(),
        };
        let error = quiescence
            .quit_official_app_and_wait_with(&probe, &mut requester, &mut clock)
            .unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        assert!(!clock.sleeps.is_empty());
    }
}
