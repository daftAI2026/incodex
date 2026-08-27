//! Native `incodex open` session, process, CDP, and cleanup orchestration.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use incodex_core::paths::{home_dir, user_root};
#[cfg(test)]
use incodex_core::session::copy_settings;
use incodex_core::session::{
    burn_session_home, burn_session_home_with_owner, copy_settings_with_window_geometry,
    create_session_home_for_open, handoff_session_owner, sweep_orphan_sessions,
    target_id_from_exec, BurnExpected, SessionHome, SessionOwnerSnapshot, WindowGeometry,
};
use incodex_core::{format_ok, format_warn};

use crate::app_bundle::resolve_executable;
use crate::cdp::{
    allocate_debug_port, debug_launch_args, inject_shared_ui_with_options_while_alive,
    launch_arg_prefix, monitor_profile_mask_health, start_lifecycle_monitor,
    start_primary_lifecycle_monitor, InjectionOptions, OFFICIAL_NEW_CODEX_URL,
};
use crate::locale::parse_locale_override;
use crate::open_presentation::{
    classify_completed_open, completed_open_failure_message, CompletedOpenState,
    CLOSED_REMOVED_MESSAGE, OPENED_MESSAGE, OPENING_MESSAGE, REMOVING_SESSION_MESSAGE,
    UI_READY_WAIT_MESSAGE, WAITING_MESSAGE,
};
use crate::profile_mask::ProfileMask;

#[derive(Debug, Clone)]
pub struct OpenPlan {
    pub bin: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub home: PathBuf,
    pub chromium: PathBuf,
    pub session_id: String,
    pub session_root: PathBuf,
    pub session_ino: u64,
    pub session_dev: u64,
    pub debug_port: u16,
    pub locale: Option<String>,
    pub profile_mask: Option<ProfileMask>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupResult {
    Removed {
        attempts: u8,
    },
    Retained {
        attempts: u8,
        retained_path: PathBuf,
        reason: String,
    },
}

/// `open` 生命周期向 shell 暴露的稳定退出码。
///
/// 0 表示 session 已删除；1 表示启动或子进程失败；2 表示 session
/// 仍被保留；3 表示 UI/CDP 未通过验收。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum OpenExitCode {
    Success = 0,
    ProcessFailure = 1,
    CleanupRetained = 2,
    UiInjectionFailure = 3,
}

impl OpenExitCode {
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenProcessResult {
    SpawnFailed { error: String },
    Exited { code: i32, ui_ready: bool },
}

impl OpenProcessResult {
    fn exit_code(&self, cleanup: &CleanupResult) -> OpenExitCode {
        if !cleanup.removed() {
            return OpenExitCode::CleanupRetained;
        }
        match self {
            Self::SpawnFailed { .. } => OpenExitCode::ProcessFailure,
            Self::Exited { code, ui_ready } => match classify_completed_open(*code, *ui_ready) {
                CompletedOpenState::Success => OpenExitCode::Success,
                CompletedOpenState::ProcessFailure => OpenExitCode::ProcessFailure,
                CompletedOpenState::UiInjectionFailure => OpenExitCode::UiInjectionFailure,
            },
        }
    }

