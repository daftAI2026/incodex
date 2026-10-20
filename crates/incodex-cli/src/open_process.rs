//! [INPUT]: 依赖 open 计划、CDP注入、惰性关闭确认与会话owner交接。
//! [OUTPUT]: 提供真实子进程监督；固定窗口空查询时惰性读取CDP主窗快照。
//! [POS]: open 的进程边界；保留真实child wait与原关闭、回收路径。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use super::*;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
#[cfg(not(target_os = "macos"))]
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
#[path = "open_process_spawn.rs"]
mod spawn;

use crate::cdp::{
    inject_shared_ui_with_options_while_alive_with_readiness, is_codex_mode_blocked_error,
    is_codex_mode_waiting_error, is_terminal_codex_mode_error, monitor_profile_mask_health,
    start_lifecycle_monitor, start_primary_lifecycle_monitor, CodexModeReadiness, InjectionOptions,
    CODEX_MODE_POLL_INTERVAL,
};

use super::launch_control;
use super::native_close::{NativeCloseAction, NativeCloseMonitor};

pub(super) trait ChildProcess {
    fn id(&self) -> u32;
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>>;
    fn wait(&mut self) -> io::Result<ExitStatus>;
    fn kill(&mut self) -> io::Result<()>;
    #[cfg(unix)]
    fn signal(&mut self, signal: libc::c_int) -> io::Result<()>;
}

impl ChildProcess for std::process::Child {
    fn id(&self) -> u32 {
        std::process::Child::id(self)
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        std::process::Child::try_wait(self)
    }

    fn wait(&mut self) -> io::Result<ExitStatus> {
        std::process::Child::wait(self)
    }

    fn kill(&mut self) -> io::Result<()> {
        std::process::Child::kill(self)
    }

