//! Private stdio transport for the short-lived native Accessibility guide.
//!
//! The short-lived native executable host is only a UI host.  It never performs a
//! TCC reset or opens Settings; those operations stay in the CLI.  This module
//! deliberately keeps the transport small and bounded so a broken/hostile host cannot
//! make `uninstall` wait forever or turn arbitrary output into a command.

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

use incodex_macos::AccessibilityStatus;
use serde_json::{json, Value};

pub(crate) const HOST_EXECUTABLE_NAME: &str = "incodex-permission-host";
const PERMISSION_COPY_NAME: &str = "incodex-permission-copy.json";
pub(crate) const OFFICIAL_APP_PATH: &str = "/Applications/ChatGPT.app";
pub(crate) const MAX_HOST_LINE_BYTES: usize = 64 * 1024;
// Initial native-window construction may wait for AppKit to attach to the
// restored Electron process.  This is separate from the two-minute user
// handoff window below and must not be confused with a Settings timeout.
pub(crate) const HOST_READY_TIMEOUT: Duration = Duration::from_secs(5 * 60);
// Once the native view is ready, keep the user's Allow/Back/Later choice
// bounded independently from the post-Allow Settings polling window.
pub(crate) const HOST_CHOICE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub(crate) const HOST_GUIDE_TIMEOUT: Duration = Duration::from_secs(120);

const CHILD_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const CHILD_EXIT_GRACE: Duration = Duration::from_millis(100);
const CODESIGN_VERIFY_TIMEOUT: Duration = Duration::from_secs(5);
const INITIAL_PROBE_ATTEMPTS: usize = 120;
const INITIAL_PROBE_INTERVAL: Duration = Duration::from_millis(250);
const POST_ALLOW_PROBE_INTERVAL: Duration = Duration::from_millis(750);
const ACCESSIBILITY_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Granted,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuideCopyContext {
    Installed,
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
    fn wait_for_window(&mut self) -> Result<(), String>;
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
    Ready,
    Allow,
    Retry,
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
    fn poll(&mut self, timeout: Duration) -> Result<HostEvent, String>;
    fn close(&mut self);
}

pub(crate) trait GuideHostFactory {
    fn start(&mut self, root: &Path, app: &Path) -> Result<Box<dyn GuideHost>, String>;
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
    let mut ops = SystemGuideOps { app };
    let mut factory = ProcessGuideFactory { copy_context };
    let mut verify_target = crate::accessibility_target::verifier(
        app,
        supports_permission_continuity(app),
        verify_target,
    );
    run_permission_guide_with(
        &mut ops,
        root,
        app,
        &mut verify_target,
        &mut factory,
        HOST_GUIDE_TIMEOUT,
    )
}

fn supports_permission_continuity(app: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::{
            ffi::{CStr, CString},
            os::unix::ffi::OsStrExt,
        };
        let Ok(path) = CString::new(app.as_os_str().as_bytes()) else {
            return false;
        };
        let mut info: libc::statfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statfs(path.as_ptr(), &mut info) } != 0 {
            return false;
        }
        // Do not rely on coarse or remotely supplied change timestamps.
        unsafe { CStr::from_ptr(info.f_fstypename.as_ptr()) }.to_bytes() == b"apfs"
            && info.f_flags & libc::MNT_LOCAL as u32 != 0
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        false
    }
}

pub(crate) fn run_permission_guide_with<O, F, H>(
    ops: &mut O,
    root: &Path,
    app: &Path,
    verify_target: &mut F,
    factory: &mut H,
    guide_timeout: Duration,
) -> Result<Outcome, String>
where
    O: GuideOps,
    F: FnMut() -> Result<(), String>,
    H: GuideHostFactory,
{
    run_permission_guide_with_timeouts(
        ops,
        root,
        app,
        verify_target,
        factory,
        HOST_CHOICE_TIMEOUT,
        guide_timeout,
    )
}