    fn failure_message(&self, code: OpenExitCode) -> String {
        match code {
            // 保留路径已在 stdout 告警；退出码负责机器可读分类，stderr 不重复。
            OpenExitCode::CleanupRetained => String::new(),
            OpenExitCode::ProcessFailure => match self {
                Self::SpawnFailed { error } => {
                    format!("Unable to start the incognito window: {error}")
                }
                Self::Exited { code, .. } => {
                    completed_open_failure_message(*code, CompletedOpenState::ProcessFailure)
                }
            },
            OpenExitCode::UiInjectionFailure => {
                completed_open_failure_message(0, CompletedOpenState::UiInjectionFailure)
            }
            OpenExitCode::Success => String::new(),
        }
    }
}

enum InjectionStatus {
    Ready,
    Failed(String),
}

#[derive(Debug)]
enum CleanupDisposition {
    Burn,
    Retain(String),
}

#[derive(Debug)]
struct SpawnOutcome {
    process: OpenProcessResult,
    owner: Option<SessionOwnerSnapshot>,
    cleanup: CleanupDisposition,
}

fn publish_injection_status(
    status_tx: &mpsc::Sender<InjectionStatus>,
    readiness: &AtomicBool,
    status: InjectionStatus,
) {
    readiness.store(matches!(status, InjectionStatus::Ready), Ordering::Release);
    let _ = status_tx.send(status);
}

impl CleanupResult {
    pub fn removed(&self) -> bool {
        matches!(self, CleanupResult::Removed { .. })
    }
}

pub fn describe_incognito_open(app_path: &Path) -> Result<(PathBuf, Vec<String>), String> {
    Ok((
        resolve_executable(app_path)?,
        vec!["--user-data-dir=<isolated-chromium>".to_string()],
    ))
}

pub fn default_source_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".codex"))
}

pub fn prepare_incognito_open(
    app_path: &Path,
    user_root: &Path,
    source_home: &Path,
    pid: i32,
) -> Result<OpenPlan, String> {
    prepare_incognito_open_with_profile_mask(app_path, user_root, source_home, pid, None)
}

pub fn prepare_incognito_open_with_profile_mask(
    app_path: &Path,
    user_root: &Path,
    source_home: &Path,
    pid: i32,
    profile_mask: Option<ProfileMask>,
) -> Result<OpenPlan, String> {
    let bin = resolve_executable(app_path)?;
    let live_geometry = incodex_macos::live_main_window_bounds(&bin)
        .ok()
        .flatten()
        .map(|bounds| WindowGeometry {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
        });
    prepare_incognito_open_with_geometry_from_bin(
        bin,
        user_root,
        source_home,
        pid,
        live_geometry,
        profile_mask,
    )
}

#[cfg(test)]
fn prepare_incognito_open_with_geometry(
    app_path: &Path,
    user_root: &Path,
    source_home: &Path,
    pid: i32,
    live_geometry: Option<WindowGeometry>,
) -> Result<OpenPlan, String> {
    let bin = resolve_executable(app_path)?;
    prepare_incognito_open_with_geometry_from_bin(
        bin,
        user_root,
        source_home,
        pid,
        live_geometry,
        None,
    )
}

fn prepare_incognito_open_with_geometry_from_bin(
    bin: PathBuf,
    user_root: &Path,
    source_home: &Path,
    pid: i32,
    live_geometry: Option<WindowGeometry>,
    profile_mask: Option<ProfileMask>,
) -> Result<OpenPlan, String> {
    let target_id = target_id_from_exec(&bin.to_string_lossy());
    let _ = sweep_orphan_sessions(user_root, Some(&target_id));
    let session = create_session_home_for_open(
        user_root,
        Some(&target_id),
        pid,
        &source_home.to_string_lossy(),
    )?;
    if let Err(error) =
        copy_settings_with_window_geometry(&session.home, source_home, live_geometry)
    {
        let _ = burn_session_home(
            &session.root,
            &BurnExpected {
                user_root,
                session_id: Some(&session.session_id),
                ino: Some(session.ino),
                dev: Some(session.dev),
            },
        );
        return Err(error);
    }
    Ok(plan_from_session(bin, session, source_home, profile_mask))
}