    #[cfg(unix)]
    fn signal(&mut self, signal: libc::c_int) -> io::Result<()> {
        if unsafe { libc::kill(std::process::Child::id(self) as libc::pid_t, signal) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn spawn_child(
    program: &Path,
    args: &[String],
    env: &[(String, String)],
) -> Result<spawn::MacChild, String> {
    spawn::spawn_macos(program, args, env)
        .map_err(|error| format!("macOS responsibility-aware spawn failed: {error}"))
}

#[cfg(not(target_os = "macos"))]
pub(super) fn spawn_child(
    program: &Path,
    args: &[String],
    env: &[(String, String)],
) -> Result<std::process::Child, String> {
    spawn_std(program, args, env).map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
fn spawn_std(
    program: &Path,
    args: &[String],
    env: &[(String, String)],
) -> io::Result<std::process::Child> {
    let mut command = Command::new(program);
    command.args(args);
    command.envs(env.iter().map(|(key, value)| (key, value)));
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.spawn()
}

#[cfg(test)]
pub(super) fn spawn_plan(plan: &OpenPlan) -> Result<OpenProcessResult, String> {
    spawn_plan_with_owner(plan).map(|outcome| outcome.process)
}

pub(super) fn spawn_plan_with_owner(plan: &OpenPlan) -> Result<SpawnOutcome, String> {
    spawn_plan_with_owner_and_native_close(plan, None)
}

pub(super) fn spawn_plan_with_owner_and_native_close(
    plan: &OpenPlan,
    mut native_close: Option<NativeCloseMonitor>,
) -> Result<SpawnOutcome, String> {
    let readiness = Arc::new(AtomicBool::new(false));
    let process_alive = Arc::new(AtomicBool::new(true));
    let pinned_launch = cfg!(target_os = "macos")
        && native_close.is_some()
        && std::env::var_os("INCODEX_LAUNCH_RUNTIME_RELEASE").is_some();

    let mut child = spawn_child(&plan.bin, &plan.args, &plan.env)?;
    let owner = match handoff_session_owner(&plan.session_root, child.id() as i32) {
        Ok(owner) => owner,
        Err(error) => {
            return Ok(match kill_and_reap(&mut child) {
                Ok(status) => SpawnOutcome {
                    process: OpenProcessResult::Exited {
                        code: status.code().unwrap_or(1),
                        ui_ready: false,
                    },
                    owner: None,
                    cleanup: CleanupDisposition::Burn,
                },
                Err(reap_error) => {
                    let reason = format!(
                        "session owner handoff failed: {error}; child exit could not be proven: {reap_error}"
                    );
                    SpawnOutcome {
                        process: OpenProcessResult::SpawnFailed {
                            error: reason.clone(),
                        },
                        owner: None,
                        cleanup: CleanupDisposition::Retain(reason),
                    }
                }
            });
        }
    };
    let _launch_control_worker = if pinned_launch {
        match launch_control::start_if_authorized(
            true,
            plan.debug_port,
            child.id(),
            plan.profile_mask.is_some(),
            process_alive.clone(),
            readiness.clone(),
        ) {
            Ok(worker) => worker,
            Err(error) => {
                eprintln!("native private-window raise control unavailable: {error}");
                None
            }
        }
    } else {
        None
    };
    let (status_tx, status_rx) = mpsc::channel();
    let mut injection_worker = if plan.debug_port != 0 {
        let port = plan.debug_port;
        let injection_readiness = readiness.clone();
        let lifecycle_process_alive = process_alive.clone();
        let options = InjectionOptions {
            locale: plan.locale.clone(),
            profile_mask: plan.profile_mask.clone(),
            ..InjectionOptions::default()
        };
        Some(start_injection_worker(
            port,
            options,
            status_tx,
            injection_readiness,
            lifecycle_process_alive,
        ))
    } else {
        publish_injection_status(
            &status_tx,
            &readiness,
            InjectionStatus::Failed("a localhost CDP port could not be allocated".into()),
        );
        None
    };

    let mut spinner = crate::spinner::Spinner::start(UI_READY_WAIT_MESSAGE);
    let mut blocker_reported = false;
    let mut ready_reported = false;
    let mut next_window_probe = Instant::now();
    let mut native_window_probe_requested = false;
    loop {
        match status_rx.try_recv() {
            Ok(InjectionStatus::Ready) if !ready_reported => {
                spinner.stop();
                println!("{}", format_ok(OPENED_MESSAGE, None));
                let _ = std::io::stdout().flush();
                spinner = crate::spinner::Spinner::start(WAITING_MESSAGE);
                ready_reported = true;
            }
            Ok(InjectionStatus::Ready) => {}
            Ok(InjectionStatus::NativeWindowProbeRequested) => {
                native_window_probe_requested = true;
            }
            Ok(InjectionStatus::BlockedByOfficialUi) if !blocker_reported => {
                spinner.stop();
                println!("{}", format_warn(OFFICIAL_BLOCKER_WAIT_MESSAGE, None));
                let _ = std::io::stdout().flush();
                spinner = crate::spinner::Spinner::start(OFFICIAL_BLOCKER_WAIT_MESSAGE);
                blocker_reported = true;
            }
            Ok(InjectionStatus::BlockedByOfficialUi) => {}
            Ok(InjectionStatus::ModeUnresolved(detail)) => {
                spinner.stop();
                println!(
                    "{}",
                    format_warn(&format!("Window opened, but {detail}."), None)
                );
                let _ = std::io::stdout().flush();
                spinner = crate::spinner::Spinner::start(WAITING_MESSAGE);
            }
            Ok(InjectionStatus::Failed(detail) | InjectionStatus::RuntimeFailed(detail)) => {
                spinner.stop();
                if plan.profile_mask.is_some() {
                    println!(
                        "{}",
                        format_warn(&format!("Closing window: {detail}."), None)
                    );
                    let _ = std::io::stdout().flush();
                    stop_injection_worker(&process_alive, &mut injection_worker);
                    return Ok(match kill_and_reap(&mut child) {
                        Ok(_) => SpawnOutcome {
                            process: if readiness.load(Ordering::Acquire) {
                                OpenProcessResult::RuntimeFailed { detail }
                            } else {
                                OpenProcessResult::Exited {
                                    code: 0,
                                    ui_ready: false,
                                }
                            },
                            owner: Some(owner),
                            cleanup: CleanupDisposition::Burn,
                        },
                        Err(error) => {
                            let reason = format!(
                                "window supervision failed and child exit could not be proven: {error}"
                            );
                            SpawnOutcome {
                                process: OpenProcessResult::SpawnFailed {
                                    error: reason.clone(),
                                },
                                owner: Some(owner),
                                cleanup: CleanupDisposition::Retain(reason),
                            }
                        }
                    });
                }
                println!(
                    "{}",
                    format_warn(&format!("Window opened, but {detail}."), None)
                );
                let _ = std::io::stdout().flush();
                spinner = crate::spinner::Spinner::start(WAITING_MESSAGE);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                stop_injection_worker(&process_alive, &mut injection_worker);
                spinner.stop();
                return Ok(SpawnOutcome {
                    process: completed_process_result(status, &readiness, &status_rx),
                    owner: Some(owner),
                    cleanup: CleanupDisposition::Burn,
                });
            }
            Ok(None) => {}
            Err(error) => {
                stop_injection_worker(&process_alive, &mut injection_worker);
                spinner.stop();
                let reason = format!("child exit could not be proven: {error}");
                return Ok(SpawnOutcome {
                    process: OpenProcessResult::SpawnFailed {
                        error: reason.clone(),
                    },
                    owner: Some(owner),
                    cleanup: CleanupDisposition::Retain(reason),
                });
            }
        }
        if native_close.is_some()
            && (native_window_probe_requested || Instant::now() >= next_window_probe)
        {
            native_window_probe_requested = false;
            next_window_probe = Instant::now() + Duration::from_millis(250);
            if let Some(lifecycle) = native_close.as_mut() {
                let primary_bounds = if lifecycle.needs_primary_bounds() && ready_reported {
                    launch_control::primary_window_bounds(plan.debug_port, &process_alive).ok()
                } else {
                    None
                };
                if lifecycle.observe(child.id() as i32, primary_bounds, || {
                    launch_control::primary_page_absent(plan.debug_port, &process_alive)
                }) == NativeCloseAction::Close
                {
                    spinner.stop();
                    stop_injection_worker(&process_alive, &mut injection_worker);
                    return Ok(match terminate_and_reap_after_native_close(&mut child) {
                        Ok((status, origin)) => SpawnOutcome {
                            process: native_close_process_result(status, origin),
                            owner: Some(owner),
                            cleanup: CleanupDisposition::Burn,
                        },
                        Err(error) => {
                            let reason = format!(
                                "red-close observed but child exit could not be proven: {error}"
                            );
                            SpawnOutcome {
                                process: OpenProcessResult::SpawnFailed {
                                    error: reason.clone(),
                                },
                                owner: Some(owner),
                                cleanup: CleanupDisposition::Retain(reason),
                            }
                        }
                    });
                }
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// A confirmed red-close authorizes burning the owned session, but cannot
/// turn an abnormal exit of the official child into a successful CLI result.
pub(super) fn native_close_process_result(
    status: ExitStatus,
    origin: NativeCloseExitOrigin,
) -> OpenProcessResult {
    #[cfg(unix)]
    if matches!(
        (origin, status.signal()),
        (NativeCloseExitOrigin::RequestedTerm, Some(libc::SIGTERM))
            | (NativeCloseExitOrigin::RequestedKill, Some(libc::SIGKILL))
    ) {
        return OpenProcessResult::Exited {
            code: 0,
            ui_ready: true,
        };
    }
    #[cfg(not(unix))]
    let _ = origin;
    if let Some(code) = status.code() {
        return OpenProcessResult::Exited {
            code,
            ui_ready: true,
        };
    }
    #[cfg(unix)]
    if let Some(signal) = status.signal() {
        return OpenProcessResult::TerminatedBySignal { signal };
    }
    OpenProcessResult::Exited {
        code: 1,
        ui_ready: true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeCloseExitOrigin {
    Unrequested,
    RequestedTerm,
    RequestedKill,
}

#[cfg(unix)]
pub(super) fn terminate_and_reap_after_native_close(
    child: &mut impl ChildProcess,
) -> Result<(ExitStatus, NativeCloseExitOrigin), String> {
    terminate_and_reap_after_native_close_with_timing(
        child,
        Duration::from_secs(5),
        Duration::from_millis(100),
    )
}

#[cfg(unix)]
pub(super) fn terminate_and_reap_after_native_close_with_timing(
    child: &mut impl ChildProcess,
    term_grace: Duration,
    poll_interval: Duration,
) -> Result<(ExitStatus, NativeCloseExitOrigin), String> {
    let mut probe_error = None;
    match child.try_wait() {
        Ok(Some(status)) => return Ok((status, NativeCloseExitOrigin::Unrequested)),
        Ok(None) => {}
        Err(error) => probe_error = Some(error.to_string()),
    }

    let term_requested = child.signal(libc::SIGTERM).is_ok();
    let deadline = Instant::now() + term_grace;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let origin = if term_requested && status.signal() == Some(libc::SIGTERM) {
                    NativeCloseExitOrigin::RequestedTerm
                } else {
                    NativeCloseExitOrigin::Unrequested
                };
                return Ok((status, origin));
            }
            Ok(None) => {}
            Err(error) => {
                probe_error.get_or_insert_with(|| error.to_string());
            }
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        thread::sleep(poll_interval.min(deadline.saturating_duration_since(now)));
    }

    match child.kill() {
        Ok(()) => {
            let reap_deadline = Instant::now() + term_grace;
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        let origin = if status.signal() == Some(libc::SIGKILL) {
                            NativeCloseExitOrigin::RequestedKill
                        } else if term_requested && status.signal() == Some(libc::SIGTERM) {
                            NativeCloseExitOrigin::RequestedTerm
                        } else {
                            NativeCloseExitOrigin::Unrequested
                        };
                        return Ok((status, origin));
                    }
                    Ok(None) => {}
                    Err(error) => {
                        probe_error.get_or_insert_with(|| error.to_string());
                    }
                }
                let now = Instant::now();
                if now >= reap_deadline {
                    let mut detail =
                        "child exit could not be proven within the SIGKILL reap deadline".to_string();
                    if let Some(probe_error) = probe_error {
                        detail.push_str(&format!("; process probe failed: {probe_error}"));
                    }
                    return Err(detail);
                }
                thread::sleep(poll_interval.min(reap_deadline.saturating_duration_since(now)));
            }
        }
        Err(kill_error) => {
            if let Some(status) = child.try_wait().map_err(|probe_error| {
                format!(
                    "SIGKILL fallback failed: {kill_error}; child exit could not be proven: {probe_error}"
                )
            })? {
                let origin = if term_requested && status.signal() == Some(libc::SIGTERM) {
                    NativeCloseExitOrigin::RequestedTerm
                } else {
                    NativeCloseExitOrigin::Unrequested
                };
                Ok((status, origin))
            } else {
                let mut detail = format!("SIGKILL fallback failed: {kill_error}");
                if let Some(probe_error) = probe_error {
                    detail.push_str(&format!("; process probe failed: {probe_error}"));
                }
                Err(detail)
            }
        }
    }
}

pub(super) fn completed_process_result(
    status: ExitStatus,
    readiness: &AtomicBool,
    statuses: &mpsc::Receiver<InjectionStatus>,
) -> OpenProcessResult {
    #[cfg(unix)]
    if let Some(signal) = status.signal() {
        return OpenProcessResult::TerminatedBySignal { signal };
    }
    // Called after joining the worker: a failure published between the last
    // channel poll and child exit must survive, just like initial acceptance.
    for status in statuses.try_iter() {
        if let InjectionStatus::RuntimeFailed(detail) = status {
            return OpenProcessResult::RuntimeFailed { detail };
        }
    }
    OpenProcessResult::Exited {
        code: status.code().unwrap_or(1),
        ui_ready: readiness.load(Ordering::Acquire),
    }
}

pub(super) fn start_injection_worker(
    port: u16,
    options: InjectionOptions,
    status_tx: mpsc::Sender<InjectionStatus>,
    readiness: Arc<AtomicBool>,
    process_alive: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut lifecycle_started = false;
        let mut mode_readiness = CodexModeReadiness::default();
        let mut blocker_status_sent = false;
        loop {
            if !process_alive.load(Ordering::Acquire) {
                return;
            }
            if !lifecycle_started {
                let lifecycle_status_tx = status_tx.clone();
                if start_primary_lifecycle_monitor(port, process_alive.clone(), move || {
                    let _ = lifecycle_status_tx.send(InjectionStatus::NativeWindowProbeRequested);
                    true
                })
                .is_ok()
                {
                    lifecycle_started = true;
                }
            }
            let injection = inject_shared_ui_with_options_while_alive_with_readiness(
                port,
                &options,
                &process_alive,
                |target_id| {
                    if !lifecycle_started {
                        let lifecycle_status_tx = status_tx.clone();
                        start_lifecycle_monitor(
                            port,
                            target_id.to_string(),
                            process_alive.clone(),
                            move || {
                                let _ = lifecycle_status_tx
                                    .send(InjectionStatus::NativeWindowProbeRequested);
                                true
                            },
                        );
                        lifecycle_started = true;
                    }
                },
                &mut mode_readiness,
            );
            match injection {
                Ok(_) => {
                    if std::env::var_os("INCODEX_CDP_LOG").is_some() {
                        eprintln!("cdp inject ok on port {port}");
                    }
                }
                Err(error) if is_codex_mode_blocked_error(&error) => {
                    if !blocker_status_sent {
                        publish_injection_status(
                            &status_tx,
                            &readiness,
                            InjectionStatus::BlockedByOfficialUi,
                        );
                        blocker_status_sent = true;
                    }
                    thread::sleep(CODEX_MODE_POLL_INTERVAL);
                    continue;
                }
                Err(error) if is_codex_mode_waiting_error(&error) => {
                    thread::sleep(CODEX_MODE_POLL_INTERVAL);
                    continue;
                }
                Err(error) => {
                    if std::env::var_os("INCODEX_CDP_LOG").is_some() {
                        eprintln!("cdp inject failed: {error}");
                    }
                    let terminal = is_terminal_codex_mode_error(&error);
                    if terminal {
                        publish_injection_status(
                            &status_tx,
                            &readiness,
                            terminal_injection_status(error, options.profile_mask.is_some()),
                        );
                        return;
                    }
                    thread::sleep(Duration::from_millis(400));
                    continue;
                }
            }
            publish_injection_status(&status_tx, &readiness, InjectionStatus::Ready);
            if options.profile_mask.is_some() {
                let _ = monitor_profile_mask_health(port, &process_alive, |error| {
                    publish_injection_status(
                        &status_tx,
                        &readiness,
                        InjectionStatus::RuntimeFailed(error.to_string()),
                    );
                });
            }
            return;
        }
    })
}

pub(super) fn stop_injection_worker(
    process_alive: &AtomicBool,
    worker: &mut Option<thread::JoinHandle<()>>,
) {
    process_alive.store(false, Ordering::Release);
    if let Some(worker) = worker.take() {
        let _ = worker.join();
    }
}

pub(super) fn kill_and_reap(child: &mut impl ChildProcess) -> Result<ExitStatus, String> {
    kill_and_reap_with_origin(child).map(|(status, _)| status)
}

pub(super) fn kill_and_reap_with_origin(
    child: &mut impl ChildProcess,
) -> Result<(ExitStatus, bool), String> {
    let mut probe_error = None;
    match child.try_wait() {
        Ok(Some(status)) => return Ok((status, false)),
        Ok(None) => {}
        Err(error) => probe_error = Some(error.to_string()),
    }
    let mut kill_error = None;
    let mut forced_kill = false;
    if let Err(error) = child.kill() {
        kill_error = Some(error.to_string());
    } else {
        forced_kill = true;
    }
    child
        .wait()
        .map(|status| (status, forced_kill))
        .map_err(|wait_error| {
            let mut detail = format!("wait/reap failed: {wait_error}");
            if let Some(error) = probe_error {
                detail.push_str(&format!("; initial wait probe failed: {error}"));
            }
            if let Some(error) = kill_error {
                detail.push_str(&format!("; kill failed: {error}"));
            }
            detail
        })
}