pub(crate) fn run_permission_guide_with_timeouts<O, F, H>(
    ops: &mut O,
    root: &Path,
    app: &Path,
    verify_target: &mut F,
    factory: &mut H,
    choice_timeout: Duration,
    guide_timeout: Duration,
) -> Result<Outcome, String>
where
    O: GuideOps,
    F: FnMut() -> Result<(), String>,
    H: GuideHostFactory,
{
    verify_target()?;
    ops.launch()?;
    if wait_for_decision(ops)? == AccessibilityStatus::Granted {
        return Ok(Outcome::Granted);
    }

    // Electron can publish a PID before its first main window exists.  Do not
    // present the native guide or touch TCC until the restored target is
    // actually presentable and its identity has been re-probed.
    ops.wait_for_window()?;
    if wait_for_decision(ops)? == AccessibilityStatus::Granted {
        return Ok(Outcome::Granted);
    }

    let mut host = factory
        .start(root, app)
        .map_err(|error| format!("native Accessibility guide could not start: {error}"))?;
    let ready_deadline = Instant::now() + HOST_READY_TIMEOUT;
    let mut ready = false;
    while Instant::now() < ready_deadline {
        match host.poll(Duration::from_millis(250))? {
            HostEvent::Ready => {
                ready = true;
                break;
            }
            HostEvent::Timeout => {
                if ops.probe() == AccessibilityStatus::Granted {
                    let _ = host.send_state(HostState::Granted);
                    host.close();
                    return Ok(Outcome::Granted);
                }
            }
            HostEvent::Later | HostEvent::Close | HostEvent::Eof => {
                host.close();
                return Ok(Outcome::Pending);
            }
            HostEvent::Error(message) => {
                host.close();
                return Err(format!(
                    "native Accessibility guide failed before becoming ready: {message}"
                ));
            }
            HostEvent::Allow | HostEvent::Retry => {
                let _ = host.send_state(HostState::Error(
                    "native Accessibility guide sent a choice before ready".into(),
                ));
                host.close();
                return Ok(Outcome::Pending);
            }
        }
    }
    if !ready {
        let _ = host.send_state(HostState::Error(
            "native Accessibility guide did not become ready in time".into(),
        ));
        host.close();
        return Ok(Outcome::Pending);
    }
    // Do not charge authentication/native-window presentation time against
    // the post-Allow Accessibility polling window.  A separate choice
    // deadline prevents a native view that never receives a decision from
    // living forever, while a later Retry does not renew the post-Allow
    // deadline.
    let choice_deadline = Instant::now() + choice_timeout;
    let mut guide_deadline = None;
    let mut reset_performed = false;
    loop {
        let deadline = guide_deadline.unwrap_or(choice_deadline);
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let poll_timeout = (deadline - now).min(Duration::from_millis(250));
        let event = host.poll(poll_timeout)?;
        // A misbehaving host may return an event after the bounded poll
        // interval.  Do not accept a late Allow/Retry from the pre-choice
        // phase (or silently extend the post-Allow phase).
        if Instant::now() >= deadline {
            break;
        }
        match event {
            HostEvent::Ready | HostEvent::Timeout => {}
            HostEvent::Allow if !reset_performed => {
                // The native guide only expresses user intent.  Revalidate
                // the target in the CLI immediately before the destructive
                // operation, then perform that operation exactly once.  Tell
                // the user that verification is in progress first. Local
                // APFS uses the session's verified-target continuity guard;
                // unsupported targets retain the full verifier fallback.
                host.send_state(HostState::Repairing)?;
                verify_target()?;
                // A user may have approved the system row while the guide was
                // animating.  Only an explicit denied probe authorizes reset;
                // unknown/not-running is an identity/readiness failure.
                // Keep this probe and reset adjacent: no UI handoff or slow
                // signature inventory belongs between the two operations.
                match ops.probe() {
                    AccessibilityStatus::Granted => {
                        host.send_state(HostState::Granted)?;
                        host.close();
                        return Ok(Outcome::Granted);
                    }
                    AccessibilityStatus::Denied => {}
                    AccessibilityStatus::Unknown | AccessibilityStatus::NotRunning => {
                        let _ = host.send_state(HostState::Error(
                            "Accessibility status was not a confirmed denial; no reset was performed".into(),
                        ));
                        host.close();
                        return Ok(Outcome::Pending);
                    }
                }
                ops.reset()?;
                reset_performed = true;
                ops.open_settings()?;
                // Only charge the two-minute handoff window once Settings
                // has actually opened.  Target revalidation and TCC reset
                // are synchronous CLI work after the user's choice and must
                // not consume the system-approval polling budget.
                guide_deadline = Some(Instant::now() + guide_timeout);
                host.send_state(HostState::AwaitingUser)?;
            }
            HostEvent::Allow => {
                let _ = host.send_state(HostState::Error(
                    "native Accessibility guide repeated Allow after reset".into(),
                ));
                host.close();
                return Ok(Outcome::Pending);
            }
            HostEvent::Retry if reset_performed => {
                // Back -> Allow is a retry of the system handoff, not another
                // TCC reset.  Settings is reopened and the same app is probed.
                host.send_state(HostState::Repairing)?;
                ops.open_settings()?;
                host.send_state(HostState::AwaitingUser)?;
            }
            HostEvent::Retry => {
                let _ = host.send_state(HostState::Error(
                    "native Accessibility guide sent Retry before initial Allow".into(),
                ));
                host.close();
                return Ok(Outcome::Pending);
            }
            HostEvent::Later | HostEvent::Close | HostEvent::Eof => {
                host.close();
                return Ok(Outcome::Pending);
            }
            HostEvent::Error(message) => {
                host.close();
                return Err(format!(
                    "native Accessibility guide reported an error: {message}"
                ));
            }
        }

        if ops.probe() == AccessibilityStatus::Granted {
            host.send_state(HostState::Granted)?;
            host.close();
            return Ok(Outcome::Granted);
        }
        ops.wait(if reset_performed {
            POST_ALLOW_PROBE_INTERVAL
        } else {
            INITIAL_PROBE_INTERVAL
        });
    }

    let _ = host.send_state(HostState::Error(if reset_performed {
        "native Accessibility guide timed out before Accessibility was granted".into()
    } else {
        "native Accessibility guide did not receive a user choice in time".into()
    }));
    host.close();
    Ok(Outcome::Pending)
}