fn plan_from_session(
    bin: PathBuf,
    session: SessionHome,
    source_home: &Path,
    profile_mask: Option<ProfileMask>,
) -> OpenPlan {
    let debug_port = allocate_debug_port().unwrap_or(0);
    let args = if debug_port == 0 {
        let mut args = launch_arg_prefix(&session.chromium.display().to_string());
        args.push(OFFICIAL_NEW_CODEX_URL.to_string());
        args
    } else {
        debug_launch_args(&session.chromium.display().to_string(), debug_port)
    };
    OpenPlan {
        args,
        env: vec![
            ("CODEX_HOME".into(), session.home.display().to_string()),
            ("INCODEX_INCOGNITO".into(), "1".into()),
            // Native `open` owns the isolated session and its final burn.
            ("INCODEX_CLEANUP_OWNER".into(), "native".into()),
            ("INCODEX_SESSION_ID".into(), session.session_id.clone()),
            (
                "INCODEX_SESSION_ROOT".into(),
                session.root.display().to_string(),
            ),
            ("INCODEX_SESSION_INO".into(), session.ino.to_string()),
            ("INCODEX_SESSION_DEV".into(), session.dev.to_string()),
            (
                "CODEX_ELECTRON_USER_DATA_PATH".into(),
                session.chromium.display().to_string(),
            ),
            (
                "INCODEX_SOURCE_HOME".into(),
                source_home.display().to_string(),
            ),
        ],
        home: session.home,
        chromium: session.chromium,
        session_id: session.session_id,
        session_root: session.root,
        session_ino: session.ino,
        session_dev: session.dev,
        bin,
        debug_port,
        locale: read_locale_override(source_home),
        profile_mask,
    }
}

fn read_locale_override(source_home: &Path) -> Option<String> {
    let content = std::fs::read_to_string(
        source_home.join(incodex_core::session_layout::CONFIG_SETTING_FILE),
    )
    .ok()?;
    parse_locale_override(&content, &['"'])
}

pub fn format_session_cleanup(cleanup: &CleanupResult) -> (bool, String) {
    match cleanup {
        CleanupResult::Removed { .. } => (true, CLOSED_REMOVED_MESSAGE.into()),
        CleanupResult::Retained {
            retained_path,
            reason,
            ..
        } => (
            false,
            format!(
                "Closed. Isolated session kept at {} ({reason})",
                retained_path.display()
            ),
        ),
    }
}

pub fn wait_and_burn(
    plan: &OpenPlan,
    user_root: &Path,
    retry_delay_ms: u64,
) -> Result<(OpenProcessResult, CleanupResult), String> {
    wait_and_burn_with_owner(
        plan,
        user_root,
        retry_delay_ms,
        spawn_plan_with_owner,
        incodex_macos::quiesce_session_processes,
        |root, expected, owner| match owner {
            Some(owner) => burn_session_home_with_owner(root, expected, owner),
            None => burn_session_home(root, expected),
        },
    )
}

#[cfg(test)]
fn spawn_plan(plan: &OpenPlan) -> Result<OpenProcessResult, String> {
    spawn_plan_with_owner(plan).map(|outcome| outcome.process)
}

