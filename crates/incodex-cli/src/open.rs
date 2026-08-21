use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use incodex_core::paths::{home_dir, user_root};
use incodex_core::session::{
    burn_session_home, copy_settings, create_session_home, sweep_orphan_sessions,
    target_id_from_exec, BurnExpected, SessionHome,
};
use incodex_core::{format_kv, format_ok, format_step, format_warn};

use crate::app_bundle::resolve_executable;
use crate::cdp::{
    allocate_debug_port, debug_launch_args, inject_shared_ui_with_options, start_lifecycle_monitor,
    InjectionOptions, WindowBounds,
};
use crate::parse::ParsedCli;
use crate::CliFailure;

#[derive(Debug, Clone)]
pub struct OpenPlan {
    pub bin: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub home: PathBuf,
    pub chromium: PathBuf,
    pub session_id: String,
    pub session_root: PathBuf,
    pub debug_port: u16,
    pub locale: Option<String>,
    pub source_bounds: Option<WindowBounds>,
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
            Self::Exited { code, .. } if *code != 0 => OpenExitCode::ProcessFailure,
            Self::Exited {
                code: 0,
                ui_ready: false,
            } => OpenExitCode::UiInjectionFailure,
            Self::Exited {
                code: 0,
                ui_ready: true,
            } => OpenExitCode::Success,
            Self::Exited { .. } => OpenExitCode::ProcessFailure,
        }
    }

    fn failure_message(&self, _cleanup: &CleanupResult, code: OpenExitCode) -> String {
        match code {
            // The retained-path warning was already printed on stdout. Do not
            // duplicate it on stderr; the distinct process code carries the
            // machine-readable failure class.
            OpenExitCode::CleanupRetained => String::new(),
            OpenExitCode::ProcessFailure => match self {
                Self::SpawnFailed { error } => {
                    format!("Unable to start the incognito window: {error}")
                }
                Self::Exited { code, .. } => {
                    format!("Incognito Codex process exited with status {code}")
                }
            },
            OpenExitCode::UiInjectionFailure => {
                "Incognito Codex UI injection was not accepted".to_string()
            }
            OpenExitCode::Success => String::new(),
        }
    }
}

enum InjectionStatus {
    Ready,
    Failed(String),
}

#[derive(Clone, Default)]
struct InjectionReadiness {
    ready: Arc<AtomicBool>,
}

impl InjectionReadiness {
    fn mark_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }

    fn observe(&self, status: &InjectionStatus) {
        if matches!(status, InjectionStatus::Ready) {
            self.mark_ready();
        }
    }

    fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
}

