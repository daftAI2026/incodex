//! [INPUT]: 依赖 native open 生命周期与私有 raise 控制协议。
//! [OUTPUT]: 验证 owner 复用、OPENED 握手、raise 回执与失败保留。
//! [POS]: Windows 与 macOS 共享 native open 状态的回归测试，不启动真实 App。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use super::{
    native_open_command, native_open_command_for_app, NativeOpenAttempt, NativeOpenOutcome,
    NativeOpenState,
};
use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct SharedWriter(Arc<Mutex<Vec<u8>>>);

impl Write for SharedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

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
fn raise_aware_request_only_raises_a_reused_ready_owner() {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let writer = SharedWriter(bytes.clone());
    let (ready_tx, ready_rx) = mpsc::channel();
    let (raise_tx, raise_rx) = mpsc::channel();
    let mut launches = 0;
    let mut state = NativeOpenState::default();

    let pending = state.request_with_raise(
        || {
            launches += 1;
            Ok(NativeOpenAttempt::with_control(
                41_u32, ready_rx, writer, raise_rx,
            ))
        },
        |_| Ok(true),
    );
    assert_eq!(pending, NativeOpenOutcome::Pending);
    assert!(
        bytes.lock().unwrap().is_empty(),
        "pending owner is not raised"
    );

    ready_tx.send(Ok(())).unwrap();
    assert_eq!(state.poll(|_| Ok(true)), Some(NativeOpenOutcome::Ready));
    let raise_pending = state.request_with_raise(
        || panic!("duplicate must reuse the ready owner"),
        |_| Ok(true),
    );
    let duplicate_while_raising = state.request_with_raise(
        || panic!("an in-flight raise must reuse the ready owner"),
        |_| Ok(true),
    );
    assert_eq!(raise_pending, NativeOpenOutcome::Pending);
    assert_eq!(duplicate_while_raising, NativeOpenOutcome::Pending);
    assert_eq!(&*bytes.lock().unwrap(), b"raise\n");

    raise_tx.send(Ok(())).unwrap();
    let raised = state.poll(|_| Ok(true));

    assert_eq!(raised, Some(NativeOpenOutcome::Ready));
    assert_eq!(launches, 1);
    assert_eq!(&*bytes.lock().unwrap(), b"raise\n");
}

#[test]
fn failed_raise_fails_only_that_request_and_keeps_the_ready_owner() {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let (ready_tx, ready_rx) = mpsc::channel();
    let (raise_tx, raise_rx) = mpsc::channel();
    let mut state = NativeOpenState::default();
    assert_eq!(
        state.request_with_raise(
            || {
                Ok(NativeOpenAttempt::with_control(
                    41_u32,
                    ready_rx,
                    SharedWriter(bytes.clone()),
                    raise_rx,
                ))
            },
            |_| Ok(true)
        ),
        NativeOpenOutcome::Pending
    );
    ready_tx.send(Ok(())).unwrap();
    assert_eq!(state.poll(|_| Ok(true)), Some(NativeOpenOutcome::Ready));

    assert_eq!(
        state.request_with_raise(
            || panic!("raise failure must not replace owner"),
            |_| { Ok(true) }
        ),
        NativeOpenOutcome::Pending
    );
    raise_tx
        .send(Err("window activation failed".into()))
        .unwrap();
    assert_eq!(
        state.poll(|_| Ok(true)),
        Some(NativeOpenOutcome::Failure(
            "window activation failed".into()
        ))
    );
    assert_eq!(state.poll(|_| Ok(true)), Some(NativeOpenOutcome::Ready));
    assert_eq!(&*bytes.lock().unwrap(), b"raise\n");
}