fn spawn_plan_with_owner(plan: &OpenPlan) -> Result<SpawnOutcome, String> {
    let mut command = Command::new(&plan.bin);
    command.args(&plan.args);
    for (key, value) in &plan.env {
        command.env(key, value);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command.spawn().map_err(|err| err.to_string())?;
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
    let (status_tx, status_rx) = mpsc::channel();
    let readiness = Arc::new(AtomicBool::new(false));
    let process_alive = Arc::new(AtomicBool::new(true));
    let mut injection_worker = if plan.debug_port != 0 {
        let port = plan.debug_port;
        let injection_readiness = readiness.clone();
        let lifecycle_process_alive = process_alive.clone();
        let options = InjectionOptions {
            locale: plan.locale.clone(),
            profile_mask: plan.profile_mask.clone(),
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
    let mut reported = false;
    loop {
        match status_rx.try_recv() {
            Ok(InjectionStatus::Ready) if !reported => {
                spinner.stop();
                println!("{}", format_ok(OPENED_MESSAGE, None));
                let _ = std::io::stdout().flush();
                spinner = crate::spinner::Spinner::start(WAITING_MESSAGE);
                reported = true;
            }
            Ok(InjectionStatus::Ready) => {}
            Ok(InjectionStatus::Failed(detail)) => {
                spinner.stop();
                if plan.profile_mask.is_some() {
                    println!(
                        "{}",
                        format_warn(&format!("Window closed: {detail}."), None)
                    );
                    let _ = std::io::stdout().flush();
                    stop_injection_worker(&process_alive, &mut injection_worker);
                    return Ok(match kill_and_reap(&mut child) {
                        Ok(_) => SpawnOutcome {
                            process: OpenProcessResult::Exited {
                                code: 0,
                                ui_ready: false,
                            },
                            owner: Some(owner),
                            cleanup: CleanupDisposition::Burn,
                        },
                        Err(error) => {
                            let reason = format!(
                                "UI injection failed and child exit could not be proven: {error}"
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
                reported = true;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => reported = true,
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                stop_injection_worker(&process_alive, &mut injection_worker);
                spinner.stop();
                return Ok(SpawnOutcome {
                    process: OpenProcessResult::Exited {
                        code: status.code().unwrap_or(1),
                        ui_ready: readiness.load(Ordering::Acquire),
                    },
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
        thread::sleep(Duration::from_millis(50));
    }
}

fn start_injection_worker(
    port: u16,
    options: InjectionOptions,
    status_tx: mpsc::Sender<InjectionStatus>,
    readiness: Arc<AtomicBool>,
    process_alive: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut lifecycle_started = false;
        let mut last_injection_error = None;
        for attempt in 1u8..=40 {
            if !process_alive.load(Ordering::Acquire) {
                return;
            }
            if !lifecycle_started
                && start_primary_lifecycle_monitor(port, process_alive.clone()).is_ok()
            {
                lifecycle_started = true;
            }
            let injection = inject_shared_ui_with_options_while_alive(
                port,
                &options,
                &process_alive,
                |target_id| {
                    if !lifecycle_started {
                        start_lifecycle_monitor(port, target_id.to_string(), process_alive.clone());
                        lifecycle_started = true;
                    }
                },
            );
            match injection {
                Ok(_) => {
                    if std::env::var_os("INCODEX_CDP_LOG").is_some() {
                        eprintln!("cdp inject ok on attempt {attempt} port {port}");
                    }
                }
                Err(error) => {
                    if std::env::var_os("INCODEX_CDP_LOG").is_some() {
                        eprintln!("cdp inject attempt {attempt}: {error}");
                    }
                    last_injection_error = Some(error);
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
                        InjectionStatus::Failed(format!("profile mask health failed: {error}")),
                    );
                });
            }
            return;
        }
        let detail = format!(
            "UI injection failed: {}",
            last_injection_error
                .as_deref()
                .unwrap_or("unknown CDP error")
        );
        publish_injection_status(&status_tx, &readiness, InjectionStatus::Failed(detail));
    })
}

fn stop_injection_worker(process_alive: &AtomicBool, worker: &mut Option<thread::JoinHandle<()>>) {
    process_alive.store(false, Ordering::Release);
    if let Some(worker) = worker.take() {
        let _ = worker.join();
    }
}

fn kill_and_reap(child: &mut std::process::Child) -> Result<std::process::ExitStatus, String> {
    let mut probe_error = None;
    match child.try_wait() {
        Ok(Some(status)) => return Ok(status),
        Ok(None) => {}
        Err(error) => probe_error = Some(error.to_string()),
    }
    let mut kill_error = None;
    if let Err(error) = child.kill() {
        kill_error = Some(error.to_string());
    }
    child.wait().map_err(|wait_error| {
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

pub fn wait_and_burn_with<S, B>(
    plan: &OpenPlan,
    user_root: &Path,
    retry_delay_ms: u64,
    spawn: S,
    mut burn: B,
) -> Result<(OpenProcessResult, CleanupResult), String>
where
    S: FnOnce(&OpenPlan) -> Result<OpenProcessResult, String>,
    B: FnMut(&Path, &BurnExpected<'_>) -> Result<bool, String>,
{
    wait_and_burn_with_owner(
        plan,
        user_root,
        retry_delay_ms,
        |plan| {
            spawn(plan).map(|process| SpawnOutcome {
                process,
                owner: None,
                cleanup: CleanupDisposition::Burn,
            })
        },
        |_| Ok(()),
        |root, expected, _owner| burn(root, expected),
    )
}

fn wait_and_burn_with_owner<S, Q, B>(
    plan: &OpenPlan,
    user_root: &Path,
    retry_delay_ms: u64,
    spawn: S,
    quiesce: Q,
    mut burn: B,
) -> Result<(OpenProcessResult, CleanupResult), String>
where
    S: FnOnce(&OpenPlan) -> Result<SpawnOutcome, String>,
    Q: FnOnce(&Path) -> Result<(), String>,
    B: FnMut(&Path, &BurnExpected<'_>, Option<&SessionOwnerSnapshot>) -> Result<bool, String>,
{
    let outcome = match spawn(plan) {
        Ok(outcome) => outcome,
        Err(error) => SpawnOutcome {
            process: OpenProcessResult::SpawnFailed { error },
            owner: None,
            cleanup: CleanupDisposition::Burn,
        },
    };
    let expected = BurnExpected {
        user_root,
        session_id: Some(&plan.session_id),
        ino: Some(plan.session_ino),
        dev: Some(plan.session_dev),
    };
    let cleanup = match outcome.cleanup {
        CleanupDisposition::Burn => {
            let mut spinner = crate::spinner::Spinner::start(REMOVING_SESSION_MESSAGE);
            let cleanup = match quiesce(&plan.session_root) {
                Ok(()) => burn_with_retries_with_owner(
                    &plan.session_root,
                    &expected,
                    outcome.owner.as_ref(),
                    retry_delay_ms,
                    &mut burn,
                ),
                Err(reason) => CleanupResult::Retained {
                    attempts: 0,
                    retained_path: plan.session_root.clone(),
                    reason,
                },
            };
            spinner.stop();
            cleanup
        }
        CleanupDisposition::Retain(reason) => CleanupResult::Retained {
            attempts: 0,
            retained_path: plan.session_root.clone(),
            reason,
        },
    };
    Ok((outcome.process, cleanup))
}

fn burn_with_retries_with_owner<B>(
    session_root: &Path,
    expected: &BurnExpected<'_>,
    owner: Option<&SessionOwnerSnapshot>,
    retry_delay_ms: u64,
    burn: &mut B,
) -> CleanupResult
where
    B: FnMut(&Path, &BurnExpected<'_>, Option<&SessionOwnerSnapshot>) -> Result<bool, String>,
{
    let mut reason = "session directory still present".to_string();
    let mut original_removed = false;
    let late_expected = BurnExpected {
        user_root: expected.user_root,
        session_id: expected.session_id,
        ino: None,
        dev: None,
    };
    for attempt in 1u8..=5 {
        let attempt_expected = if original_removed {
            &late_expected
        } else {
            expected
        };
        let attempt_owner = if original_removed { None } else { owner };
        match burn(session_root, attempt_expected, attempt_owner) {
            Ok(removed) => {
                // 只有已证明删除创建时 root，后续重建才允许路径证明 fallback。
                original_removed |= removed;
            }
            Err(error) => {
                reason = error;
                if attempt == 5 {
                    return if session_root.exists() {
                        CleanupResult::Retained {
                            attempts: attempt,
                            retained_path: session_root.to_path_buf(),
                            reason,
                        }
                    } else {
                        CleanupResult::Removed { attempts: attempt }
                    };
                }
            }
        }
        if !session_root.exists() && !original_removed {
            return CleanupResult::Removed { attempts: attempt };
        }
        // 若原始 root 已证明删除但路径暂时不存在，仍保留有界观察窗口以捕捉迟到重建。
        if attempt < 5 && retry_delay_ms > 0 {
            thread::sleep(Duration::from_millis(retry_delay_ms * u64::from(attempt)));
        }
    }
    if session_root.exists() {
        CleanupResult::Retained {
            attempts: 5,
            retained_path: session_root.to_path_buf(),
            reason,
        }
    } else {
        CleanupResult::Removed { attempts: 5 }
    }
}

#[path = "open_command.rs"]
mod command;
pub use command::run_open;

#[cfg(test)]
#[path = "open_cleanup_tests.rs"]
mod open_cleanup_tests;
#[cfg(test)]
#[path = "open_tests.rs"]
mod open_tests;
