//! [INPUT]: 依赖 native CLI、来源窗口边界、App 路径及 OPENED/raise 回执。
//! [OUTPUT]: 提供单 owner 生命周期、真实就绪握手与非阻塞 raise 回执通道。
//! [POS]: Windows 保留 request；macOS launch 复用状态并调用 request_with_raise。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
//!
//! 准备中不代表失败；不因就绪等待终止官方 App。

#[cfg(target_os = "windows")]
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::thread;

use serde_json::{json, Value};

pub(crate) const NATIVE_OPEN_RAISED: &str = "INCODEX_RAISED";
pub(crate) const NATIVE_OPEN_RAISE_FAILED: &str = "INCODEX_RAISE_FAILED";
pub(crate) const NATIVE_OPEN_CONTROL_READY: &str = "INCODEX_CONTROL_READY";
const NATIVE_OPEN_RAISE_REQUEST: &[u8] = b"raise\n";

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct NativeOpenBridgeRequest {
    pub request_id: String,
    pub execution_context_id: u64,
    pub source_bounds: Option<String>,
    pub runtime_release: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum NativeOpenOutcome {
    Pending,
    Ready,
    Failure(String),
}

pub(crate) fn native_open_bridge_response(
    request: &NativeOpenBridgeRequest,
    outcome: &NativeOpenOutcome,
    context_is_current: bool,
) -> Option<Value> {
    if !context_is_current {
        return None;
    }
    match outcome {
        NativeOpenOutcome::Pending => None,
        NativeOpenOutcome::Ready => Some(json!({
            "requestId": request.request_id,
            "ok": true,
            "code": "OK"
        })),
        NativeOpenOutcome::Failure(reason) => Some(json!({
            "requestId": request.request_id,
            "ok": false,
            "code": "FAILED",
            "reason": reason
        })),
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn take_native_open_requests_for_resolution(
    pending: &mut VecDeque<NativeOpenBridgeRequest>,
    outcome: &NativeOpenOutcome,
) -> VecDeque<NativeOpenBridgeRequest> {
    if matches!(outcome, NativeOpenOutcome::Pending) {
        VecDeque::new()
    } else {
        std::mem::take(pending)
    }
}

enum Readiness {
    Waiting(Receiver<Result<(), String>>),
    Ready,
    Failed(String),
}

pub(crate) struct NativeOpenAttempt<T> {
    child: T,
    readiness: Readiness,
    control: Option<NativeOpenControl>,
}

struct NativeOpenControl {
    input: Box<dyn Write + Send>,
    capability: Option<Receiver<()>>,
    responses: Receiver<Result<(), String>>,
    available: bool,
    raise_pending: bool,
}

impl<T> NativeOpenAttempt<T> {
    pub(crate) fn new(child: T, ready: Receiver<Result<(), String>>) -> Self {
        Self {
            child,
            readiness: Readiness::Waiting(ready),
            control: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_control<W>(
        child: T,
        ready: Receiver<Result<(), String>>,
        input: W,
        responses: Receiver<Result<(), String>>,
    ) -> Self
    where
        W: Write + Send + 'static,
    {
        Self {
            child,
            readiness: Readiness::Waiting(ready),
            control: Some(NativeOpenControl {
                input: Box::new(input),
                capability: None,
                responses,
                available: true,
                raise_pending: false,
            }),
        }
    }

    pub(crate) fn with_control_events<W>(
        child: T,
        ready: Receiver<Result<(), String>>,
        input: W,
        capability: Receiver<()>,
        responses: Receiver<Result<(), String>>,
    ) -> Self
    where
        W: Write + Send + 'static,
    {
        Self {
            child,
            readiness: Readiness::Waiting(ready),
            control: Some(NativeOpenControl {
                input: Box::new(input),
                capability: Some(capability),
                responses,
                available: false,
                raise_pending: false,
            }),
        }
    }

    fn begin_raise(&mut self) -> Result<(), String> {
        let Some(control) = self.control.as_mut() else {
            return Err("native Incodex open has no raise control channel".into());
        };
        if !control.available {
            return Err(
                "native Incodex open CLI did not advertise the raise control capability".into(),
            );
        }
        if control.raise_pending {
            return Ok(());
        }
        control
            .input
            .write_all(NATIVE_OPEN_RAISE_REQUEST)
            .and_then(|_| control.input.flush())
            .map_err(|error| format!("cannot send native Incodex raise request: {error}"))?;
        control.raise_pending = true;
        Ok(())
    }

    fn control_event(&mut self) -> Option<Result<(), String>> {
        let control = self.control.as_mut()?;
        let capability_event = control
            .capability
            .as_ref()
            .map(|capability| capability.try_recv());
        match capability_event {
            Some(Ok(())) => {
                control.available = true;
                control.capability = None;
            }
            Some(Err(TryRecvError::Disconnected)) => control.capability = None,
            Some(Err(TryRecvError::Empty)) | None => {}
        }
        if !control.raise_pending {
            return None;
        }
        match control.responses.try_recv() {
            Ok(response) => {
                control.raise_pending = false;
                Some(response)
            }
            Err(TryRecvError::Disconnected) => {
                control.raise_pending = false;
                Some(Err(
                    "native Incodex raise acknowledgement channel disconnected".into(),
                ))
            }
            Err(TryRecvError::Empty) => None,
        }
    }
}

pub(crate) struct NativeOpenState<T> {
    active: Option<NativeOpenAttempt<T>>,
}

impl<T> Default for NativeOpenState<T> {
    fn default() -> Self {
        Self { active: None }
    }
}

impl<T> NativeOpenState<T> {
    #[cfg(any(target_os = "windows", test))]
    pub(crate) fn request<L, A>(&mut self, launch: L, mut child_is_alive: A) -> NativeOpenOutcome
    where
        L: FnOnce() -> Result<NativeOpenAttempt<T>, String>,
        A: FnMut(&mut T) -> Result<bool, String>,
    {
        self.request_inner(launch, &mut child_is_alive, false)
    }

    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn request_with_raise<L, A>(
        &mut self,
        launch: L,
        mut child_is_alive: A,
    ) -> NativeOpenOutcome
    where
        L: FnOnce() -> Result<NativeOpenAttempt<T>, String>,
        A: FnMut(&mut T) -> Result<bool, String>,
    {
        self.request_inner(launch, &mut child_is_alive, true)
    }

    fn request_inner<L, A>(
        &mut self,
        launch: L,
        child_is_alive: &mut A,
        raise_ready_owner: bool,
    ) -> NativeOpenOutcome
    where
        L: FnOnce() -> Result<NativeOpenAttempt<T>, String>,
        A: FnMut(&mut T) -> Result<bool, String>,
    {
        let previously_ready = self
            .active
            .as_ref()
            .is_some_and(|attempt| matches!(&attempt.readiness, Readiness::Ready));
        let raise_was_pending = self
            .active
            .as_ref()
            .and_then(|attempt| attempt.control.as_ref())
            .is_some_and(|control| control.raise_pending);
        if self.active.is_some() {
            let outcome = self
                .poll(&mut *child_is_alive)
                .expect("an active native open produces a poll outcome");
            if raise_ready_owner && previously_ready && raise_was_pending {
                return outcome;
            }
            if raise_ready_owner && previously_ready {
                if let Some(attempt) = self.active.as_mut() {
                    if outcome != NativeOpenOutcome::Ready {
                        return outcome;
                    }
                    return attempt
                        .begin_raise()
                        .map(|()| NativeOpenOutcome::Pending)
                        .unwrap_or_else(NativeOpenOutcome::Failure);
                }
            }
            if self.active.is_some() || !previously_ready {
                return outcome;
            }
        }

        match launch() {
            Ok(attempt) => self.active = Some(attempt),
            Err(error) => return NativeOpenOutcome::Failure(error),
        }
        self.poll(&mut *child_is_alive)
            .expect("a launched native open produces a poll outcome")
    }

    pub(crate) fn poll<A>(&mut self, mut child_is_alive: A) -> Option<NativeOpenOutcome>
    where
        A: FnMut(&mut T) -> Result<bool, String>,
    {
        let active = self.active.as_mut()?;
        if let Readiness::Waiting(receiver) = &active.readiness {
            match receiver.try_recv() {
                Ok(Ok(())) => active.readiness = Readiness::Ready,
                Ok(Err(error)) => active.readiness = Readiness::Failed(error),
                Err(TryRecvError::Disconnected) => {
                    active.readiness = Readiness::Failed(
                        "native Incodex open readiness channel disconnected".into(),
                    )
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        let ready = matches!(&active.readiness, Readiness::Ready);
        let failure = match &active.readiness {
            Readiness::Failed(error) => Some(error.clone()),
            Readiness::Waiting(_) | Readiness::Ready => None,
        };
        let alive = match child_is_alive(&mut active.child) {
            Ok(alive) => alive,
            Err(error) => return Some(NativeOpenOutcome::Failure(error)),
        };

        if !alive {
            let raise_pending = active
                .control
                .as_ref()
                .is_some_and(|control| control.raise_pending);
            self.active.take();
            return Some(if raise_pending {
                NativeOpenOutcome::Failure(
                    "native Incodex open exited before the raise acknowledgement".into(),
                )
            } else if ready {
                NativeOpenOutcome::Ready
            } else {
                NativeOpenOutcome::Failure(failure.unwrap_or_else(|| {
                    "native Incodex open exited before the window was ready".into()
                }))
            });
        }
        if ready {
            match active.control_event() {
                Some(Ok(())) => Some(NativeOpenOutcome::Ready),
                Some(Err(error)) => Some(NativeOpenOutcome::Failure(error)),
                None if active
                    .control
                    .as_ref()
                    .is_some_and(|control| control.raise_pending) =>
                {
                    Some(NativeOpenOutcome::Pending)
                }
                None => Some(NativeOpenOutcome::Ready),
            }
        } else if let Some(error) = failure {
            Some(NativeOpenOutcome::Failure(error))
        } else {
            Some(NativeOpenOutcome::Pending)
        }
    }
}

pub(crate) fn validate_native_open_request(
    request: NativeOpenBridgeRequest,
    validate: impl FnOnce(Option<&str>) -> Result<(), String>,
) -> Result<NativeOpenBridgeRequest, (NativeOpenBridgeRequest, String)> {
    // 每个请求先独立验证；已有 owner 不能跳过身份检查或吸收未知请求。
    if let Err(error) = validate(request.runtime_release.as_deref()) {
        return Err((request, error));
    }
    Ok(request)
}

#[cfg(target_os = "windows")]
pub(crate) fn launch_native_open(
    executable: &Path,
    source_bounds: Option<&str>,
) -> Result<NativeOpenAttempt<Child>, String> {
    launch_native_open_command(native_open_command(executable, source_bounds), false)
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn launch_native_open_for_app(
    executable: &Path,
    source_bounds: Option<&str>,
    app: &Path,
    runtime_release: &str,
) -> Result<NativeOpenAttempt<Child>, String> {
    launch_native_open_command(
        native_open_command_for_app(executable, source_bounds, app, runtime_release),
        true,
    )
}

fn launch_native_open_command(
    mut command: Command,
    with_control: bool,
) -> Result<NativeOpenAttempt<Child>, String> {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command
        .stdin(if with_control {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot start native Incodex open: {error}"))?;
    let stdout = child.stdout.take();
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (capability_sender, capability_receiver) = mpsc::channel();
    let (raise_sender, raise_receiver) = mpsc::channel();
    match stdout {
        Some(stdout) => {
            let reader_ready_sender = ready_sender.clone();
            if let Err(error) = thread::Builder::new()
                .name("incodex-open-readiness".into())
                .spawn(move || {
                    read_child_output(stdout, reader_ready_sender, capability_sender, raise_sender)
                })
            {
                let _ = ready_sender.send(Err(format!(
                    "cannot monitor native Incodex open readiness: {error}"
                )));
            }
        }
        None => {
            let _ = ready_sender.send(Err("native Incodex open has no readiness channel".into()));
        }
    }
    match (with_control, child.stdin.take()) {
        (true, Some(stdin)) => Ok(NativeOpenAttempt::with_control_events(
            child,
            ready_receiver,
            stdin,
            capability_receiver,
            raise_receiver,
        )),
        _ => Ok(NativeOpenAttempt::new(child, ready_receiver)),
    }
}

fn native_open_command(executable: &Path, source_bounds: Option<&str>) -> Command {
    let mut command = Command::new(executable);
    command.arg("open");
    if let Some(bounds) = source_bounds {
        command.env("INCODEX_SOURCE_BOUNDS", bounds);
    }
    command
}

#[cfg(any(target_os = "macos", test))]
fn native_open_command_for_app(
    executable: &Path,
    source_bounds: Option<&str>,
    app: &Path,
    runtime_release: &str,
) -> Command {
    let mut command = native_open_command(executable, source_bounds);
    command
        .arg("--app")
        .arg(app)
        .env("INCODEX_LAUNCH_RUNTIME_RELEASE", runtime_release);
    command
}

fn read_child_output(
    stdout: impl std::io::Read,
    ready_sender: SyncSender<Result<(), String>>,
    capability_sender: mpsc::Sender<()>,
    raise_sender: mpsc::Sender<Result<(), String>>,
) {
    let mut reader = BufReader::new(stdout);
    let mut opened = false;
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) if !opened => {
                let _ = ready_sender.send(Err(
                    "native Incodex open exited before the incognito window was ready".into(),
                ));
                let _ = raise_sender.send(Err(
                    "native Incodex open exited before a raise acknowledgement".into(),
                ));
                break;
            }
            Ok(0) => {
                let _ = raise_sender.send(Err(
                    "native Incodex open exited before a raise acknowledgement".into(),
                ));
                break;
            }
            Ok(_) if line.trim_end_matches(&['\r', '\n'][..]) == NATIVE_OPEN_CONTROL_READY => {
                let _ = capability_sender.send(());
            }
            Ok(_) if !opened && line.contains(crate::open_presentation::OPENED_MESSAGE) => {
                opened = true;
                let _ = ready_sender.send(Ok(()));
            }
            Ok(_) if line.trim_end_matches(&['\r', '\n'][..]) == NATIVE_OPEN_RAISED => {
                let _ = raise_sender.send(Ok(()));
            }
            Ok(_) if line.trim_end_matches(&['\r', '\n'][..]) == NATIVE_OPEN_RAISE_FAILED => {
                let _ = raise_sender.send(Err(
                    "native Incodex could not bring the existing private window forward".into(),
                ));
            }
            Ok(_) => {}
            Err(error) if !opened => {
                let _ = ready_sender.send(Err(format!(
                    "cannot read native Incodex open readiness: {error}"
                )));
                let _ = raise_sender.send(Err(format!(
                    "cannot read native Incodex raise acknowledgement: {error}"
                )));
                break;
            }
            Err(error) => {
                let _ = raise_sender.send(Err(format!(
                    "cannot read native Incodex raise acknowledgement: {error}"
                )));
                break;
            }
        }
    }
}

#[cfg(test)]
#[path = "windows_installed_native_open_tests.rs"]
mod tests;
