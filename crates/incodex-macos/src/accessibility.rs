//! Read-only Accessibility/TCC inspection for the exact executable of a live app.
//!
//! The process list is only used to identify the exact host executable.  The TCC decision is
//! made for that host's audit token, never for the caller (the CLI or this library test process).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::{AppQuiescence, ProcessProbe, SystemProcessProbe};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessibilityStatus {
    Granted,
    Denied,
    Unknown,
    NotRunning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessibilityReport {
    pub status: AccessibilityStatus,
    pub pid: Option<i32>,
    pub reason: Option<String>,
}

impl AccessibilityReport {
    fn status(status: AccessibilityStatus, pid: Option<i32>) -> Self {
        Self {
            status,
            pid,
            reason: None,
        }
    }

    fn unknown(reason: impl Into<String>) -> Self {
        Self {
            status: AccessibilityStatus::Unknown,
            pid: None,
            reason: Some(reason.into()),
        }
    }

    fn unknown_for_pid(pid: i32, reason: impl Into<String>) -> Self {
        Self {
            status: AccessibilityStatus::Unknown,
            pid: Some(pid),
            reason: Some(reason.into()),
        }
    }
}

/// Inspect Accessibility for the exact executable inside `app`.
///
/// This function performs no request, prompt, reset, or mutation.  A non-macOS build has no
/// meaningful TCC answer and therefore returns `Unknown` rather than guessing.
pub fn inspect_accessibility_for_app(app: &Path) -> AccessibilityReport {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        return AccessibilityReport::unknown("Accessibility inspection is only supported on macOS");
    }

    #[cfg(target_os = "macos")]
    {
        let quiescence = match AppQuiescence::for_app(app) {
            Ok(quiescence) => quiescence,
            Err(error) => return AccessibilityReport::unknown(error),
        };
        let process_probe = SystemProcessProbe;
        let tcc_probe = SystemTccAccessibilityProbe::default();
        inspect_accessibility_for_executable(quiescence.executable(), &process_probe, &tcc_probe)
    }
}

/// Test seam for the pure decision layer.  The default public entry point above always uses the
/// real process table and the real read-only TCC/audit-token probe.
#[allow(dead_code)]
pub(crate) fn inspect_accessibility_for_app_with<P, T>(
    app: &Path,
    process_probe: &P,
    tcc_probe: &T,
) -> AccessibilityReport
where
    P: ProcessProbe,
    T: TccAccessibilityProbe,
{
    // Keep the injected seam path-exact.  The production entry point canonicalizes the app
    // before probing the kernel path; fixture probes intentionally provide that path directly.
    let quiescence = match AppQuiescence::for_bundle_at(app, app) {
        Ok(quiescence) => quiescence,
        Err(error) => return AccessibilityReport::unknown(error),
    };
    inspect_accessibility_for_executable(quiescence.executable(), process_probe, tcc_probe)
}

fn inspect_accessibility_for_executable<P, T>(
    executable: &Path,
    process_probe: &P,
    tcc_probe: &T,
) -> AccessibilityReport
where
    P: ProcessProbe,
    T: TccAccessibilityProbe,
{
    let first_snapshot = match process_probe.process_paths() {
        Ok(paths) => paths,
        Err(error) => {
            return AccessibilityReport::unknown(format!(
                "cannot inspect running app processes: {error}"
            ))
        }
    };
    let pid = match unique_matching_pid(&first_snapshot, executable) {
        Ok(Some(pid)) => pid,
        Ok(None) => return AccessibilityReport::status(AccessibilityStatus::NotRunning, None),
        Err(error) => return AccessibilityReport::unknown(error),
    };

    let allowed = match tcc_probe.check_accessibility(pid) {
        Ok(allowed) => allowed,
        Err(error) => return AccessibilityReport::unknown_for_pid(pid, error),
    };

    let second_snapshot = match process_probe.process_paths() {
        Ok(paths) => paths,
        Err(error) => {
            return AccessibilityReport::unknown(format!(
                "cannot revalidate running app process {pid}: {error}"
            ))
        }
    };
    if let Err(error) = revalidate_pid(&second_snapshot, executable, pid) {
        return AccessibilityReport::unknown(error);
    }
    if let Err(error) = tcc_probe.revalidate_process_identity(pid) {
        return AccessibilityReport::unknown(error);
    }

    AccessibilityReport::status(
        if allowed {
            AccessibilityStatus::Granted
        } else {
            AccessibilityStatus::Denied
        },
        Some(pid),
    )
}