fn wait_for_decision(ops: &mut impl GuideOps) -> Result<AccessibilityStatus, String> {
    for _ in 0..INITIAL_PROBE_ATTEMPTS {
        match ops.probe() {
            status @ (AccessibilityStatus::Granted | AccessibilityStatus::Denied) => {
                return Ok(status)
            }
            AccessibilityStatus::Unknown | AccessibilityStatus::NotRunning => {
                ops.wait(INITIAL_PROBE_INTERVAL)
            }
        }
    }
    Err("The target app's identity or Accessibility permission remained unavailable; no reset was performed.".into())
}

struct SystemGuideOps<'a> {
    app: &'a Path,
}

impl GuideOps for SystemGuideOps<'_> {
    fn launch(&mut self) -> Result<(), String> {
        bounded_command(Command::new("/usr/bin/open").arg(self.app))
    }

    fn probe(&mut self) -> AccessibilityStatus {
        incodex_macos::inspect_accessibility_for_app(self.app).status
    }

    fn wait_for_window(&mut self) -> Result<(), String> {
        let target = incodex_macos::AppQuiescence::for_app(self.app)?;
        for _ in 0..80 {
            if incodex_macos::live_main_window_bounds(target.executable())?.is_some() {
                return Ok(());
            }
            self.wait(INITIAL_PROBE_INTERVAL);
        }
        Err("The target app's window did not appear; no reset was performed. Open ChatGPT and check with incodex doctor.".into())
    }

    fn reset(&mut self) -> Result<(), String> {
        bounded_command(Command::new("/usr/bin/tccutil").args([
            "reset",
            "Accessibility",
            incodex_macos::OFFICIAL_BUNDLE_IDENTIFIER,
        ]))
    }

    fn open_settings(&mut self) -> Result<(), String> {
        use incodex_core::format_kv;
        let mut command = open_settings_command(ACCESSIBILITY_SETTINGS_URL);
        bounded_command(&mut command)?;
        println!(
            "{}",
            format_kv(
                "Accessibility",
                "Complete the Accessibility approval in System Settings. Checking automatically for up to two minutes.",
                None,
            )
        );
        Ok(())
    }

    fn wait(&mut self, duration: Duration) {
        thread::sleep(duration);
    }
}

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
        let nonce = new_nonce()?;

        let mut child = Command::new(&executable)
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
        host.send_configure(&native_guide_config(copy_context)?)?;
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

    fn send_configure(&mut self, config: &NativeGuideConfig) -> Result<(), String> {
        self.send_json(json!({
            "nonce": self.nonce,
            "type": "configure",
            "copy": config.copy,
            "layoutDirection": config.layout_direction,
        }))
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
        "ready" => Ok(HostEvent::Ready),
        "allow" => Ok(HostEvent::Allow),
        "retry" => Ok(HostEvent::Retry),
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

fn native_guide_config(copy_context: GuideCopyContext) -> Result<NativeGuideConfig, String> {
    let source = incodex_runtime_assets::external_files()
        .iter()
        .find_map(|(name, body)| (*name == PERMISSION_COPY_NAME).then_some(*body))
        .ok_or("embedded native Accessibility copy catalog is missing")?;
    let mut catalog: Value = serde_json::from_str(source)
        .map_err(|error| format!("invalid native Accessibility copy catalog: {error}"))?;
    let catalog_object = catalog
        .as_object_mut()
        .ok_or("native Accessibility copy catalog is not an object")?;
    let raw_locale = configured_locale().unwrap_or_else(|| "en".into());
    let locale = resolve_catalog_locale(&raw_locale, catalog_object);
    let mut copy = catalog_object
        .remove(&locale)
        .ok_or_else(|| format!("native Accessibility copy catalog has no locale: {locale}"))?;
    let copy_object = copy
        .as_object_mut()
        .ok_or("native Accessibility locale copy is not an object")?;
    choose_guide_body(copy_object, copy_context)?;
    if let Some(error_body) = copy_object.get("errorBody").and_then(Value::as_str) {
        copy_object.insert(
            "errorBody".into(),
            Value::String(reentry_error_body(error_body)),
        );
    }
    let layout_direction = if is_rtl_locale(&locale) {
        "rightToLeft"
    } else {
        "leftToRight"
    };
    Ok(NativeGuideConfig {
        copy,
        layout_direction,
    })
}

fn choose_guide_body(
    copy: &mut serde_json::Map<String, Value>,
    context: GuideCopyContext,
) -> Result<(), String> {
    let key = match context {
        GuideCopyContext::Installed => "body",
        GuideCopyContext::Official => "officialBody",
    };
    let body = copy
        .get(key)
        .and_then(Value::as_str)
        .filter(|body| !body.trim().is_empty())
        .ok_or_else(|| format!("native Accessibility copy is missing {key}"))?
        .to_owned();
    copy.insert("body".into(), Value::String(body));
    Ok(())
}

fn reentry_error_body(body: &str) -> String {
    body.replace("incodex install", "incodex accessibility")
}

fn configured_locale() -> Option<String> {
    let source_home = locale_source_home();
    let config =
        fs::read_to_string(source_home.join(incodex_core::session_layout::CONFIG_SETTING_FILE))
            .ok();
    let config_override = config
        .as_deref()
        .and_then(|content| crate::locale::parse_locale_override(content, &['"']));
    select_locale_source(config_override.as_deref())
}

fn locale_source_home() -> PathBuf {
    let fallback = crate::open::default_source_home();
    resolve_locale_source_home(env::var_os("INCODEX_SOURCE_HOME").as_deref(), &fallback)
}

fn resolve_locale_source_home(value: Option<&std::ffi::OsStr>, fallback: &Path) -> PathBuf {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return fallback.to_path_buf();
    };
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn select_locale_source(config_override: Option<&str>) -> Option<String> {
    config_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn resolve_catalog_locale(raw: &str, catalog: &serde_json::Map<String, Value>) -> String {
    let normalized = raw.trim().replace('_', "-");
    let normalized = normalized.split('.').next().unwrap_or("en");
    if catalog.contains_key(normalized) {
        return normalized.to_string();
    }
    let lower = normalized.to_ascii_lowercase();
    if let Some(locale) = catalog
        .keys()
        .find(|locale| locale.to_ascii_lowercase() == lower)
    {
        return locale.clone();
    }
    if lower.starts_with("zh-hant-hk") || lower.starts_with("zh-hk") {
        return "zh-HK".into();
    }
    if lower.starts_with("zh-hant") || lower.starts_with("zh-tw") {
        return "zh-TW".into();
    }
    if lower.starts_with("zh") {
        return "zh-CN".into();
    }
    if lower == "en" || lower.starts_with("en-") {
        return "en".into();
    }
    let language = lower.split('-').next().unwrap_or("en");
    if catalog.contains_key(language) {
        return language.into();
    }
    let override_locale = match language {
        "es" => Some("es-419"),
        "fr" => Some("fr-FR"),
        "no" => Some("nb-NO"),
        "pt" => Some("pt-BR"),
        _ => None,
    };
    if let Some(locale) = override_locale.filter(|locale| catalog.contains_key(*locale)) {
        return locale.into();
    }
    catalog
        .keys()
        .find(|locale| {
            locale
                .to_ascii_lowercase()
                .starts_with(&format!("{language}-"))
        })
        .cloned()
        .unwrap_or_else(|| "en".into())
}

fn is_rtl_locale(locale: &str) -> bool {
    matches!(
        locale.to_ascii_lowercase().split('-').next(),
        Some("ar" | "fa" | "ur")
    )
}

fn verified_native_host_path(root: &Path) -> Result<PathBuf, String> {
    let published = incodex_runtime_bundle::ensure_current(root)?;
    let identity = incodex_runtime_bundle::runtime_identity()?;
    let deployed = incodex_runtime_bundle::inspect_deployed(root)?
        .ok_or("Runtime was not published for the native Accessibility guide")?;
    if deployed.release != published.release {
        return Err("published Runtime release changed during guide startup".into());
    }
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
mod tests {
    use super::*;

    #[test]
    fn accepts_only_nonce_bound_host_events() {
        assert_eq!(
            decode_host_event(br#"{"nonce":"abc","type":"ready"}"#, "abc"),
            Ok(HostEvent::Ready)
        );
        assert!(decode_host_event(br#"{"nonce":"wrong","type":"ready"}"#, "abc").is_err());
        assert!(decode_host_event(br#"{"nonce":"abc","type":"state"}"#, "abc").is_err());
    }

    #[test]
    fn rejects_oversized_transport_lines_without_parsing_them() {
        let (sender, receiver) = mpsc::sync_channel(2);
        let oversized = vec![b'x'; MAX_HOST_LINE_BYTES + 1];
        read_host_lines(std::io::Cursor::new(oversized), sender);
        assert!(matches!(receiver.recv().unwrap(), ReaderEvent::Oversized));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn reader_emits_each_line_then_eof() {
        let (sender, receiver) = mpsc::sync_channel(4);
        read_host_lines(std::io::Cursor::new(b"first\nsecond\n"), sender);
        assert!(matches!(
            receiver.recv().unwrap(),
            ReaderEvent::Line(line) if line == b"first"
        ));
        assert!(matches!(
            receiver.recv().unwrap(),
            ReaderEvent::Line(line) if line == b"second"
        ));
        assert!(matches!(receiver.recv().unwrap(), ReaderEvent::Eof));
    }

    #[test]
    fn reader_stops_cleanly_when_receiver_is_closed() {
        let (sender, receiver) = mpsc::sync_channel(1);
        drop(receiver);
        read_host_lines(std::io::Cursor::new(b"discarded\n"), sender);
    }

    #[test]
    fn bounded_reader_channel_handles_output_flood_without_unbounded_queue() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let reader =
            thread::spawn(|| read_host_lines(std::io::Cursor::new(b"one\ntwo\nthree\n"), sender));
        for expected in [b"one".as_slice(), b"two", b"three"] {
            assert!(matches!(
                receiver.recv().unwrap(),
                ReaderEvent::Line(line) if line == expected
            ));
        }
        assert!(matches!(receiver.recv().unwrap(), ReaderEvent::Eof));
        reader.join().unwrap();
    }

    #[test]
    fn nonzero_host_exit_after_stdout_eof_is_reported_as_error() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exit 7"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let stdin = child.stdin.take();
        let (sender, events) = mpsc::sync_channel(8);
        thread::spawn(move || read_host_lines(stdout, sender));
        let mut host = ProcessGuideHost {
            child,
            stdin,
            events,
            nonce: "test".into(),
            closed: false,
        };
        assert!(matches!(
            host.poll(Duration::from_secs(1)).unwrap(),
            HostEvent::Error(message) if message.contains("exited unsuccessfully")
        ));
        host.close();
    }

    #[test]
    fn native_copy_locale_resolution_matches_the_runtime_catalog_rules() {
        let catalog = [
            "en", "zh-CN", "zh-HK", "zh-TW", "de-DE", "es-419", "fr-FR", "nb-NO", "pt-BR", "ar",
            "fa", "ur",
        ]
        .into_iter()
        .map(|locale| (locale.to_string(), Value::Null))
        .collect();
        let cases = [
            ("", "en"),
            ("C.UTF-8", "en"),
            ("zh-MO", "zh-CN"),
            ("zh-Hant-HK", "zh-HK"),
            ("zh_Hant.UTF-8", "zh-TW"),
            ("de", "de-DE"),
            ("es", "es-419"),
            ("fr", "fr-FR"),
            ("no", "nb-NO"),
            ("pt", "pt-BR"),
            ("en-GB", "en"),
            ("unknown", "en"),
        ];
        for (raw, expected) in cases {
            assert_eq!(
                resolve_catalog_locale(raw, &catalog),
                expected,
                "locale {raw}"
            );
        }
    }

    #[test]
    fn native_error_copy_points_to_explicit_reentry_not_read_only_doctor() {
        assert_eq!(
            reentry_error_body("Run incodex install to check again."),
            "Run incodex accessibility to check again."
        );
    }

    #[test]
    fn native_guide_body_follows_verified_target_identity() {
        let mut installed = serde_json::json!({
            "body": "Installing Incodex changes ChatGPT.",
            "officialBody": "This is the official ChatGPT app.",
        });
        choose_guide_body(
            installed.as_object_mut().unwrap(),
            GuideCopyContext::Installed,
        )
        .unwrap();
        assert_eq!(installed["body"], "Installing Incodex changes ChatGPT.");

        let mut official = installed.clone();
        choose_guide_body(
            official.as_object_mut().unwrap(),
            GuideCopyContext::Official,
        )
        .unwrap();
        assert_eq!(official["body"], "This is the official ChatGPT app.");

        let mut missing = serde_json::json!({ "body": "Install only" });
        assert!(
            choose_guide_body(missing.as_object_mut().unwrap(), GuideCopyContext::Official)
                .is_err()
        );
    }

    #[test]
    fn embedded_native_copy_has_both_reasons_for_all_65_locales() {
        let source = incodex_runtime_assets::external_files()
            .iter()
            .find_map(|(name, body)| (*name == PERMISSION_COPY_NAME).then_some(*body))
            .expect("embedded permission catalog");
        let catalog: serde_json::Map<String, Value> = serde_json::from_str(source).unwrap();
        assert_eq!(catalog.len(), 65);
        for (locale, entry) in catalog {
            let install = entry["body"].as_str().expect("localized install reason");
            let official = entry["officialBody"]
                .as_str()
                .expect("localized official reason");
            let instruction = entry["addedTitle"].as_str().expect("instruction");
            assert!(!install.trim().is_empty(), "{locale}");
            assert!(!official.trim().is_empty(), "{locale}");
            assert!(official.contains("ChatGPT"), "{locale}");
            assert_ne!(install, official, "{locale}");
            assert_ne!(install, instruction, "{locale}");

            let mut installed = entry.as_object().unwrap().clone();
            choose_guide_body(&mut installed, GuideCopyContext::Installed).unwrap();
            assert_eq!(installed["body"], install, "{locale}");
            let mut restored = entry.as_object().unwrap().clone();
            choose_guide_body(&mut restored, GuideCopyContext::Official).unwrap();
            assert_eq!(restored["body"], official, "{locale}");
        }
    }

    #[test]
    fn native_copy_layout_direction_follows_the_canonical_rtl_locale() {
        let catalog: serde_json::Map<String, Value> = [
            ("en".into(), Value::Null),
            ("ar".into(), Value::Null),
            ("fa".into(), Value::Null),
            ("ur".into(), Value::Null),
            ("zh-CN".into(), Value::Null),
            ("zh-HK".into(), Value::Null),
            ("de-DE".into(), Value::Null),
        ]
        .into_iter()
        .collect();
        for raw in ["ar", "AR-SA", "fa_IR", "ur-PK"] {
            assert!(is_rtl_locale(&resolve_catalog_locale(raw, &catalog)));
        }
        for raw in ["", "zh-Hant-HK", "de-DE", "unknown"] {
            assert!(!is_rtl_locale(&resolve_catalog_locale(raw, &catalog)));
        }
    }

    #[test]
    fn open_settings_command_uses_background_open() {
        let command = open_settings_command(ACCESSIBILITY_SETTINGS_URL);
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(command.get_program().to_string_lossy(), "/usr/bin/open");
        assert_eq!(
            args,
            vec!["-g".to_string(), ACCESSIBILITY_SETTINGS_URL.to_string()]
        );
    }

    #[test]
    fn configured_locale_prefers_a_nonempty_config_override() {
        assert_eq!(select_locale_source(Some("fr-FR")), Some("fr-FR".into()));
    }

    #[test]
    fn configured_locale_falls_back_to_catalog_default_when_override_is_empty() {
        assert_eq!(select_locale_source(Some("  ")), None);
        assert_eq!(select_locale_source(None), None);
    }

    #[test]
    fn locale_source_home_matches_the_runtime_source_home_override() {
        let fallback = Path::new("/tmp/default-codex-home");
        assert_eq!(
            resolve_locale_source_home(
                Some(std::ffi::OsStr::new("/tmp/selected-codex-home")),
                fallback
            ),
            PathBuf::from("/tmp/selected-codex-home")
        );
        assert_eq!(resolve_locale_source_home(None, fallback), fallback);
    }
}
