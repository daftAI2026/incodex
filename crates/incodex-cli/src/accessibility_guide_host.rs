/*
 * [INPUT]: 依赖已验证 native host、权限状态机、宿主探针与更新协调日志。
 * [OUTPUT]: 提供有界 stdio、安装前置检查与独立 AX/录屏 reset 接口。
 * [POS]: 原生卡可信传输；安装与运行中引导共用状态机，UI 不执行 reset。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
//! 短期原生卡片只表达用户选择。
//! CLI 持有设置导航与 reset；有界传输拒绝任意命令。

#[cfg(test)]
#[path = "permission_guide_rows_tests.rs"]
mod permission_guide_rows_tests;

use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

#[path = "permission_guide_flow.rs"]
mod flow;
use crate::accessibility_target::supports_permission_continuity;
use flow::run_permission_guide_with;
#[path = "permission_guide_system.rs"]
mod system;
#[cfg(test)]
pub(crate) use flow::{
    run_permission_guide_with_limits, run_permission_guide_with_timeouts, GuideTimeouts,
};
pub(crate) use flow::{PermissionSnapshot, ScreenPermissionStatus};
use system::SystemGuideOps;
#[path = "permission_guide_copy.rs"]
mod copy;
use copy::*;
use incodex_macos::AccessibilityStatus;
use serde_json::{json, Value};

pub(crate) const HOST_EXECUTABLE_NAME: &str = "incodex-permission-host";
const PERMISSION_COPY_NAME: &str = "incodex-permission-copy.json";
pub(crate) const OFFICIAL_APP_PATH: &str = "/Applications/ChatGPT.app";
pub(crate) const MAX_HOST_LINE_BYTES: usize = 64 * 1024;
// AppKit may take longer to initialize on a cold system. This pre-configure
// locale handshake is bounded independently from visible-window readiness.
const HOST_LOCALE_TIMEOUT: Duration = Duration::from_secs(5);
// Initial native-window construction may wait for AppKit to attach to the
// restored Electron process. Machine readiness is bounded independently;
// the user's Settings approval has no authorization countdown.
pub(crate) const HOST_READY_TIMEOUT: Duration = Duration::from_secs(5 * 60);

const CHILD_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const CHILD_EXIT_GRACE: Duration = Duration::from_millis(100);
const CODESIGN_VERIFY_TIMEOUT: Duration = Duration::from_secs(5);
const INITIAL_PROBE_ATTEMPTS: usize = 120;
const INITIAL_PROBE_INTERVAL: Duration = Duration::from_millis(250);
const POST_ALLOW_PROBE_INTERVAL: Duration = Duration::from_millis(750);
const ACCESSIBILITY_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Granted,
    Pending,
    AwaitingEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuideTraceEvent {
    PreflightGranted,
    PreflightUnknown,
    CardReady,
    UserSkip,
    UserClose,
    HostEof,
    MachineInterrupted,
    CardCompleted,
}

impl GuideTraceEvent {
    pub(crate) fn log_event(self) -> &'static str {
        match self {
            Self::PreflightGranted => "permission-guide preflight-granted",
            Self::PreflightUnknown => "permission-guide preflight-unknown",
            Self::CardReady => "permission-guide card-ready",
            Self::UserSkip => "permission-guide user-skip",
            Self::UserClose => "permission-guide user-close",
            Self::HostEof => "permission-guide host-eof",
            Self::MachineInterrupted => "permission-guide machine-interrupted",
            Self::CardCompleted => "permission-guide card-completed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuideCopyContext {
    Installed,
    Installing,
    Official,
}

struct NativeGuideConfig {
    copy: Value,
    layout_direction: &'static str,
}

/// Operations that remain in the CLI process.  In particular, the Runtime
/// host cannot reset TCC or open Settings on the CLI's behalf.
pub(crate) trait GuideOps {
    fn launch(&mut self) -> Result<(), String>;
    fn probe(&mut self) -> AccessibilityStatus;
    fn observe(&mut self, _event: GuideTraceEvent) {}
    // 缺少录屏适配保持未知。None 仅供显式 AX 单行协议夹具。
    fn probe_screen(&mut self) -> Option<ScreenPermissionStatus> {
        Some(ScreenPermissionStatus::Unknown)
    }
    // 单行适配可复用已读 AX。生产适配必须重读同进程快照。
    fn snapshot_after_ax(&mut self, accessibility: AccessibilityStatus) -> PermissionSnapshot {
        PermissionSnapshot {
            accessibility,
            screen: self.probe_screen(),
        }
    }
    fn snapshot(&mut self) -> PermissionSnapshot {
        let ax = self.probe();
        self.snapshot_after_ax(ax)
    }
    fn record_screen_intent(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn reset_screen(&mut self) -> Result<(), String> {
        Err("Screen Recording reset is unavailable".into())
    }
    fn open_screen_settings(&mut self) -> Result<(), String> {
        Err("Screen Recording settings are unavailable".into())
    }
    fn wait_for_window(&mut self) -> Result<(), String>;
    fn lock_target(&mut self) -> Result<Option<incodex_transaction::TargetLock>, String> {
        Ok(None)
    }
    fn reset(&mut self) -> Result<(), String>;
    fn open_settings(&mut self) -> Result<(), String>;
    fn wait(&mut self, duration: Duration);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HostState {
    Repairing,
    AwaitingUser,
    Granted,
    Error(String),
}

impl HostState {
    fn as_str(&self) -> (&'static str, Option<&str>) {
        match self {
            Self::Repairing => ("repairing", None),
            Self::AwaitingUser => ("awaiting-user", None),
            Self::Granted => ("granted", None),
            Self::Error(message) => ("error", Some(message.as_str())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HostEvent {
    AppLocale(String),
    Ready,
    Allow,
    Retry,
    AllowScreen,
    RetryScreen,
    Later,
    Close,
    Error(String),
    Eof,
    Timeout,
}

/// A transport abstraction keeps the renewal state machine deterministic in
/// unit tests while the production implementation remains a real child
/// process connected only through private stdin/stdout.
pub(crate) trait GuideHost {
    fn send_state(&mut self, state: HostState) -> Result<(), String>;
    fn send_permissions(&mut self, _snapshot: &PermissionSnapshot) -> Result<(), String> {
        Ok(())
    }
    fn send_permission_state(&mut self, permission: &str, state: HostState) -> Result<(), String> {
        if permission == "accessibility" {
            self.send_state(state)
        } else {
            Err("Screen Recording presentation is unavailable".into())
        }
    }
    fn poll(&mut self, timeout: Duration) -> Result<HostEvent, String>;
    fn close(&mut self);
}

pub(crate) trait GuideHostFactory {
    fn start(&mut self, root: &Path, app: &Path) -> Result<Box<dyn GuideHost>, String>;
    fn start_with_permissions(
        &mut self,
        root: &Path,
        app: &Path,
        _permissions: &PermissionSnapshot,
    ) -> Result<Box<dyn GuideHost>, String> {
        self.start(root, app)
    }
}

pub(crate) struct ProcessGuideFactory {
    copy_context: GuideCopyContext,
}

impl GuideHostFactory for ProcessGuideFactory {
    fn start(&mut self, root: &Path, app: &Path) -> Result<Box<dyn GuideHost>, String> {
        let canonical_app = fs::canonicalize(app).map_err(|error| {
            format!("cannot resolve native Accessibility guide app path: {error}")
        })?;
        if canonical_app != Path::new(OFFICIAL_APP_PATH) {
            return Err(format!(
                "native Accessibility guide requires the default app path: {}",
                OFFICIAL_APP_PATH
            ));
        }
        Ok(Box::new(ProcessGuideHost::spawn(root, self.copy_context)?))
    }

    fn start_with_permissions(
        &mut self,
        root: &Path,
        app: &Path,
        permissions: &PermissionSnapshot,
    ) -> Result<Box<dyn GuideHost>, String> {
        if fs::canonicalize(app).map_err(|error| error.to_string())? != Path::new(OFFICIAL_APP_PATH)
        {
            return Err("native permission guide requires the default app path".into());
        }
        let executable = verified_native_host_path(root)?;
        Ok(Box::new(
            ProcessGuideHost::spawn_executable_with_permissions(
                &executable,
                self.copy_context,
                Some(permissions),
            )?,
        ))
    }
}

/// Run the one-shot guide for either the restored official app or an already
/// validated patched app.  The caller owns its transaction lock and supplies a
/// full verifier before launch. On APFS, a fresh continuity check then binds
/// Allow to that same verified tree; other filesystems retain full revalidation.
/// The exact running process is still audited immediately before any reset.
pub(crate) fn run_permission_guide<F>(
    root: &Path,
    app: &Path,
    copy_context: GuideCopyContext,
    verify_target: F,
) -> Result<Outcome, String>
where
    F: FnMut() -> Result<(), String>,
{
    let mut ops = SystemGuideOps {
        root,
        app,
        screen: if copy_context == GuideCopyContext::Installed {
            crate::permission_screen_probe::InstalledScreenProbe::for_app(root, app)
        } else {
            None
        },
    };
    let mut factory = ProcessGuideFactory { copy_context };
    let mut verify_target = crate::accessibility_target::verifier(
        app,
        supports_permission_continuity(app),
        verify_target,
    );
    run_permission_guide_with(&mut ops, root, app, &mut verify_target, &mut factory)
}

#[path = "permission_guide_update.rs"]
mod update;
pub(crate) use update::run as run_permission_update_guide;

fn open_settings_command(url: &str) -> Command {
    let mut command = Command::new("/usr/bin/open");
    command.args(["-g", url]);
    command
}

fn bounded_command(command: &mut Command) -> Result<(), String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("{program}: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("{program} exited with {status}")),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} timed out"));
            }
        }
    }
}

struct ProcessGuideHost {
    child: Child,
    stdin: Option<ChildStdin>,
    events: Receiver<ReaderEvent>,
    nonce: String,
    closed: bool,
}

enum ReaderEvent {
    Line(Vec<u8>),
    Oversized,
    ReadError(String),
    Eof,
}

impl ProcessGuideHost {
    fn spawn(root: &Path, copy_context: GuideCopyContext) -> Result<Self, String> {
        let executable = verified_native_host_path(root)?;
        Self::spawn_executable(&executable, copy_context)
    }

    #[cfg(test)]
    fn spawn_for_test(executable: &Path, copy_context: GuideCopyContext) -> Result<Self, String> {
        Self::spawn_executable(executable, copy_context)
    }

    fn spawn_executable(executable: &Path, copy_context: GuideCopyContext) -> Result<Self, String> {
        Self::spawn_executable_with_permissions(executable, copy_context, None)
    }

    fn spawn_executable_with_permissions(
        executable: &Path,
        copy_context: GuideCopyContext,
        permissions: Option<&PermissionSnapshot>,
    ) -> Result<Self, String> {
        let nonce = new_nonce()?;

        let mut child = Command::new(executable)
            .arg("--nonce")
            .arg(&nonce)
            // The native host is trusted only through the verified Runtime
            // release. Do not let a parent process injection setting alter
            // this short-lived process.
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH")
            .env_remove("ELECTRON_RUN_AS_NODE")
            .env_remove("DYLD_INSERT_LIBRARIES")
            .env_remove("DYLD_LIBRARY_PATH")
            .env_remove("DYLD_FRAMEWORK_PATH")
            .env_remove("DYLD_FALLBACK_LIBRARY_PATH")
            .env_remove("DYLD_FALLBACK_FRAMEWORK_PATH")
            .env_remove("DYLD_ROOT_PATH")
            .env_remove("DYLD_SHARED_REGION")
            .env_remove("DYLD_IMAGE_SUFFIX")
            .env_remove("DYLD_PRINT_LIBRARIES")
            .env_remove("DYLD_PRINT_APIS")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("cannot start native Accessibility guide: {error}"))?;
        let stdin = child.stdin.take().ok_or_else(|| {
            abort_child(&mut child);
            "native Accessibility guide stdin was not available".to_string()
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            abort_child(&mut child);
            "native Accessibility guide stdout was not available".to_string()
        })?;
        if let Err(error) = set_nonblocking(&stdin) {
            abort_child(&mut child);
            return Err(error);
        }
        let (sender, events) = mpsc::sync_channel(8);
        if let Err(error) = thread::Builder::new()
            .name("incodex-permission-host-reader".into())
            .spawn(move || read_host_lines(stdout, sender))
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "cannot start native Accessibility guide reader: {error}"
            ));
        }

        let mut host = Self {
            child,
            stdin: Some(stdin),
            events,
            nonce,
            closed: false,
        };
        host.send_json(json!({ "nonce": host.nonce, "type": "app-locale-request" }))?;
        let app_locale = match host.poll(HOST_LOCALE_TIMEOUT)? {
            HostEvent::AppLocale(locale) => locale,
            HostEvent::Error(message) => {
                return Err(format!(
                    "native Accessibility guide locale probe failed: {message}"
                ))
            }
            other => {
                return Err(format!(
                    "native Accessibility guide did not report the target app locale: {other:?}"
                ))
            }
        };
        let config = native_guide_config(copy_context, Some(&app_locale))?;
        let mut message = json!({"nonce":host.nonce, "type":"configure", "copy":config.copy, "layoutDirection":config.layout_direction});
        if let Some(permissions) = permissions.filter(|snapshot| snapshot.screen.is_some()) {
            message["permissions"] = permissions.json();
        }
        if copy_context == GuideCopyContext::Installing {
            message["presentationContext"] = "installation".into();
        }
        host.send_json(message)?;
        Ok(host)
    }

    fn send_json(&mut self, value: Value) -> Result<(), String> {
        let body = serde_json::to_vec(&value).map_err(|error| {
            format!("cannot encode native Accessibility guide message: {error}")
        })?;
        if body.len() > MAX_HOST_LINE_BYTES {
            return Err("native Accessibility guide message is too large".into());
        }
        let stdin = self
            .stdin
            .as_mut()
            .ok_or("native Accessibility guide stdin is closed")?;
        write_bounded(stdin, &body)?;
        write_bounded(stdin, b"\n")?;
        flush_bounded(stdin)
    }

    fn child_status(&mut self) -> Result<Option<ExitStatus>, String> {
        self.child
            .try_wait()
            .map_err(|error| format!("native Accessibility guide status failed: {error}"))
    }

    fn eof_event(&mut self) -> Result<HostEvent, String> {
        let deadline = Instant::now() + CHILD_EXIT_GRACE;
        loop {
            if let Some(status) = self.child_status()? {
                return Ok(exit_status_event(status));
            }
            if Instant::now() >= deadline {
                return Ok(HostEvent::Eof);
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}

fn exit_status_event(status: ExitStatus) -> HostEvent {
    if status.success() {
        HostEvent::Eof
    } else {
        HostEvent::Error(format!(
            "native Accessibility guide exited unsuccessfully: {status}"
        ))
    }
}

impl GuideHost for ProcessGuideHost {
    fn send_permissions(&mut self, snapshot: &PermissionSnapshot) -> Result<(), String> {
        if snapshot.screen.is_none() {
            return Ok(());
        }
        self.send_json(
            json!({"nonce":self.nonce, "type":"permissions", "permissions":snapshot.json()}),
        )
    }

    fn send_permission_state(&mut self, permission: &str, state: HostState) -> Result<(), String> {
        let (state, message) = state.as_str();
        let mut value =
            json!({"nonce":self.nonce,"type":"state","state":state,"permission":permission});
        if let Some(message) = message {
            value["message"] = message.into();
        }
        self.send_json(value)
    }
    fn send_state(&mut self, state: HostState) -> Result<(), String> {
        let (state, message) = state.as_str();
        let mut value = json!({
            "nonce": self.nonce,
            "type": "state",
            "state": state,
        });
        if let Some(message) = message {
            value["message"] = Value::String(message.to_string());
        }
        self.send_json(value)
    }

    fn poll(&mut self, timeout: Duration) -> Result<HostEvent, String> {
        match self.events.recv_timeout(timeout) {
            Ok(ReaderEvent::Line(line)) => decode_host_event(&line, &self.nonce),
            Ok(ReaderEvent::Oversized) => Ok(HostEvent::Error(
                "native Accessibility guide message is too large".into(),
            )),
            Ok(ReaderEvent::ReadError(error)) => Ok(HostEvent::Error(format!(
                "native Accessibility guide output failed: {error}"
            ))),
            Ok(ReaderEvent::Eof) => self.eof_event(),
            Err(RecvTimeoutError::Timeout) => Ok(self
                .child_status()?
                .map_or(HostEvent::Timeout, exit_status_event)),
            Err(RecvTimeoutError::Disconnected) => self.eof_event(),
        }
    }

    fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        if let Some(mut stdin) = self.stdin.take() {
            let message = json!({ "nonce": self.nonce, "type": "close" });
            if let Ok(body) = serde_json::to_vec(&message) {
                let _ = write_bounded(&mut stdin, &body);
                let _ = write_bounded(&mut stdin, b"\n");
                let _ = stdin.flush();
            }
        }
        reap_child(&mut self.child);
    }
}

impl Drop for ProcessGuideHost {
    fn drop(&mut self) {
        self.close();
    }
}

fn read_host_lines(mut stdout: impl Read, sender: SyncSender<ReaderEvent>) {
    let mut line = Vec::with_capacity(256);
    let mut byte = [0_u8; 1];
    loop {
        match stdout.read(&mut byte) {
            Ok(0) => {
                let _ = sender.send(ReaderEvent::Eof);
                return;
            }
            Ok(1) if byte[0] == b'\n' => {
                let body = std::mem::take(&mut line);
                if sender.send(ReaderEvent::Line(body)).is_err() {
                    return;
                }
            }
            Ok(1) => {
                line.push(byte[0]);
                if line.len() > MAX_HOST_LINE_BYTES {
                    let _ = sender.send(ReaderEvent::Oversized);
                    return;
                }
            }
            Ok(_) => unreachable!("one-byte read returned more than one byte"),
            Err(error) => {
                let _ = sender.send(ReaderEvent::ReadError(error.to_string()));
                return;
            }
        }
    }
}

fn set_nonblocking(stdin: &ChildStdin) -> Result<(), String> {
    let fd = stdin.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err("cannot inspect native Accessibility guide stdin flags".into());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err("cannot bound native Accessibility guide stdin writes".into());
    }
    Ok(())
}

fn write_bounded(writer: &mut impl Write, body: &[u8]) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut offset = 0;
    while offset < body.len() {
        match writer.write(&body[offset..]) {
            Ok(0) => return Err("native Accessibility guide stdin closed".into()),
            Ok(written) => offset += written,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err("native Accessibility guide stdin write timed out".into());
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(format!("native Accessibility guide stdin failed: {error}")),
        }
    }
    Ok(())
}

fn flush_bounded(writer: &mut impl Write) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match writer.flush() {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err("native Accessibility guide stdin flush timed out".into());
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(format!("native Accessibility guide stdin failed: {error}")),
        }
    }
}

fn decode_host_event(line: &[u8], nonce: &str) -> Result<HostEvent, String> {
    let value: Value = serde_json::from_slice(line)
        .map_err(|error| format!("invalid native Accessibility guide message: {error}"))?;
    let object = value
        .as_object()
        .ok_or("native Accessibility guide message is not an object")?;
    if object.get("nonce").and_then(Value::as_str) != Some(nonce) {
        return Err("native Accessibility guide nonce mismatch".into());
    }
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or("native Accessibility guide message type is missing")?;
    match kind {
        "app-locale" => {
            if object.len() != 3 {
                return Err("native Accessibility guide locale message has extra fields".into());
            }
            let locale = object
                .get("locale")
                .and_then(Value::as_str)
                .filter(|value| {
                    !value.is_empty()
                        && value.len() <= 64
                        && value.as_bytes()[0].is_ascii_alphabetic()
                        && value.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
                        })
                })
                .ok_or("native Accessibility guide target app locale is invalid")?;
            Ok(HostEvent::AppLocale(locale.into()))
        }
        "ready" => Ok(HostEvent::Ready),
        "allow" if object.len() == 2 => Ok(HostEvent::Allow),
        "allow-screen" if object.len() == 2 => Ok(HostEvent::AllowScreen),
        "retry-screen" if object.len() == 2 => Ok(HostEvent::RetryScreen),
        "retry" if object.len() == 2 => Ok(HostEvent::Retry),
        "later" => Ok(HostEvent::Later),
        "close" => Ok(HostEvent::Close),
        "error" => Ok(HostEvent::Error(
            object
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("native Accessibility guide reported an error")
                .to_string(),
        )),
        other => Err(format!(
            "native Accessibility guide message type is unsupported: {other}"
        )),
    }
}

fn verified_native_host_path(root: &Path) -> Result<PathBuf, String> {
    let identity = incodex_runtime_bundle::runtime_identity()?;
    // 原生引导只消费已发布资产。旧 CLI 不得用 ensure_current 倒退新选择。
    let deployed = incodex_runtime_bundle::inspect_deployed(root)?.ok_or(
        "Runtime is missing for the native permission guide; publish it with the current CLI first",
    )?;
    if !identity.matches(&deployed) {
        return Err("published Runtime identity changed during guide startup".into());
    }
    let path = root
        .join("runtime")
        .join(&deployed.release)
        .join(HOST_EXECUTABLE_NAME);
    let metadata = fs::symlink_metadata(&path).map_err(|error| {
        format!(
            "native Accessibility guide executable is missing from the verified Runtime: {error}"
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err("native Accessibility guide executable is not a regular file".into());
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err("native Accessibility guide executable is not executable".into());
    }
    let canonical = fs::canonicalize(&path).map_err(|error| {
        format!("cannot resolve native Accessibility guide executable: {error}")
    })?;
    if canonical != path {
        return Err("native Accessibility guide executable path changed during validation".into());
    }
    verify_codesign(&canonical)?;
    Ok(canonical)
}

fn verify_codesign(path: &Path) -> Result<(), String> {
    let mut child = Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict", "--verbose=0", "--"])
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot verify native Accessibility guide signature: {error}"))?;
    let deadline = Instant::now() + CODESIGN_VERIFY_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!(
                    "native Accessibility guide signature failed: {status}"
                ))
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("native Accessibility guide signature verification timed out".into());
            }
        }
    }
}

fn new_nonce() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|error| format!("cannot create native Accessibility guide nonce: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn abort_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn reap_child(child: &mut Child) {
    let deadline = Instant::now() + CHILD_CLEANUP_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
        }
    }
}

#[cfg(test)]
#[path = "accessibility_guide_host_tests.rs"]
mod tests;

#[path = "permission_guide_install.rs"]
mod install;
pub(crate) use install::{
    launch_after_install, run as run_install_permission_guide, supports_preflight,
};