#[test]
fn a_cli_without_the_raise_capability_fails_duplicate_without_writing_stdin() {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let (ready_tx, ready_rx) = mpsc::channel();
    let (_control_tx, control_rx) = mpsc::channel();
    let (_raise_tx, raise_rx) = mpsc::channel();
    let mut state = NativeOpenState::default();
    assert_eq!(
        state.request_with_raise(
            || {
                Ok(NativeOpenAttempt::with_control_events(
                    41_u32,
                    ready_rx,
                    SharedWriter(bytes.clone()),
                    control_rx,
                    raise_rx,
                ))
            },
            |_| Ok(true)
        ),
        NativeOpenOutcome::Pending
    );
    ready_tx.send(Ok(())).unwrap();
    assert_eq!(state.poll(|_| Ok(true)), Some(NativeOpenOutcome::Ready));

    assert!(matches!(
        state.request_with_raise(
            || panic!("an unsupported CLI keeps its ready owner"),
            |_| Ok(true)
        ),
        NativeOpenOutcome::Failure(reason) if reason.contains("raise control capability")
    ));
    assert_eq!(state.poll(|_| Ok(true)), Some(NativeOpenOutcome::Ready));
    assert!(bytes.lock().unwrap().is_empty());
}

#[test]
fn child_exit_during_raise_fails_the_duplicate_and_releases_its_owner() {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let (ready_tx, ready_rx) = mpsc::channel();
    let (_raise_tx, raise_rx) = mpsc::channel();
    let mut state = NativeOpenState::default();
    assert_eq!(
        state.request_with_raise(
            || {
                Ok(NativeOpenAttempt::with_control(
                    41_u32,
                    ready_rx,
                    SharedWriter(bytes.clone()),
                    raise_rx,
                ))
            },
            |_| Ok(true)
        ),
        NativeOpenOutcome::Pending
    );
    ready_tx.send(Ok(())).unwrap();
    assert_eq!(state.poll(|_| Ok(true)), Some(NativeOpenOutcome::Ready));
    assert_eq!(
        state.request_with_raise(
            || panic!("the ready owner must not be relaunched before raise ack"),
            |_| Ok(true)
        ),
        NativeOpenOutcome::Pending
    );

    assert!(matches!(
        state.poll(|_| Ok(false)),
        Some(NativeOpenOutcome::Failure(reason)) if reason.contains("raise acknowledgement")
    ));
    assert_eq!(state.poll(|_| Ok(true)), None);
    assert_eq!(&*bytes.lock().unwrap(), b"raise\n");
}