fn publish_injection_status(
    status_tx: &mpsc::Sender<InjectionStatus>,
    readiness: &InjectionReadiness,
    status: InjectionStatus,
) {
    if matches!(status, InjectionStatus::Ready) {
        readiness.mark_ready();
    }
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
    let bin = resolve_executable(app_path)?;
    let target_id = target_id_from_exec(&bin.to_string_lossy());
    let _ = sweep_orphan_sessions(user_root, Some(&target_id));
    let session = create_session_home(
        user_root,
        Some(&target_id),
        pid,
        &source_home.to_string_lossy(),
    )?;
    if let Err(error) = copy_settings(&session.home, source_home, user_root) {
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
    Ok(plan_from_session(bin, session, source_home))
}

fn plan_from_session(bin: PathBuf, session: SessionHome, source_home: &Path) -> OpenPlan {
    let debug_port = allocate_debug_port().unwrap_or(0);
    let args = if debug_port == 0 {
        vec![format!("--user-data-dir={}", session.chromium.display())]
    } else {
        debug_launch_args(&session.chromium.display().to_string(), debug_port)
    };
    OpenPlan {
        args,
        env: vec![
            ("CODEX_HOME".into(), session.home.display().to_string()),
            ("INCODEX_INCOGNITO".into(), "1".into()),
            ("INCODEX_SESSION_ID".into(), session.session_id.clone()),
            (
                "INCODEX_SESSION_ROOT".into(),
                session.root.display().to_string(),
            ),
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
        bin,
        debug_port,
        locale: read_locale_override(source_home),
        source_bounds: None,
    }
}

fn read_locale_override(source_home: &Path) -> Option<String> {
    let content = std::fs::read_to_string(source_home.join("config.toml")).ok()?;
    content.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        if name.trim() != "localeOverride" {
            return None;
        }
        value
            .trim()
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

pub fn format_session_cleanup(cleanup: &CleanupResult) -> (bool, String) {
    match cleanup {
        CleanupResult::Removed { .. } => (true, "Closed. Isolated session removed.".into()),
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

fn open_progress_copy() -> (&'static str, &'static str, &'static str) {
    (
        "Opening incognito Codex window",
        "Opened. Incognito Codex window is ready.",
        "Waiting for the window to close",
    )
}

pub fn wait_and_burn(
    plan: &OpenPlan,
    user_root: &Path,
    retry_delay_ms: u64,
) -> Result<(OpenProcessResult, CleanupResult), String> {
    wait_and_burn_with(
        plan,
        user_root,
        retry_delay_ms,
        spawn_plan,
        |root, expected| burn_session_home(root, expected).map_err(|err| err),
    )
}

fn spawn_plan(plan: &OpenPlan) -> Result<OpenProcessResult, String> {
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
    let (status_tx, status_rx) = mpsc::channel();
    let readiness = InjectionReadiness::default();
    if plan.debug_port != 0 {
        let port = plan.debug_port;
        let child_pid = child.id();
        let source_bounds = plan.source_bounds;
        let injection_readiness = readiness.clone();
        let options = InjectionOptions {
            locale: plan.locale.clone(),
        };
        thread::spawn(move || {
            let mut bounds_ready = source_bounds.is_none();
            let mut primary_target_id = None;
            let mut last_injection_error = None;
            for attempt in 1u8..=40 {
                if !bounds_ready {
                    if let Some(bounds) = source_bounds {
                        bounds_ready = incodex_macos::tile_process_front_window(
                            child_pid,
                            (bounds.x, bounds.y, bounds.width, bounds.height),
                            22,
                        )
                        .is_ok();
                    }
                }
                if primary_target_id.is_none() {
                    match inject_shared_ui_with_options(port, &options) {
                        Ok(target_id) => {
                            primary_target_id = Some(target_id);
                            if std::env::var_os("INCODEX_CDP_LOG").is_some() {
                                eprintln!("cdp inject ok on attempt {attempt} port {port}");
                            }
                        }
                        Err(err) => {
                            last_injection_error = Some(err.clone());
                            if std::env::var_os("INCODEX_CDP_LOG").is_some() {
                                eprintln!("cdp inject attempt {attempt}: {err}");
                            }
                        }
                    }
                }
                if bounds_ready {
                    if let Some(target_id) = primary_target_id.take() {
                        start_lifecycle_monitor(port, target_id);
                        publish_injection_status(
                            &status_tx,
                            &injection_readiness,
                            InjectionStatus::Ready,
                        );
                        return;
                    }
                }
                thread::sleep(Duration::from_millis(400));
            }
            let detail = if primary_target_id.is_none() {
                format!(
                    "UI injection failed: {}",
                    last_injection_error
                        .as_deref()
                        .unwrap_or("unknown CDP error")
                )
            } else {
                "the window could not inherit the main window bounds".to_string()
            };
            publish_injection_status(
                &status_tx,
                &injection_readiness,
                InjectionStatus::Failed(detail),
            );
        });
    } else {
        publish_injection_status(
            &status_tx,
            &readiness,
            InjectionStatus::Failed("a localhost CDP port could not be allocated".into()),
        );
    }

    let (_, opened, waiting) = open_progress_copy();
    let mut spinner = crate::spinner::Spinner::start("Waiting for Codex UI to become ready");
    let mut reported = false;
    loop {
        if !reported {
            match status_rx.try_recv() {
                Ok(InjectionStatus::Ready) => {
                    readiness.observe(&InjectionStatus::Ready);
                    spinner.stop();
                    println!("{}", format_ok(opened, None));
                    let _ = std::io::stdout().flush();
                    spinner = crate::spinner::Spinner::start(waiting);
                    reported = true;
                }
                Ok(InjectionStatus::Failed(detail)) => {
                    spinner.stop();
                    println!(
                        "{}",
                        format_warn(&format!("Window opened, but {detail}."), None)
                    );
                    let _ = std::io::stdout().flush();
                    spinner = crate::spinner::Spinner::start(waiting);
                    reported = true;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => reported = true,
            }
        }
        if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
            spinner.stop();
            return Ok(OpenProcessResult::Exited {
                code: status.code().unwrap_or(1),
                ui_ready: readiness.is_ready(),
            });
        }
        thread::sleep(Duration::from_millis(50));
    }
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
    B: FnMut(&Path, &BurnExpected<'_>) -> Result<(), String>,
{
    let process = match spawn(plan) {
        Ok(process) => process,
        Err(error) => OpenProcessResult::SpawnFailed { error },
    };
    let expected = BurnExpected {
        user_root,
        session_id: Some(&plan.session_id),
        ino: None,
        dev: None,
    };
    let cleanup = burn_with_retries(&plan.session_root, &expected, retry_delay_ms, &mut burn);
    Ok((process, cleanup))
}

fn burn_with_retries<B>(
    session_root: &Path,
    expected: &BurnExpected<'_>,
    retry_delay_ms: u64,
    burn: &mut B,
) -> CleanupResult
where
    B: FnMut(&Path, &BurnExpected<'_>) -> Result<(), String>,
{
    let mut reason = "session directory still present".to_string();
    for attempt in 1u8..=5 {
        if let Err(error) = burn(session_root, expected) {
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
        if !session_root.exists() {
            return CleanupResult::Removed { attempts: attempt };
        }
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

pub fn run_open(parsed: &ParsedCli) -> Result<(), CliFailure> {
    let app_path = parsed
        .app
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(incodex_core::DEFAULT_APP));
    if parsed.dry_run {
        let (bin, _) = describe_incognito_open(&app_path).map_err(CliFailure::from)?;
        println!(
            "{}",
            format_step("Open incognito without patching Codex", None)
        );
        println!(
            "{}",
            format_kv("App", &app_path.display().to_string(), None)
        );
        println!("{}", format_kv("Binary", &bin.display().to_string(), None));
        println!("{}", format_warn("Dry run. No window opened.", None));
        return Ok(());
    }
    let root = user_root();
    let source = default_source_home();
    let mut plan = prepare_incognito_open(&app_path, &root, &source, std::process::id() as i32)?;
    if app_path == Path::new(incodex_core::DEFAULT_APP) {
        plan.source_bounds =
            incodex_macos::front_codex_window_bounds().map(|(x, y, width, height)| WindowBounds {
                x,
                y,
                width,
                height,
            });
    }
    let (opening, _, _) = open_progress_copy();
    println!("{}", format_step(opening, None));
    println!(
        "{}",
        format_kv("Binary", &plan.bin.display().to_string(), None)
    );
    println!(
        "{}",
        format_kv("Home", &plan.home.display().to_string(), None)
    );
    println!("{}", format_kv("Session", &plan.session_id, None));
    let (process, cleanup) = wait_and_burn(&plan, &root, 250)?;
    let (ok, message) = format_session_cleanup(&cleanup);
    if ok {
        println!("{}", format_ok(&message, None));
    } else {
        println!("{}", format_warn(&message, None));
    }
    println!();
    let code = process.exit_code(&cleanup);
    if code == OpenExitCode::Success {
        Ok(())
    } else {
        Err(CliFailure::with_code(
            code.as_i32(),
            process.failure_message(&cleanup, code),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_root() -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("incodex-open-unit-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fake_app(root: &Path) -> PathBuf {
        let app = root.join("ChatGPT.app");
        let mac = app.join("Contents/MacOS");
        fs::create_dir_all(&mac).unwrap();
        fs::write(
            app.join("Contents/Info.plist"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>ChatGPT</string></dict></plist>\n",
        )
        .unwrap();
        let executable = mac.join("ChatGPT");
        fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o755);
        }
        fs::set_permissions(executable, permissions).unwrap();
        app
    }

    #[test]
    fn copy_failure_burns_the_session() {
        let root = temp_root();
        let app = fake_app(&root);
        let user = root.join("home");
        let source = root.join("codex");
        fs::create_dir_all(&source).unwrap();
        let bin = resolve_executable(&app).unwrap();
        let target_id = target_id_from_exec(&bin.to_string_lossy());
        let session = create_session_home(&user, Some(&target_id), 1, "").unwrap();
        fs::remove_dir_all(&session.home).unwrap();
        assert!(copy_settings(&session.home, &source, &user).is_err());
        burn_session_home(
            &session.root,
            &BurnExpected {
                user_root: &user,
                session_id: Some(&session.session_id),
                ino: Some(session.ino),
                dev: Some(session.dev),
            },
        )
        .unwrap();
        assert!(!session.root.exists());
    }

    #[test]
    fn open_progress_distinguishes_launch_ready_and_waiting() {
        let (opening, opened, waiting) = open_progress_copy();
        assert_eq!(opening, "Opening incognito Codex window");
        assert_eq!(opened, "Opened. Incognito Codex window is ready.");
        assert_eq!(waiting, "Waiting for the window to close");
    }

    #[test]
    fn ready_published_between_status_poll_and_child_exit_is_not_lost() {
        let (status_tx, status_rx) = mpsc::channel();
        let readiness = InjectionReadiness::default();

        // spawn_plan polls status_rx before child.try_wait. The first poll is
        // empty; the producer then publishes Ready while the child exits.
        assert!(matches!(
            status_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        publish_injection_status(&status_tx, &readiness, InjectionStatus::Ready);

        // No second channel poll happens before this child-exit observation.
        // The producer's acceptance must already be visible here.
        assert!(
            readiness.is_ready(),
            "Ready published between lifecycle polls must survive child exit"
        );
    }

    #[test]
    fn lifecycle_exit_codes_distinguish_process_ui_and_cleanup_failures() {
        let removed = CleanupResult::Removed { attempts: 1 };
        let retained = CleanupResult::Retained {
            attempts: 5,
            retained_path: PathBuf::from("/tmp/session"),
            reason: "EPERM".into(),
        };
        assert_eq!(
            OpenProcessResult::Exited {
                code: 0,
                ui_ready: true,
            }
            .exit_code(&removed),
            OpenExitCode::Success
        );
        assert_eq!(
            OpenProcessResult::SpawnFailed {
                error: "ENOENT".into()
            }
            .exit_code(&removed),
            OpenExitCode::ProcessFailure
        );
        assert_eq!(
            OpenProcessResult::Exited {
                code: 7,
                ui_ready: true,
            }
            .exit_code(&removed),
            OpenExitCode::ProcessFailure
        );
        assert_eq!(
            OpenProcessResult::Exited {
                code: 0,
                ui_ready: false,
            }
            .exit_code(&removed),
            OpenExitCode::UiInjectionFailure
        );
        assert_eq!(
            OpenProcessResult::Exited {
                code: 0,
                ui_ready: true,
            }
            .exit_code(&retained),
            OpenExitCode::CleanupRetained
        );
    }

    #[test]
    fn burn_failure_does_not_claim_removed() {
        let root = temp_root();
        let app = fake_app(&root);
        let user = root.join("home");
        let source = root.join("codex");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("auth.json"), "{}\n").unwrap();
        let plan = prepare_incognito_open(&app, &user, &source, 1).unwrap();
        let (process, cleanup) = wait_and_burn_with(
            &plan,
            &user,
            0,
            |_| {
                Ok(OpenProcessResult::Exited {
                    code: 0,
                    ui_ready: true,
                })
            },
            |_, _| Err("EPERM".into()),
        )
        .unwrap();
        assert_eq!(
            process.exit_code(&cleanup),
            OpenExitCode::CleanupRetained,
            "retained session must have a distinct lifecycle code"
        );
        assert!(plan.session_root.exists());
        assert_eq!(
            cleanup,
            CleanupResult::Retained {
                attempts: 5,
                retained_path: plan.session_root.clone(),
                reason: "EPERM".into(),
            }
        );
        let (ok, message) = format_session_cleanup(&cleanup);
        assert!(!ok);
        assert!(!message.to_lowercase().contains("removed"));
    }

    #[test]
    fn spawn_error_still_burns() {
        let root = temp_root();
        let app = fake_app(&root);
        let user = root.join("home");
        let source = root.join("codex");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("auth.json"), "{}\n").unwrap();
        let plan = prepare_incognito_open(&app, &user, &source, 1).unwrap();
        let (_process, cleanup) = wait_and_burn_with(
            &plan,
            &user,
            0,
            |_| Err("ENOENT".into()),
            |root, expected| burn_session_home(root, expected),
        )
        .unwrap();
        assert!(!plan.session_root.exists());
        assert!(cleanup.removed());
    }

    #[test]
    fn cdp_port_failure_is_not_success() {
        let root = temp_root();
        let app = fake_app(&root);
        let user = root.join("home");
        let source = root.join("codex");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("auth.json"), "{}\n").unwrap();
        let mut plan = prepare_incognito_open(&app, &user, &source, 1).unwrap();
        plan.debug_port = 0;
        let process = spawn_plan(&plan).unwrap();
        assert_eq!(
            process.exit_code(&CleanupResult::Removed { attempts: 1 }),
            OpenExitCode::UiInjectionFailure,
            "missing CDP port must be a UI acceptance failure"
        );
        burn_session_home(
            &plan.session_root,
            &BurnExpected {
                user_root: &user,
                session_id: Some(&plan.session_id),
                ino: None,
                dev: None,
            },
        )
        .unwrap();
    }

    #[test]
    fn locale_override_is_carried_into_the_cdp_injection_plan() {
        let root = temp_root();
        let app = fake_app(&root);
        let user = root.join("home");
        let source = root.join("codex");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("config.toml"),
            "model = \"test\"\nlocaleOverride = \"zh-CN\"\n",
        )
        .unwrap();
        let plan = prepare_incognito_open(&app, &user, &source, 1).unwrap();
        assert_eq!(plan.locale.as_deref(), Some("zh-CN"));
    }
}