fn unique_matching_pid(
    process_paths: &[(i32, PathBuf)],
    executable: &Path,
) -> Result<Option<i32>, String> {
    let mut seen_pids = HashSet::with_capacity(process_paths.len());
    let mut matches = Vec::new();
    for (pid, path) in process_paths {
        if *pid <= 0 {
            return Err(format!("process probe returned invalid PID {pid}"));
        }
        if !seen_pids.insert(*pid) {
            return Err(format!("process probe returned duplicate PID {pid}"));
        }
        if path == executable {
            matches.push(*pid);
        }
    }
    match matches.as_slice() {
        [] => Ok(None),
        [pid] => Ok(Some(*pid)),
        _ => Err(format!(
            "multiple running processes match the exact app executable: {:?}",
            matches
        )),
    }
}

fn revalidate_pid(
    process_paths: &[(i32, PathBuf)],
    executable: &Path,
    expected_pid: i32,
) -> Result<(), String> {
    match unique_matching_pid(process_paths, executable)? {
        Some(pid) if pid == expected_pid => Ok(()),
        Some(pid) => Err(format!(
            "app process identity changed while checking Accessibility (expected pid {expected_pid}, observed pid {pid})"
        )),
        None if process_paths.iter().any(|(pid, _)| *pid == expected_pid) => Err(format!(
            "pid {expected_pid} was reused or its executable changed while checking Accessibility"
        )),
        None => Err(format!(
            "app process {expected_pid} exited while checking Accessibility"
        )),
    }
}

pub(crate) trait TccAccessibilityProbe {
    fn check_accessibility(&self, pid: i32) -> Result<bool, String>;

    fn revalidate_process_identity(&self, _pid: i32) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct SystemTccAccessibilityProbe {
    last_token: std::sync::Mutex<Option<AuditToken>>,
}

#[cfg(target_os = "macos")]
impl TccAccessibilityProbe for SystemTccAccessibilityProbe {
    fn check_accessibility(&self, pid: i32) -> Result<bool, String> {
        let token_before = audit_token_for_pid(pid)?;
        validate_audit_token(&token_before, pid)?;
        let api = TccApi::load()?;
        let granted = unsafe { (api.check)(api.service, token_before, std::ptr::null()) } != 0;

        // A PID can be recycled while the TCC call is in flight.  The audit token's
        // pidversion is the kernel generation identity that survives that race; a path-only
        // recheck cannot distinguish a same-path restart.
        let token_after = audit_token_for_pid(pid)?;
        validate_audit_token(&token_after, pid)?;
        if token_before.val[7] != token_after.val[7] {
            return Err(format!(
                "pid {pid} was reused while checking Accessibility (pidversion {} -> {})",
                token_before.val[7], token_after.val[7]
            ));
        }
        let mut last_token = self
            .last_token
            .lock()
            .map_err(|_| "TCC audit-token state lock is poisoned".to_string())?;
        *last_token = Some(token_after);
        Ok(granted)
    }