#[test]
fn legacy_request_does_not_raise_a_ready_owner() {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let (ready_tx, ready_rx) = mpsc::channel();
    let (_raise_tx, raise_rx) = mpsc::channel();
    let mut state = NativeOpenState::default();
    assert_eq!(
        state.request(
            || {
                Ok(NativeOpenAttempt::with_control(
                    41_u32,
                    ready_rx,
                    SharedWriter(bytes.clone()),
                    raise_rx,
                ))
            },
            |_| Ok(true)
        ),
        NativeOpenOutcome::Pending
    );
    ready_tx.send(Ok(())).unwrap();

    assert_eq!(
        state.request(|| panic!("legacy request must reuse owner"), |_| Ok(true)),
        NativeOpenOutcome::Ready
    );
    assert!(
        bytes.lock().unwrap().is_empty(),
        "Windows path stays unchanged"
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
fn app_launch_keeps_the_same_bounds_and_adds_only_the_explicit_app_target() {
    let executable = Path::new("/usr/local/bin/incodex");
    let app = Path::new("/Applications/Codex Host.app");
    let command = native_open_command_for_app(
        executable,
        Some("250,136,1399,820"),
        app,
        "releases/1.5.1-runtime-hash",
    );
    let arguments = command
        .get_args()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let bounds = command
        .get_envs()
        .find(|(name, _)| *name == OsStr::new("INCODEX_SOURCE_BOUNDS"))
        .and_then(|(_, value)| value)
        .and_then(OsStr::to_str);
    let runtime_release = command
        .get_envs()
        .find(|(name, _)| *name == OsStr::new("INCODEX_LAUNCH_RUNTIME_RELEASE"))
        .and_then(|(_, value)| value)
        .and_then(OsStr::to_str);

    assert_eq!(command.get_program(), executable.as_os_str());
    assert_eq!(arguments, ["open", "--app", app.to_str().unwrap()]);
    assert_eq!(bounds, Some("250,136,1399,820"));
    assert_eq!(runtime_release, Some("releases/1.5.1-runtime-hash"));
}

#[test]
fn only_the_child_opened_line_marks_readiness() {
    let output = format!(
        "Preparing private session\n{}\n",
        crate::open_presentation::OPENED_MESSAGE
    );
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (capability_sender, _capability_receiver) = mpsc::channel();
    let (raise_sender, _raise_receiver) = mpsc::channel();
    super::read_child_output(
        std::io::Cursor::new(output),
        ready_sender,
        capability_sender,
        raise_sender,
    );

    assert_eq!(ready_receiver.try_recv(), Ok(Ok(())));
}

#[test]
fn child_output_ending_before_opened_is_a_readiness_failure() {
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (capability_sender, _capability_receiver) = mpsc::channel();
    let (raise_sender, _raise_receiver) = mpsc::channel();
    super::read_child_output(
        std::io::Cursor::new("still preparing\n"),
        ready_sender,
        capability_sender,
        raise_sender,
    );

    assert!(matches!(ready_receiver.try_recv(), Ok(Err(_))));
}

#[test]
fn child_acknowledges_raise_only_after_opened() {
    let output = format!(
        "{}\n{}\n",
        crate::open_presentation::OPENED_MESSAGE,
        super::NATIVE_OPEN_RAISED
    );
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (capability_sender, _capability_receiver) = mpsc::channel();
    let (raise_sender, raise_receiver) = mpsc::channel();
    super::read_child_output(
        std::io::Cursor::new(output),
        ready_sender,
        capability_sender,
        raise_sender,
    );

    assert_eq!(ready_receiver.try_recv(), Ok(Ok(())));
    assert_eq!(raise_receiver.try_recv(), Ok(Ok(())));
}

#[test]
fn failed_child_raise_acknowledgement_is_preserved_after_initial_ready() {
    let output = format!(
        "{}\n{}\n",
        crate::open_presentation::OPENED_MESSAGE,
        super::NATIVE_OPEN_RAISE_FAILED
    );
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (capability_sender, _capability_receiver) = mpsc::channel();
    let (raise_sender, raise_receiver) = mpsc::channel();
    super::read_child_output(
        std::io::Cursor::new(output),
        ready_sender,
        capability_sender,
        raise_sender,
    );

    assert_eq!(ready_receiver.try_recv(), Ok(Ok(())));
    assert_eq!(
        raise_receiver.try_recv(),
        Ok(Err(
            "native Incodex could not bring the existing private window forward".into()
        ))
    );
}

#[test]
fn child_advertises_raise_control_with_an_exact_closed_marker() {
    let output = format!(
        "{}\n{}\n{}\n",
        super::NATIVE_OPEN_CONTROL_READY,
        crate::open_presentation::OPENED_MESSAGE,
        super::NATIVE_OPEN_RAISED
    );
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (capability_sender, capability_receiver) = mpsc::channel();
    let (raise_sender, raise_receiver) = mpsc::channel();
    super::read_child_output(
        std::io::Cursor::new(output),
        ready_sender,
        capability_sender,
        raise_sender,
    );

    assert_eq!(ready_receiver.try_recv(), Ok(Ok(())));
    assert_eq!(capability_receiver.try_recv(), Ok(()));
    assert_eq!(raise_receiver.try_recv(), Ok(Ok(())));
}

#[test]
fn launch_error_is_returned_without_creating_an_owner() {
    let mut state = NativeOpenState::<u32>::default();
    let outcome = state.request(|| Err("spawn failed".to_string()), |_| Ok(true));

    assert_eq!(outcome, NativeOpenOutcome::Failure("spawn failed".into()));
    assert_eq!(state.poll(|_| Ok(true)), None);
}
