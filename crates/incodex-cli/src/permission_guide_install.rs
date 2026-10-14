/*
 * [INPUT]: 依赖现有宿主探针、原生双行卡、LaunchServices 与权限变化提示。
 * [OUTPUT]: 安装后先读最终宿主权限；闲置时复用快照，提示到达才复查。
 * [POS]: 共用 GuideOps 的安装适配；生产与隔离共用探针收尾内核。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;

const PREFLIGHT_ENV: &str = "INCODEX_INTERNAL_PERMISSION_PREFLIGHT";
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(super) struct ProbeCache(Option<PermissionSnapshot>);

impl ProbeCache {
    pub(super) fn read(
        &mut self,
        changed: bool,
        read: impl FnOnce() -> PermissionSnapshot,
    ) -> PermissionSnapshot {
        if changed || self.0.is_none() {
            self.0 = Some(read());
        }
        self.0.as_ref().unwrap().clone()
    }
}

struct InstallGuideOps<'a> {
    base: SystemGuideOps<'a>,
    cache: ProbeCache,
    changes: PermissionChangeHint,
}

impl InstallGuideOps<'_> {
    fn read(&mut self, force: bool) -> PermissionSnapshot {
        let changed = self.changes.changed();
        let root = self.base.root;
        let app = self.base.app;
        self.cache.read(force || changed, || {
            cold_snapshot(root, app).unwrap_or(PermissionSnapshot {
                accessibility: AccessibilityStatus::Unknown,
                screen: Some(ScreenPermissionStatus::Unknown),
            })
        })
    }
}

impl GuideOps for InstallGuideOps<'_> {
    fn launch(&mut self) -> Result<(), String> {
        self.base.launch()
    }
    fn probe(&mut self) -> AccessibilityStatus {
        self.read(true).accessibility
    }
    fn probe_screen(&mut self) -> Option<ScreenPermissionStatus> {
        self.read(true).screen
    }
    fn snapshot_after_ax(&mut self, _: AccessibilityStatus) -> PermissionSnapshot {
        self.snapshot()
    }
    fn snapshot(&mut self) -> PermissionSnapshot {
        self.read(false)
    }
    fn observe(&mut self, event: GuideTraceEvent) {
        self.base.observe(event);
    }
    fn record_screen_intent(&mut self) -> Result<(), String> {
        self.base.record_screen_intent()
    }
    fn lock_target(&mut self) -> Result<Option<incodex_transaction::TargetLock>, String> {
        self.base.lock_target()
    }
    fn reset(&mut self) -> Result<(), String> {
        self.base.reset()
    }
    fn reset_screen(&mut self) -> Result<(), String> {
        self.base.reset_screen()
    }
    fn open_settings(&mut self) -> Result<(), String> {
        self.cache.0 = None;
        self.base.open_settings()
    }
    fn open_screen_settings(&mut self) -> Result<(), String> {
        self.cache.0 = None;
        self.base.open_screen_settings()
    }
    fn wait_for_window(&mut self) -> Result<(), String> {
        self.base.wait_for_window()
    }
    fn wait(&mut self, duration: Duration) {
        self.base.wait(duration);
    }
}

pub(crate) fn supports_preflight(app: &Path) -> bool {
    incodex_asar::Archive::open(app.join(incodex_core::paths::ASAR_REL))
        .and_then(|archive| archive.extract(incodex_asar::LOADER_NAME))
        .is_ok_and(|bytes| bytes == incodex_runtime_bundle::loader_source().as_bytes())
}

pub(crate) fn run<F>(root: &Path, app: &Path, verify_target: F) -> Result<Outcome, String>
where
    F: FnMut() -> Result<(), String>,
{
    let mut ops = InstallGuideOps {
        base: SystemGuideOps {
            root,
            app,
            screen: crate::permission_screen_probe::InstalledScreenProbe::for_app(root, app),
        },
        cache: ProbeCache::default(),
        changes: PermissionChangeHint::new(),
    };
    let mut factory = ProcessGuideFactory {
        copy_context: GuideCopyContext::Installing,
    };
    let mut verify_target = crate::accessibility_target::verifier(
        app,
        supports_permission_continuity(app),
        verify_target,
    );
    flow::run_permission_guide_with_mode(
        &mut ops,
        root,
        app,
        &mut verify_target,
        &mut factory,
        flow::GuideTimeouts::for_user(),
        flow::GuideMode::BeforeLaunch,
    )
}

pub(crate) fn launch_after_install<F>(app: &Path, verify_target: F) -> Result<(), String>
where
    F: FnMut() -> Result<(), String>,
{
    launch_after_install_with(
        verify_target,
        || incodex_macos::AppQuiescence::for_app(app)?.ensure_quiescent(),
        || bounded_command(Command::new("/usr/bin/open").arg(app)),
    )
}

pub(super) fn launch_after_install_with(
    mut verify_target: impl FnMut() -> Result<(), String>,
    quiet: impl FnOnce() -> Result<(), String>,
    open: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    verify_target()?;
    // 旧 probe 必须先结束，不能把正常 open 交给仍在等待的无窗实例。
    quiet()?;
    open()
}

// ---- 只注入 OS 动作；nonce 退休与进程收尾顺序由生产内核唯一持有 ----
pub(super) trait ProbeSession {
    fn exited(&mut self) -> Result<bool, String>;
    fn snapshot(&mut self) -> PermissionSnapshot;
    fn observed(&self) -> bool;
    fn retire(&mut self);
    fn finish(&mut self) -> Result<(), String>;
}

pub(super) fn collect_snapshot(
    session: &mut impl ProbeSession,
) -> Result<PermissionSnapshot, String> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let result = loop {
        match session.exited() {
            Ok(true) => break Err("permission probe exited without a response".into()),
            Err(error) => break Err(error),
            Ok(false) => {}
        }
        let snapshot = session.snapshot();
        if session.observed() {
            break Ok(snapshot);
        }
        if Instant::now() >= deadline {
            break Err("permission probe timed out".into());
        }
        thread::sleep(INITIAL_PROBE_INTERVAL);
    };
    // coherent_snapshot 已完成第二次身份核验。只退休本次 nonce。
    session.retire();
    session.finish()?;
    result
}

struct InstalledProbeSession<'a> {
    ops: Option<SystemGuideOps<'a>>,
    launcher: Child,
    target: incodex_macos::AppQuiescence,
    profile: ProbeProfile,
}

impl ProbeSession for InstalledProbeSession<'_> {
    fn exited(&mut self) -> Result<bool, String> {
        self.launcher
            .try_wait()
            .map(|status| status.is_some())
            .map_err(|error| error.to_string())
    }
    fn snapshot(&mut self) -> PermissionSnapshot {
        self.ops.as_mut().unwrap().snapshot()
    }
    fn observed(&self) -> bool {
        self.ops
            .as_ref()
            .and_then(|ops| ops.screen.as_ref())
            .is_some_and(|probe| probe.has_observation())
    }
    fn retire(&mut self) {
        drop(self.ops.take());
    }
    fn finish(&mut self) -> Result<(), String> {
        let exited = wait_for_launcher(&mut self.launcher);
        let quiet = self.target.ensure_quiescent();
        if quiet.is_ok() {
            self.profile.cleanup = true;
        }
        exited?;
        quiet
    }
}

fn cold_snapshot(root: &Path, app: &Path) -> Result<PermissionSnapshot, String> {
    let target = incodex_macos::AppQuiescence::for_app(app)?;
    target.ensure_quiescent()?;
    let screen = crate::permission_screen_probe::InstalledScreenProbe::for_app(root, app)
        .ok_or("installed permission probe is unavailable")?;
    let context = screen
        .preflight_context(app)
        .ok_or("permission request changed")?;
    let mut profile = ProbeProfile::new()?;
    profile.cleanup = true;
    let launcher = probe_launch_command(app, &context, &profile.path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot launch permission probe: {error}"))?;
    profile.cleanup = false;
    let mut session = InstalledProbeSession {
        ops: Some(SystemGuideOps {
            root,
            app,
            screen: Some(screen),
        }),
        launcher,
        target,
        profile,
    };
    collect_snapshot(&mut session)
}

pub(super) fn probe_launch_command(app: &Path, context: &Value, profile: &Path) -> Command {
    // LaunchServices 建立 App 责任域，不能直接继承终端的 TCC 主体。
    let mut command = Command::new("/usr/bin/open");
    command
        .args(["-n", "-g", "-j", "-W", "--env"])
        .arg(format!("{PREFLIGHT_ENV}={context}"))
        .arg("--env")
        .arg(format!(
            "CODEX_ELECTRON_USER_DATA_PATH={}",
            profile.display()
        ))
        .arg(app)
        .arg("--args")
        .arg(format!("--user-data-dir={}", profile.display()));
    command
}

pub(super) fn wait_for_launcher(child: &mut Child) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("permission launch exited with {status}")),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            _ => {
                // 仅回收自有 open 等待器。宿主自行退出，绝不强退 App。
                let _ = child.kill();
                let _ = child.wait();
                return Err("permission probe did not exit; the work app was not opened".into());
            }
        }
    }
}

struct ProbeProfile {
    path: PathBuf,
    cleanup: bool,
}

impl ProbeProfile {
    fn new() -> Result<Self, String> {
        use std::ffi::{CStr, CString};
        use std::os::unix::ffi::OsStrExt;
        let template = env::temp_dir().join("incodex-permission-XXXXXX");
        let mut bytes = CString::new(template.as_os_str().as_bytes())
            .map_err(|error| error.to_string())?
            .into_bytes_with_nul();
        let created = unsafe { libc::mkdtemp(bytes.as_mut_ptr().cast()) };
        if created.is_null() {
            return Err(format!(
                "cannot create permission probe profile: {}",
                io::Error::last_os_error()
            ));
        }
        let path = PathBuf::from(std::ffi::OsStr::from_bytes(
            unsafe { CStr::from_ptr(created) }.to_bytes(),
        ));
        Ok(Self {
            path,
            cleanup: false,
        })
    }
}

impl Drop for ProbeProfile {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

// ---- 仅在当前 CLI owner 生命周期内观察变化提示 ----
struct PermissionChangeHint(Option<i32>);

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn notify_register_check(name: *const libc::c_char, token: *mut i32) -> u32;
    fn notify_check(token: i32, changed: *mut i32) -> u32;
    fn notify_cancel(token: i32) -> u32;
}

impl PermissionChangeHint {
    fn new() -> Self {
        #[cfg(target_os = "macos")]
        {
            let mut token = 0;
            let status = unsafe {
                notify_register_check(c"com.apple.tcc.access.changed".as_ptr(), &mut token)
            };
            Self((status == 0).then_some(token))
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self(None)
        }
    }
    fn changed(&self) -> bool {
        #[cfg(target_os = "macos")]
        if let Some(token) = self.0 {
            let mut changed = 0;
            return unsafe { notify_check(token, &mut changed) == 0 } && changed != 0;
        }
        false
    }
}

impl Drop for PermissionChangeHint {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(token) = self.0 {
            unsafe {
                notify_cancel(token);
            }
        }
    }
}