    fn revalidate_process_identity(&self, pid: i32) -> Result<(), String> {
        let expected = {
            let last_token = self
                .last_token
                .lock()
                .map_err(|_| "TCC audit-token state lock is poisoned".to_string())?;
            last_token.ok_or_else(|| "TCC audit-token identity was not retained".to_string())?
        };
        let current = audit_token_for_pid(pid)?;
        validate_audit_token(&current, pid)?;
        if expected.val[7] != current.val[7] {
            return Err(format!(
                "pid {pid} was reused after the process snapshot (pidversion {} -> {})",
                expected.val[7], current.val[7]
            ));
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct AuditToken {
    val: [u32; 8],
}

#[cfg(target_os = "macos")]
const TASK_AUDIT_TOKEN: libc::task_flavor_t = 15;

#[cfg(target_os = "macos")]
const TASK_AUDIT_TOKEN_COUNT: libc::mach_msg_type_number_t = 8;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    #[link_name = "mach_task_self_"]
    static MACH_TASK_SELF: libc::mach_port_t;
    fn task_name_for_pid(
        target_tport: libc::mach_port_t,
        pid: libc::pid_t,
        tn: *mut libc::mach_port_t,
    ) -> libc::kern_return_t;
    fn mach_port_deallocate(
        task: libc::mach_port_t,
        name: libc::mach_port_t,
    ) -> libc::kern_return_t;
}

#[cfg(target_os = "macos")]
fn audit_token_for_pid(pid: i32) -> Result<AuditToken, String> {
    let self_task = unsafe { MACH_TASK_SELF };
    let mut task = 0;
    let result = unsafe { task_name_for_pid(self_task, pid as libc::pid_t, &mut task) };
    if result != 0 || task == 0 {
        return Err(format!(
            "task_name_for_pid({pid}) failed with kern_return {result}"
        ));
    }

    let mut token = AuditToken::default();
    let mut count = TASK_AUDIT_TOKEN_COUNT;
    let task_info_result = unsafe {
        libc::task_info(
            task,
            TASK_AUDIT_TOKEN,
            (&mut token as *mut AuditToken).cast(),
            &mut count,
        )
    };
    let deallocate_result = unsafe { mach_port_deallocate(self_task, task) };
    if task_info_result != 0 {
        return Err(format!(
            "task_info(TASK_AUDIT_TOKEN) for pid {pid} failed with kern_return {task_info_result}"
        ));
    }
    if count != TASK_AUDIT_TOKEN_COUNT {
        return Err(format!(
            "task_info(TASK_AUDIT_TOKEN) for pid {pid} returned unexpected count {count}"
        ));
    }
    if deallocate_result != 0 {
        return Err(format!(
            "mach_port_deallocate for pid {pid} failed with kern_return {deallocate_result}"
        ));
    }
    Ok(token)
}

#[cfg(target_os = "macos")]
fn validate_audit_token(token: &AuditToken, pid: i32) -> Result<(), String> {
    let uid = unsafe { libc::getuid() as u32 };
    let euid = unsafe { libc::geteuid() as u32 };
    if token.val[5] as i32 != pid {
        return Err(format!(
            "audit token pid {} does not match requested pid {pid}",
            token.val[5]
        ));
    }
    if token.val[0] != uid || token.val[1] != euid {
        return Err(format!(
            "audit token uid/euid {}/{} does not match current {uid}/{euid}",
            token.val[0], token.val[1]
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
type CfStringRef = *const std::ffi::c_void;

#[cfg(target_os = "macos")]
type CfDictionaryRef = *const std::ffi::c_void;

#[cfg(target_os = "macos")]
type TccAccessCheckAuditTokenFn =
    unsafe extern "C" fn(CfStringRef, AuditToken, CfDictionaryRef) -> u8;

#[cfg(target_os = "macos")]
struct FrameworkHandle(*mut std::ffi::c_void);

#[cfg(target_os = "macos")]
impl FrameworkHandle {
    fn open(path: &str) -> Result<Self, String> {
        let path = std::ffi::CString::new(path).map_err(|error| error.to_string())?;
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
        if handle.is_null() {
            return Err(dynamic_loader_error("cannot load TCC framework"));
        }
        Ok(Self(handle))
    }

    fn symbol(&self, name: &str) -> Result<*mut std::ffi::c_void, String> {
        let name_c = std::ffi::CString::new(name).map_err(|error| error.to_string())?;
        unsafe {
            libc::dlerror();
        }
        let symbol = unsafe { libc::dlsym(self.0, name_c.as_ptr()) };
        if symbol.is_null() {
            return Err(dynamic_loader_error(&format!(
                "TCC framework is missing symbol {name}"
            )));
        }
        Ok(symbol)
    }

    fn data_pointer(&self, name: &str) -> Result<*const std::ffi::c_void, String> {
        let symbol = self.symbol(name)?;
        let value = unsafe { *(symbol as *const *const std::ffi::c_void) };
        if value.is_null() {
            return Err(format!("TCC framework returned a null {name}"));
        }
        Ok(value)
    }
}

#[cfg(target_os = "macos")]
impl Drop for FrameworkHandle {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.0);
        }
    }
}

#[cfg(target_os = "macos")]
fn dynamic_loader_error(prefix: &str) -> String {
    let error = unsafe { libc::dlerror() };
    if error.is_null() {
        return prefix.to_string();
    }
    let detail = unsafe { std::ffi::CStr::from_ptr(error) }.to_string_lossy();
    format!("{prefix}: {detail}")
}

#[cfg(target_os = "macos")]
struct TccApi {
    _framework: FrameworkHandle,
    check: TccAccessCheckAuditTokenFn,
    service: CfStringRef,
}

#[cfg(target_os = "macos")]
impl TccApi {
    fn load() -> Result<Self, String> {
        let framework = FrameworkHandle::open(
            "/System/Library/PrivateFrameworks/TCC.framework/Versions/A/TCC",
        )?;
        let check = unsafe {
            std::mem::transmute::<*mut std::ffi::c_void, TccAccessCheckAuditTokenFn>(
                framework.symbol("TCCAccessCheckAuditToken")?,
            )
        };
        let service = framework.data_pointer("kTCCServiceAccessibility")?;
        Ok(Self {
            _framework: framework,
            check,
            service,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::ProcessProbe;
    #[cfg(not(target_os = "macos"))]
    use super::inspect_accessibility_for_app;
    use super::{inspect_accessibility_for_app_with, AccessibilityStatus, TccAccessibilityProbe};
    use std::collections::VecDeque;

    type ProcessSnapshotResult = Result<Vec<(i32, PathBuf)>, String>;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_SERIAL: AtomicU64 = AtomicU64::new(0);

    #[derive(Clone)]
    struct FixtureProcessProbe {
        snapshots: Arc<Mutex<VecDeque<ProcessSnapshotResult>>>,
    }

    impl FixtureProcessProbe {
        fn new(snapshots: Vec<ProcessSnapshotResult>) -> Self {
            Self {
                snapshots: Arc::new(Mutex::new(snapshots.into_iter().collect())),
            }
        }
    }

    impl ProcessProbe for FixtureProcessProbe {
        fn process_paths(&self) -> Result<Vec<(i32, PathBuf)>, String> {
            self.snapshots
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err("fixture process snapshot exhausted".into()))
        }
    }

    #[derive(Clone)]
    enum FixtureTccOutcome {
        Missing,
        Failed,
        Decision(bool),
    }

    #[derive(Clone)]
    struct FixtureTcc {
        outcome: FixtureTccOutcome,
        calls: Arc<Mutex<Vec<i32>>>,
    }

    impl FixtureTcc {
        fn missing() -> Self {
            Self::new(FixtureTccOutcome::Missing)
        }

        fn failed() -> Self {
            Self::new(FixtureTccOutcome::Failed)
        }

        fn decision(value: bool) -> Self {
            Self::new(FixtureTccOutcome::Decision(value))
        }

        fn new(outcome: FixtureTccOutcome) -> Self {
            Self {
                outcome,
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn calls(&self) -> Vec<i32> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl TccAccessibilityProbe for FixtureTcc {
        fn check_accessibility(&self, pid: i32) -> Result<bool, String> {
            self.calls.lock().unwrap().push(pid);
            match &self.outcome {
                FixtureTccOutcome::Missing => Err("TCCAccessCheckAuditToken is unavailable".into()),
                FixtureTccOutcome::Failed => Err("TCC access check failed".into()),
                FixtureTccOutcome::Decision(value) => Ok(*value),
            }
        }
    }

    struct IdentityChangingTcc;

    impl TccAccessibilityProbe for IdentityChangingTcc {
        fn check_accessibility(&self, _pid: i32) -> Result<bool, String> {
            Ok(true)
        }

        fn revalidate_process_identity(&self, _pid: i32) -> Result<(), String> {
            Err("audit token pidversion changed after process snapshot".into())
        }
    }

    fn app_fixture() -> (PathBuf, PathBuf) {
        let root = loop {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let serial = FIXTURE_SERIAL.fetch_add(1, Ordering::Relaxed);
            let candidate = std::env::temp_dir().join(format!(
                "incodex-accessibility-test-{}-{stamp}-{serial}",
                std::process::id(),
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!(
                    "cannot create fixture root {}: {error}",
                    candidate.display()
                ),
            }
        };
        let app = root.join("ChatGPT.app");
        let executable = app.join("Contents/MacOS/ChatGPT");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(
            app.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.openai.codex</string>
<key>CFBundleExecutable</key><string>ChatGPT</string>
</dict></plist>
"#,
        )
        .unwrap();
        fs::write(&executable, b"fixture executable").unwrap();
        (app, executable)
    }

    fn stable_probe(pid: i32, executable: &Path) -> FixtureProcessProbe {
        let snapshot = vec![(pid, executable.to_path_buf())];
        FixtureProcessProbe::new(vec![Ok(snapshot.clone()), Ok(snapshot)])
    }

    fn assert_unknown(report: &super::AccessibilityReport) {
        assert_eq!(report.status, AccessibilityStatus::Unknown);
        assert!(report.reason.is_some(), "unknown result must explain why");
    }

    #[test]
    fn no_exactly_matching_running_host_is_not_running() {
        let (app, _executable) = app_fixture();
        let probe = FixtureProcessProbe::new(vec![Ok(Vec::new())]);
        let tcc = FixtureTcc::decision(true);

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_eq!(report.status, AccessibilityStatus::NotRunning);
        assert_eq!(report.pid, None);
        assert!(tcc.calls().is_empty());
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn same_basename_in_another_bundle_is_not_running() {
        let (app, _executable) = app_fixture();
        let probe = FixtureProcessProbe::new(vec![Ok(vec![(
            41,
            PathBuf::from("/tmp/another/ChatGPT.app/Contents/MacOS/ChatGPT"),
        )])]);
        let tcc = FixtureTcc::decision(true);

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_eq!(report.status, AccessibilityStatus::NotRunning);
        assert_eq!(report.pid, None);
        assert!(tcc.calls().is_empty());
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn multiple_exact_hosts_fail_closed_without_randomly_checking_one() {
        let (app, executable) = app_fixture();
        let probe = FixtureProcessProbe::new(vec![Ok(vec![
            (41, executable.clone()),
            (42, executable.clone()),
        ])]);
        let tcc = FixtureTcc::decision(true);

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_unknown(&report);
        assert_eq!(report.pid, None);
        assert!(tcc.calls().is_empty());
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn duplicate_pid_or_reused_pid_between_snapshots_fails_closed() {
        let (app, executable) = app_fixture();
        let other_executable = executable.with_file_name("OtherHost");
        let probe = FixtureProcessProbe::new(vec![
            Ok(vec![(41, executable.clone())]),
            Ok(vec![(41, other_executable)]),
        ]);
        let tcc = FixtureTcc::decision(true);

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_unknown(&report);
        assert_eq!(
            report.pid, None,
            "a reused PID is no longer a trusted identity"
        );
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn duplicate_pid_entries_are_ambiguous_even_when_the_path_matches() {
        let (app, executable) = app_fixture();
        let probe = FixtureProcessProbe::new(vec![Ok(vec![
            (41, executable.clone()),
            (41, executable.clone()),
        ])]);
        let tcc = FixtureTcc::decision(true);

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_unknown(&report);
        assert_eq!(report.pid, None);
        assert!(tcc.calls().is_empty());
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn missing_tcc_symbol_is_unknown() {
        let (app, executable) = app_fixture();
        let probe = stable_probe(41, &executable);
        let tcc = FixtureTcc::missing();

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_unknown(&report);
        assert_eq!(report.pid, Some(41));
        assert_eq!(tcc.calls(), vec![41]);
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn tcc_call_failure_is_unknown() {
        let (app, executable) = app_fixture();
        let probe = stable_probe(41, &executable);
        let tcc = FixtureTcc::failed();

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_unknown(&report);
        assert_eq!(report.pid, Some(41));
        assert_eq!(tcc.calls(), vec![41]);
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn explicit_tcc_false_is_denied() {
        let (app, executable) = app_fixture();
        let probe = stable_probe(41, &executable);
        let tcc = FixtureTcc::decision(false);

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_eq!(report.status, AccessibilityStatus::Denied);
        assert_eq!(report.pid, Some(41));
        assert_eq!(tcc.calls(), vec![41]);
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn explicit_tcc_true_is_granted() {
        let (app, executable) = app_fixture();
        let probe = stable_probe(41, &executable);
        let tcc = FixtureTcc::decision(true);

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_eq!(report.status, AccessibilityStatus::Granted);
        assert_eq!(report.pid, Some(41));
        assert_eq!(tcc.calls(), vec![41]);
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn audit_token_generation_change_after_process_snapshot_is_unknown() {
        let (app, executable) = app_fixture();
        let probe = stable_probe(41, &executable);

        let report = inspect_accessibility_for_app_with(&app, &probe, &IdentityChangingTcc);

        assert_unknown(&report);
        assert_eq!(report.pid, None);
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn process_probe_failure_is_unknown() {
        let (app, _executable) = app_fixture();
        let probe = FixtureProcessProbe::new(vec![Err("process table unavailable".into())]);

        let report = inspect_accessibility_for_app_with(&app, &probe, &FixtureTcc::decision(true));

        assert_unknown(&report);
        assert_eq!(report.pid, None);
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_macos_is_explicitly_unsupported_as_unknown() {
        let report = inspect_accessibility_for_app(Path::new("/tmp/does-not-matter.app"));

        assert_eq!(report.status, AccessibilityStatus::Unknown);
        assert_eq!(report.pid, None);
        assert!(report.reason.is_some());
    }
}
