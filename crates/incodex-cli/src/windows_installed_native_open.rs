//! Windows 安装入口的原生 open 生命周期与就绪通道。
//! 准备中不代表失败；保留清理 owner，只有子进程的真实 OPENED 才确认成功。

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::thread;

use serde_json::{json, Value};

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
}

impl<T> NativeOpenAttempt<T> {
    pub(crate) fn new(child: T, ready: Receiver<Result<(), String>>) -> Self {
        Self {
            child,
            readiness: Readiness::Waiting(ready),
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
    pub(crate) fn request<L, A>(&mut self, launch: L, mut child_is_alive: A) -> NativeOpenOutcome
    where
        L: FnOnce() -> Result<NativeOpenAttempt<T>, String>,
        A: FnMut(&mut T) -> Result<bool, String>,
    {
        let previously_ready = self
            .active
            .as_ref()
            .is_some_and(|attempt| matches!(&attempt.readiness, Readiness::Ready));
        if self.active.is_some() {
            let outcome = self
                .poll(&mut child_is_alive)
                .expect("an active native open produces a poll outcome");
            if !(previously_ready && self.active.is_none()) {
                return outcome;
            }
        }

        match launch() {
            Ok(attempt) => self.active = Some(attempt),
            Err(error) => return NativeOpenOutcome::Failure(error),
        }
        self.poll(child_is_alive)
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
            self.active.take();
            return Some(if ready {
                NativeOpenOutcome::Ready
            } else {
                NativeOpenOutcome::Failure(failure.unwrap_or_else(|| {
                    "native Incodex open exited before the window was ready".into()
                }))
            });
        }
        if ready {
            Some(NativeOpenOutcome::Ready)
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

pub(crate) fn launch_native_open(
    executable: &Path,
    source_bounds: Option<&str>,
) -> Result<NativeOpenAttempt<Child>, String> {
    let mut command = native_open_command(executable, source_bounds);
    let mut child = command
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot start native Incodex open: {error}"))?;
    let stdout = child.stdout.take();
    let (sender, receiver) = mpsc::sync_channel(1);
    match stdout {
        Some(stdout) => {
            let reader_sender = sender.clone();
            if let Err(error) = thread::Builder::new()
                .name("incodex-open-readiness".into())
                .spawn(move || read_opened_message(stdout, reader_sender))
            {
                let _ = sender.send(Err(format!(
                    "cannot monitor native Incodex open readiness: {error}"
                )));
            }
        }
        None => {
            let _ = sender.send(Err("native Incodex open has no readiness channel".into()));
        }
    }
    Ok(NativeOpenAttempt::new(child, receiver))
}

fn native_open_command(executable: &Path, source_bounds: Option<&str>) -> Command {
    let mut command = Command::new(executable);
    command.arg("open");
    if let Some(bounds) = source_bounds {
        command.env("INCODEX_SOURCE_BOUNDS", bounds);
    }
    command
}

fn read_opened_message(stdout: impl std::io::Read, sender: SyncSender<Result<(), String>>) {
    let mut reader = BufReader::new(stdout);
    let mut opened = false;
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) if !opened => {
                let _ = sender.send(Err(
                    "native Incodex open exited before the incognito window was ready".into(),
                ));
                break;
            }
            Ok(0) => break,
            Ok(_) if !opened && line.contains(crate::open_presentation::OPENED_MESSAGE) => {
                opened = true;
                let _ = sender.send(Ok(()));
            }
            Ok(_) => {}
            Err(error) if !opened => {
                let _ = sender.send(Err(format!(
                    "cannot read native Incodex open readiness: {error}"
                )));
                break;
            }
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{native_open_command, NativeOpenAttempt, NativeOpenOutcome, NativeOpenState};
    use std::ffi::OsStr;
    use std::path::Path;
    use std::sync::mpsc;

    #[test]
    fn unknown_release_is_rejected_before_joining_waiting_or_ready_owner() {
        use super::{validate_native_open_request, NativeOpenBridgeRequest, NativeOpenOutcome};
        use std::collections::VecDeque;

        let (sender, receiver) = mpsc::channel();
        let mut state = NativeOpenState::default();
        assert_eq!(
            state.request(
                || Ok(NativeOpenAttempt::new(41_u32, receiver)),
                |_| Ok(true)
            ),
            NativeOpenOutcome::Pending
        );
        let request = |id: &str, release: &str| NativeOpenBridgeRequest {
            request_id: id.into(),
            execution_context_id: 17,
            source_bounds: None,
            runtime_release: Some(release.into()),
        };
        let pending = VecDeque::from([request("incodex-known-a", "a")]);
        let mut outcome = None;
        for ready in [false, true] {
            if ready {
                sender.send(Ok(())).unwrap();
                assert_eq!(state.poll(|_| Ok(true)), Some(NativeOpenOutcome::Ready));
                outcome = Some(NativeOpenOutcome::Ready);
            }
            let before_outcome = format!("{outcome:?}");
            let rejected =
                validate_native_open_request(request("incodex-unknown", "unknown"), |release| {
                    if release == Some("a") {
                        Ok(())
                    } else {
                        Err("unobserved generation".into())
                    }
                })
                .unwrap_err();
            assert_eq!(rejected.0.request_id, "incodex-unknown");
            assert_eq!(pending.len(), 1);
            assert_eq!(pending[0].request_id, "incodex-known-a");
            assert_eq!(format!("{outcome:?}"), before_outcome);
            assert!(super::native_open_bridge_response(
                &rejected.0,
                &NativeOpenOutcome::Failure(rejected.1.clone()),
                false
            )
            .is_none());
            assert_eq!(
                super::native_open_bridge_response(
                    &rejected.0,
                    &NativeOpenOutcome::Failure(rejected.1),
                    true
                )
                .unwrap()["ok"],
                false
            );
        }
        assert_eq!(
            validate_native_open_request(request("incodex-valid-after", "a"), |_| Ok(()))
                .unwrap()
                .request_id,
            "incodex-valid-after"
        );
        assert_eq!(
            state.request(
                || panic!("original ready owner must be retained"),
                |owner| {
                    assert_eq!(*owner, 41);
                    Ok(true)
                }
            ),
            NativeOpenOutcome::Ready
        );
        assert_eq!(pending.len(), 1);
        assert_eq!(outcome, Some(NativeOpenOutcome::Ready));
    }

    #[test]
    fn delayed_ready_keeps_the_same_attempt_pending_until_opened() {
        let (sender, receiver) = mpsc::channel();
        let mut state = NativeOpenState::default();
        let before_ready = state.request(
            || Ok(NativeOpenAttempt::new(41_u32, receiver)),
            |_| Ok(true),
        );
        sender.send(Ok(())).expect("readiness receiver stays owned");
        let after_ready = state.request(|| panic!("must reuse the same child"), |_| Ok(true));

        assert_eq!(
            (before_ready, after_ready),
            (NativeOpenOutcome::Pending, NativeOpenOutcome::Ready)
        );
    }

    #[test]
    fn duplicate_live_request_reuses_child_without_acknowledging_early() {
        let (sender, receiver) = mpsc::channel();
        let mut state = NativeOpenState::default();
        let mut launches = 0;
        let mut selected_bounds = None;
        let first = state.request(
            || {
                launches += 1;
                selected_bounds = Some("first request bounds".to_string());
                Ok(NativeOpenAttempt::new(7_u32, receiver))
            },
            |_| Ok(true),
        );
        let duplicate = state.request(
            || {
                launches += 1;
                selected_bounds = Some("duplicate request bounds".to_string());
                Err("duplicate launch".to_string())
            },
            |_| Ok(true),
        );
        sender.send(Ok(())).expect("readiness receiver stays owned");
        let completed = state.request(|| panic!("must reuse the same child"), |_| Ok(true));

        assert_eq!(
            (launches, first, duplicate, completed),
            (
                1,
                NativeOpenOutcome::Pending,
                NativeOpenOutcome::Pending,
                NativeOpenOutcome::Ready
            ),
            "duplicate requests reuse one child and wait for OPENED"
        );
        assert_eq!(selected_bounds.as_deref(), Some("first request bounds"));
    }

    #[test]
    fn child_exit_before_opened_is_a_failure() {
        let (_sender, receiver) = mpsc::channel();
        let mut state = NativeOpenState::default();
        let outcome = state.request(
            || Ok(NativeOpenAttempt::new(12_u32, receiver)),
            |_| Ok(false),
        );

        assert!(matches!(outcome, NativeOpenOutcome::Failure(_)));
    }

    #[test]
    fn disconnected_readiness_channel_is_a_failure() {
        let (sender, receiver) = mpsc::channel();
        drop(sender);
        let mut state = NativeOpenState::default();
        let outcome = state.request(
            || Ok(NativeOpenAttempt::new(15_u32, receiver)),
            |_| Ok(true),
        );

        assert!(matches!(outcome, NativeOpenOutcome::Failure(_)));
    }

    #[test]
    fn ready_live_owner_stays_ready_without_a_second_launch() {
        let (sender, receiver) = mpsc::channel();
        let mut state = NativeOpenState::default();
        let mut launches = 0;
        let pending = state.request(
            || {
                launches += 1;
                Ok(NativeOpenAttempt::new(22_u32, receiver))
            },
            |_| Ok(true),
        );
        sender.send(Ok(())).expect("readiness receiver stays owned");
        let ready = state.request(
            || {
                launches += 1;
                Err("ready owner must be reused".into())
            },
            |_| Ok(true),
        );

        assert_eq!(
            (launches, pending, ready),
            (1, NativeOpenOutcome::Pending, NativeOpenOutcome::Ready)
        );
    }

    #[test]
    fn dead_ready_owner_is_replaced_by_the_next_request() {
        let (first_sender, first_receiver) = mpsc::channel();
        let (second_sender, second_receiver) = mpsc::channel();
        let first_child_alive = std::cell::Cell::new(true);
        let mut state = NativeOpenState::default();
        let mut launches = 0;
        let pending = state.request(
            || {
                launches += 1;
                Ok(NativeOpenAttempt::new(1_u32, first_receiver))
            },
            |child| Ok(*child != 1 || first_child_alive.get()),
        );
        first_sender
            .send(Ok(()))
            .expect("readiness receiver stays owned");
        first_child_alive.set(false);
        let first_ready = state.poll(|child| Ok(*child != 1 || first_child_alive.get()));
        let next_pending = state.request(
            || {
                launches += 1;
                Ok(NativeOpenAttempt::new(2_u32, second_receiver))
            },
            |child| Ok(*child != 1 || first_child_alive.get()),
        );
        second_sender
            .send(Ok(()))
            .expect("second readiness receiver stays owned");
        let second_ready = state.poll(|_| Ok(true));

        assert_eq!(launches, 2);
        assert_eq!(pending, NativeOpenOutcome::Pending);
        assert_eq!(first_ready, Some(NativeOpenOutcome::Ready));
        assert_eq!(next_pending, NativeOpenOutcome::Pending);
        assert_eq!(second_ready, Some(NativeOpenOutcome::Ready));
    }

    #[test]
    fn exited_child_fails_queued_request_then_next_request_launches_again() {
        let (first_sender, first_receiver) = mpsc::channel();
        let (second_sender, second_receiver) = mpsc::channel();
        let first_child_alive = std::cell::Cell::new(true);
        let mut state = NativeOpenState::default();
        let mut launches = 0;
        let first = state.request(
            || {
                launches += 1;
                Ok(NativeOpenAttempt::new(1_u32, first_receiver))
            },
            |_| Ok(true),
        );
        drop(first_sender);
        first_child_alive.set(false);
        let aborted = state.poll(|_| Ok(first_child_alive.get()));
        let retry = state.request(
            || {
                launches += 1;
                Ok(NativeOpenAttempt::new(2_u32, second_receiver))
            },
            |_| Ok(true),
        );
        second_sender
            .send(Ok(()))
            .expect("retry readiness receiver stays owned");
        let ready = state.poll(|_| Ok(true));

        assert_eq!(first, NativeOpenOutcome::Pending);
        assert!(matches!(aborted, Some(NativeOpenOutcome::Failure(_))));
        assert_eq!(launches, 2);
        assert_eq!(retry, NativeOpenOutcome::Pending);
        assert_eq!(ready, Some(NativeOpenOutcome::Ready));
    }

    #[test]
    fn launch_uses_the_managed_executable_and_first_request_bounds() {
        let executable = Path::new(r"C:\Incodex\incodex.exe");
        let command = native_open_command(executable, Some("250,136,1399,820"));
        let arguments = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let bounds = command
            .get_envs()
            .find(|(name, _)| *name == OsStr::new("INCODEX_SOURCE_BOUNDS"))
            .and_then(|(_, value)| value)
            .and_then(OsStr::to_str);

        assert_eq!(command.get_program(), executable.as_os_str());
        assert_eq!(arguments, ["open"]);
        assert_eq!(bounds, Some("250,136,1399,820"));
    }

    #[test]
    fn only_the_child_opened_line_marks_readiness() {
        let output = format!(
            "Preparing private session\n{}\n",
            crate::open_presentation::OPENED_MESSAGE
        );
        let (sender, receiver) = mpsc::sync_channel(1);
        super::read_opened_message(std::io::Cursor::new(output), sender);

        assert_eq!(receiver.try_recv(), Ok(Ok(())));
    }

    #[test]
    fn child_output_ending_before_opened_is_a_readiness_failure() {
        let (sender, receiver) = mpsc::sync_channel(1);
        super::read_opened_message(std::io::Cursor::new("still preparing\n"), sender);

        assert!(matches!(receiver.try_recv(), Ok(Err(_))));
    }

    #[test]
    fn launch_error_is_returned_without_creating_an_owner() {
        let mut state = NativeOpenState::<u32>::default();
        let outcome = state.request(|| Err("spawn failed".to_string()), |_| Ok(true));

        assert_eq!(outcome, NativeOpenOutcome::Failure("spawn failed".into()));
        assert_eq!(state.poll(|_| Ok(true)), None);
    }
}
