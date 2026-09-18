//! Private stdio transport for the short-lived native Accessibility guide.
//!
//! The short-lived Node native host is only a UI host.  It never performs a
//! TCC reset or opens Settings; those operations stay in the CLI.  This module
//! deliberately keeps the transport small and bounded so a broken/hostile host cannot
//! make `uninstall` wait forever or turn arbitrary output into a command.

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

pub(crate) const HOST_ARTIFACT_NAME: &str = "incodex-permission-host.cjs";
pub(crate) const OFFICIAL_APP_PATH: &str = "/Applications/ChatGPT.app";
pub(crate) const OFFICIAL_NODE_PATH: &str =
    "/Applications/ChatGPT.app/Contents/Resources/cua_node/bin/node";
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
const NODE_VERIFY_TIMEOUT: Duration = Duration::from_secs(5);
const INITIAL_PROBE_ATTEMPTS: usize = 120;
const INITIAL_PROBE_INTERVAL: Duration = Duration::from_millis(250);
const POST_ALLOW_PROBE_INTERVAL: Duration = Duration::from_millis(750);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Granted,
    Pending,
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

#[derive(Default)]
pub(crate) struct ProcessGuideFactory;

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
        Ok(Box::new(ProcessGuideHost::spawn(root)?))
    }
}

/// Run the one-shot guide for either the restored official app or an already
/// validated patched app.  The caller owns its transaction lock and supplies a
/// verifier that is invoked before launch and again immediately before the
/// single possible reset.  No signature policy is hidden in this generic loop.
pub(crate) fn run_permission_guide<F>(
    root: &Path,
    app: &Path,
    mut verify_target: F,
) -> Result<Outcome, String>
where
    F: FnMut() -> Result<(), String>,
{
    let mut ops = SystemGuideOps { app };
    let mut factory = ProcessGuideFactory;
    run_permission_guide_with(
        &mut ops,
        root,
        app,
        &mut verify_target,
        &mut factory,
        HOST_GUIDE_TIMEOUT,
    )
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
                // the user that this verification is in progress first: a
                // vendor/signature check may take several seconds.
                host.send_state(HostState::Repairing)?;
                verify_target()?;
                // A user may have approved the system row while the guide was
                // animating.  Only an explicit denied probe authorizes reset;
                // unknown/not-running is an identity/readiness failure.
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
        bounded_command(
            Command::new("/usr/bin/open").arg(
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
            ),
        )?;
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
    fn spawn(root: &Path) -> Result<Self, String> {
        let node = verified_node_path()?;
        let script = verified_host_script(root)?;
        let nonce = new_nonce()?;

        let mut child = Command::new(&node)
            .arg(&script)
            .arg("--nonce")
            .arg(&nonce)
            // The native host is trusted only through the verified Runtime
            // release.  Do not let a parent Electron/Node injection setting
            // alter this short-lived process.
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

        Ok(Self {
            child,
            stdin: Some(stdin),
            events,
            nonce,
            closed: false,
        })
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

fn verified_node_path() -> Result<PathBuf, String> {
    let path = Path::new(OFFICIAL_NODE_PATH);
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect official Runtime node: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err("official Runtime node is not a regular file".into());
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err("official Runtime node is not executable".into());
    }
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve official Runtime node: {error}"))?;
    if canonical != path {
        return Err("official Runtime node path changed during validation".into());
    }
    verify_codesign(path)?;
    Ok(canonical)
}

fn verify_codesign(path: &Path) -> Result<(), String> {
    let mut child = Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict", "--verbose=0", "--"])
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot verify official Runtime node signature: {error}"))?;
    let deadline = Instant::now() + NODE_VERIFY_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!("official Runtime node signature failed: {status}"))
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("official Runtime node signature verification timed out".into());
            }
        }
    }
}

fn verified_host_script(root: &Path) -> Result<PathBuf, String> {
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
    let release = root.join("runtime").join(&deployed.release);
    let script = release.join(HOST_ARTIFACT_NAME);
    let metadata = fs::symlink_metadata(&script)
        .map_err(|error| format!("native Accessibility guide Runtime asset is missing: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err("native Accessibility guide Runtime asset is not a regular file".into());
    }
    let canonical = fs::canonicalize(&script).map_err(|error| {
        format!("cannot resolve native Accessibility guide Runtime asset: {error}")
    })?;
    if canonical != script {
        return Err(
            "native Accessibility guide Runtime asset path changed during validation".into(),
        );
    }
    Ok(canonical)
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
}
