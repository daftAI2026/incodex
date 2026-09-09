// 登录观察者的事件顺序；恢复执行仍属于既有安装事务。
use std::path::Path;
use std::ptr;
use std::sync::Arc;
use windows::ApplicationModel::{PackageCatalog, PackageUpdatingEventArgs};
use windows::Foundation::TypedEventHandler;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_INVALID_PARAMETER,
    HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, OpenMutexW, OpenProcess, ReleaseMutex, ResetEvent,
    SetEvent, WaitForMultipleObjects, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE,
    MUTEX_MODIFY_STATE, PROCESS_SYNCHRONIZE, SYNCHRONIZATION_SYNCHRONIZE,
};

use crate::windows_app::{discover_codex_package, CODEX_PACKAGE_FAMILY_NAME};
use crate::windows_install_state::{
    acquire_windows_install_state, read_windows_install_state, read_windows_update_repair_intent,
    WindowsInstallPhase, WindowsInstallState,
};
use crate::windows_process::strict_running_codex_package_process_ids;
use crate::windows_update_repair::{
    repair_windows_runtime_after_update_with, resume_windows_update_repair_with,
    WindowsRuntimeApartment, WindowsUpdateRepairAuthorization,
};

pub(crate) const MODE: &str = "--incodex-windows-update-observer";
const START_TIMEOUT_MS: u32 = 15_000;

struct OwnedHandle(HANDLE);
// 内核事件可跨线程发信号；Arc 保证回调结束前不关闭句柄。
unsafe impl Send for OwnedHandle {}
unsafe impl Sync for OwnedHandle {}
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}
struct OwnerLock(OwnedHandle);
impl Drop for OwnerLock {
    fn drop(&mut self) {
        unsafe { ReleaseMutex(self.0 .0) };
    }
}

fn system_error(context: &str) -> String {
    format!("{context}: {}", std::io::Error::last_os_error())
}

fn object_name(root: &Path, kind: &str) -> Vec<u16> {
    use sha2::{Digest, Sha256};
    let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let display = crate::windows_system::windows_path_for_display(&canonical);
    let digest = Sha256::digest(display.to_lowercase().as_bytes());
    let suffix: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("Local\\Incodex-UpdateObserver-{suffix}-{kind}")
        .encode_utf16()
        .chain([0])
        .collect()
}

fn event(root: &Path, kind: &str, manual: bool) -> Result<Arc<OwnedHandle>, String> {
    let handle = unsafe {
        CreateEventW(
            ptr::null(),
            manual.into(),
            0,
            object_name(root, kind).as_ptr(),
        )
    };
    if handle.is_null() {
        return Err(system_error("cannot create update observer event"));
    }
    Ok(Arc::new(OwnedHandle(handle)))
}

fn acquire_owner(root: &Path) -> Result<Option<OwnerLock>, String> {
    let handle = unsafe { CreateMutexW(ptr::null(), 1, object_name(root, "owner").as_ptr()) };
    if handle.is_null() {
        return Err(system_error("cannot create update observer owner"));
    }
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let handle = OwnedHandle(handle);
    if existed {
        match unsafe { WaitForSingleObject(handle.0, 0) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => {}
            WAIT_TIMEOUT => return Ok(None),
            _ => return Err(system_error("cannot acquire update observer owner")),
        }
    }
    Ok(Some(OwnerLock(handle)))
}

pub(crate) fn stop(root: &Path) -> Result<(), String> {
    let stop = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, object_name(root, "stop").as_ptr()) };
    if stop.is_null() {
        return if unsafe { GetLastError() } == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(system_error("cannot open update observer stop event"))
        };
    }
    let stop = OwnedHandle(stop);
    if unsafe { SetEvent(stop.0) } == 0 {
        return Err(system_error("cannot stop update observer"));
    }
    let owner = unsafe {
        OpenMutexW(
            SYNCHRONIZATION_SYNCHRONIZE | MUTEX_MODIFY_STATE,
            0,
            object_name(root, "owner").as_ptr(),
        )
    };
    if owner.is_null() {
        return if unsafe { GetLastError() } == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(system_error("cannot observe update observer exit"))
        };
    }
    let owner = OwnedHandle(owner);
    match unsafe { WaitForSingleObject(owner.0, START_TIMEOUT_MS) } {
        WAIT_OBJECT_0 | WAIT_ABANDONED => {
            unsafe { ReleaseMutex(owner.0) };
            Ok(())
        }
        _ => Err("Windows update observer did not stop before the timeout".into()),
    }
}

pub(crate) fn start(state: &WindowsInstallState) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
    let root = crate::windows_activation::installed_debugger_user_root(&state.helper_path)?;
    stop(&root)?;
    let ready = event(&root, "ready", true)?;
    if unsafe { ResetEvent(ready.0) } == 0 {
        return Err(system_error("cannot reset observer readiness"));
    }
    let mut child = Command::new(&state.helper_path)
        .arg(MODE)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| format!("cannot start Windows update observer: {error}"))?;
    let handles = [ready.0, child.as_raw_handle() as HANDLE];
    if unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, START_TIMEOUT_MS) } != WAIT_OBJECT_0
    {
        // 只回收本次创建、尚未确认就绪的观察者，不触碰官方 App 或其它 CLI。
        let _ = child.kill();
        let _ = child.wait();
        return Err("Windows update observer did not establish its subscription; inspect windows/update-observer.json".into());
    }
    Ok(())
}

struct Subscription {
    catalog: PackageCatalog,
    token: i64,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        let _ = self.catalog.RemovePackageUpdating(self.token);
    }
}
fn subscribe(wake: Arc<OwnedHandle>) -> Result<Subscription, String> {
    let catalog = PackageCatalog::OpenForCurrentUser().map_err(|error| error.to_string())?;
    let handler =
        TypedEventHandler::<PackageCatalog, PackageUpdatingEventArgs>::new(move |_, args| {
            if let Some(args) = args.as_ref() {
                if args.IsComplete()?
                    && args.ErrorCode()?.0 == 0
                    && args.TargetPackage()?.Id()?.FamilyName()? == CODEX_PACKAGE_FAMILY_NAME
                {
                    unsafe { SetEvent(wake.0) };
                }
            }
            Ok(())
        });
    let token = catalog
        .PackageUpdating(&handler)
        .map_err(|error| error.to_string())?;
    Ok(Subscription { catalog, token })
}

fn wait(stop: &OwnedHandle, wake: &OwnedHandle) -> Result<bool, String> {
    let handles = [stop.0, wake.0];
    match unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) } {
        WAIT_OBJECT_0 => Ok(false),
        result if result == WAIT_OBJECT_0 + 1 => Ok(true),
        _ => Err(system_error(
            "cannot wait for Windows update observer events",
        )),
    }
}

fn status(root: &Path, phase: &str, detail: &str) -> Result<(), String> {
    let parent = incodex_core::windows_session::ensure_private_windows_dir(&root.join("windows"))?;
    let body = serde_json::to_vec_pretty(&serde_json::json!({
        "pid": std::process::id(), "phase": phase, "detail": detail,
        "unixSeconds": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
    })).map_err(|error| error.to_string())?;
    crate::windows_runtime::replace_private_file(
        &parent,
        &parent.join("update-observer.json"),
        &body,
    )
}

fn reconcile(root: &Path, helper: &Path, stop: &OwnedHandle) -> Result<bool, String> {
    loop {
        // 持久状态快照在安装锁下读取，等待进程退出时不占安装锁。
        let (state, intent, target) = {
            let _gate = acquire_windows_install_state()?;
            if !crate::windows_update_startup::is_registered(helper)? {
                return Ok(false);
            }
            let state = read_windows_install_state(root)?;
            let intent = read_windows_update_repair_intent(root)?;
            if state.is_none() && intent.is_none() {
                return Ok(false);
            }
            if let Some(state) = state.as_ref() {
                if state.helper_path != helper {
                    return Ok(false);
                }
                if !state.desired_enabled() && intent.is_none() {
                    return Ok(false);
                }
            } else if intent
                .as_ref()
                .is_some_and(|intent| intent.helper_path != helper)
            {
                return Ok(false);
            }
            let target = discover_codex_package()?.package_full_name;
            (state, intent, target)
        };
        if state.as_ref().is_some_and(|state| {
            state.desired_enabled()
                && state.package_full_name == target
                && matches!(
                    state.phase,
                    WindowsInstallPhase::EnabledObserved | WindowsInstallPhase::EnabledUnobserved
                )
        }) {
            status(root, "watching", &target)?;
            return Ok(true);
        }
        // 只等待一个已验证目标的进程；退出后整个对账重来，不沿用旧 PID/授权。
        let mut running =
            strict_running_codex_package_process_ids(&target).map_err(|error| error.to_string())?;
        let mut wait_package = target.as_str();
        let source = state
            .as_ref()
            .map(|state| state.package_full_name.as_str())
            .or_else(|| {
                intent
                    .as_ref()
                    .map(|intent| intent.source_package_full_name.as_str())
            });
        if let Some(source) = source.filter(|source| *source != target && running.is_empty()) {
            running = strict_running_codex_package_process_ids(source)
                .map_err(|error| error.to_string())?;
            wait_package = source;
        }
        if let Some(pid) = running.first() {
            status(root, "waiting-for-normal-exit", &target)?;
            let process = unsafe {
                OpenProcess(
                    PROCESS_SYNCHRONIZE
                        | windows_sys::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION,
                    0,
                    *pid,
                )
            };
            if process.is_null() {
                if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER {
                    continue;
                }
                return Err(system_error("cannot observe updated package process"));
            }
            let process = OwnedHandle(process);
            crate::windows_process::require_process_package_identity(process.0, wait_package)
                .map_err(|error| {
                    format!("updated package process identity changed before waiting: {error}")
                })?;
            if !wait(stop, &process)? {
                return Ok(false);
            }
            continue;
        }
        if unsafe { WaitForSingleObject(stop.0, 0) } != WAIT_TIMEOUT {
            return Ok(false);
        }
        let _gate = acquire_windows_install_state()?;
        if !crate::windows_update_startup::is_registered(helper)? {
            return Ok(false);
        }
        if unsafe { WaitForSingleObject(stop.0, 0) } != WAIT_TIMEOUT {
            return Ok(false);
        }
        if read_windows_install_state(root)? != state
            || read_windows_update_repair_intent(root)? != intent
        {
            drop(_gate);
            continue;
        }
        if discover_codex_package()?.package_full_name != target {
            drop(_gate);
            continue;
        }
        status(root, "repairing", &target)?;
        let inspect = |package: &str| {
            let current = discover_codex_package().map_err(std::io::Error::other)?;
            if current.package_full_name != target {
                return Err(std::io::Error::other(
                    "Store generation changed during observer repair",
                ));
            }
            strict_running_codex_package_process_ids(package)
        };
        let repaired = if let Some(state) = state
            .as_ref()
            .filter(|state| state.desired_enabled() && state.package_full_name != target)
        {
            repair_windows_runtime_after_update_with(
                root,
                WindowsUpdateRepairAuthorization {
                    package_full_name: &state.package_full_name,
                    epoch: state.epoch,
                    registration_id: &state.registration_id,
                    helper_source: helper,
                },
                &target,
                inspect,
                crate::windows_app::codex_package_full_name_is_installed,
                crate::windows_activation::disable_installed_runtime,
                crate::windows_activation::enable_installed_runtime,
            )
        } else if let Some(intent) = intent.as_ref().filter(|intent| {
            intent.target_package_full_name == target && intent.helper_path == helper
        }) {
            resume_windows_update_repair_with(
                root,
                intent,
                helper,
                inspect,
                crate::windows_app::codex_package_full_name_is_installed,
                crate::windows_activation::disable_installed_runtime,
                crate::windows_activation::enable_installed_runtime,
            )
        } else {
            return Err(
                "Windows update observer has no authorized recovery for the current package".into(),
            );
        };
        let installed = match repaired {
            Ok(installed) => installed,
            Err(error) => {
                // 用户可能恰在预检后启动官方 App；保留 intent，转为句柄等待。
                let running = strict_running_codex_package_process_ids(&target)
                    .map_err(|probe| format!("{error}; cannot inspect target: {probe}"))?;
                if !running.is_empty() {
                    drop(_gate);
                    continue;
                }
                return Err(error);
            }
        };
        status(root, "watching", &installed.package_full_name)?;
        return Ok(true);
    }
}

pub(crate) fn try_run(args: &[String]) -> Option<Result<(), String>> {
    if args.first().map(String::as_str) != Some(MODE) {
        return None;
    }
    Some((|| {
        if args.len() != 1 {
            return Err("Windows update observer takes no arguments".into());
        }
        let helper =
            std::fs::canonicalize(std::env::current_exe().map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        let root = crate::windows_activation::installed_debugger_user_root(&helper)?;
        let expected = crate::windows_profile::windows_user_profile()?.join(".incodex");
        let expected = std::fs::canonicalize(expected).map_err(|error| error.to_string())?;
        if root != expected {
            return Err("Windows update observer is outside the current user profile".into());
        }
        let mut in_job = 0;
        if unsafe {
            windows_sys::Win32::System::JobObjects::IsProcessInJob(
                windows_sys::Win32::System::Threading::GetCurrentProcess(),
                ptr::null_mut(),
                &mut in_job,
            )
        } == 0
        {
            return Err(system_error(
                "cannot prove update observer process independence",
            ));
        }
        if in_job != 0 {
            return Err("Windows update observer must run outside an inherited process Job".into());
        }
        let Some(_owner) = acquire_owner(&root)? else {
            return Ok(());
        };
        let stop = event(&root, "stop", true)?;
        if unsafe { ResetEvent(stop.0) } == 0 {
            return Err(system_error("cannot reset update observer cancellation"));
        }
        let wake = event(&root, "wake", false)?;
        let ready = event(&root, "ready", true)?;
        let _apartment = WindowsRuntimeApartment::initialize()?;
        let result = run_observer_with(
            || {
                let subscription = subscribe(wake.clone())?;
                status(&root, "subscribed", "startup reconciliation pending")?;
                if unsafe { SetEvent(ready.0) } == 0 {
                    return Err(system_error("cannot signal observer readiness"));
                }
                Ok(subscription)
            },
            || reconcile(&root, &helper, &stop),
            || wait(&stop, &wake),
        );
        let detail = result
            .as_ref()
            .err()
            .map(String::as_str)
            .unwrap_or("cancelled or authorization retired");
        let _ = status(
            &root,
            if result.is_ok() { "stopped" } else { "failed" },
            detail,
        );
        result
    })())
}

fn run_observer_with<S, R, W, T>(subscribe: S, mut reconcile: R, mut wait: W) -> Result<(), String>
where
    S: FnOnce() -> Result<T, String>,
    R: FnMut() -> Result<bool, String>,
    W: FnMut() -> Result<bool, String>,
{
    let _subscription = subscribe()?;
    loop {
        if !reconcile()? || !wait()? {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn fixture_root(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "incodex-observer-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn observer_has_only_one_owner_and_releases_it_on_exit() {
        let root = fixture_root("owner");
        let first = acquire_owner(&root).unwrap().unwrap();
        let second_root = root.clone();
        assert!(
            std::thread::spawn(move || acquire_owner(&second_root).unwrap().is_none())
                .join()
                .unwrap()
        );
        drop(first);
        assert!(acquire_owner(&root).unwrap().is_some());
    }

    #[test]
    fn cancellation_takes_priority_over_an_update_or_process_exit() {
        let root = fixture_root("cancel");
        let stop = event(&root, "stop", true).unwrap();
        let wake = event(&root, "wake", false).unwrap();
        unsafe {
            SetEvent(stop.0);
            SetEvent(wake.0);
        }
        assert!(!wait(&stop, &wake).unwrap());
    }

    #[test]
    fn absent_authorization_stops_before_waiting_for_another_event() {
        run_observer_with(
            || Ok(()),
            || Ok(false),
            || panic!("cancelled observer waited again"),
        )
        .unwrap();
    }

    #[test]
    fn observer_subscribes_before_startup_reconciliation_without_a_new_event() {
        let calls = RefCell::new(Vec::new());
        run_observer_with(
            || {
                calls.borrow_mut().push("subscribe");
                Ok(())
            },
            || {
                calls.borrow_mut().push("reconcile");
                Ok(true)
            },
            || {
                calls.borrow_mut().push("stop");
                Ok(false)
            },
        )
        .unwrap();
        assert_eq!(*calls.borrow(), ["subscribe", "reconcile", "stop"]);
    }

    #[test]
    fn observer_reconciles_every_generation_and_stops_without_authorization() {
        let mut reconciliations = 0;
        let mut wakeups = 0;
        run_observer_with(
            || Ok(()),
            || {
                reconciliations += 1;
                Ok(reconciliations < 3)
            },
            || {
                wakeups += 1;
                Ok(true)
            },
        )
        .unwrap();
        assert_eq!(reconciliations, 3);
        assert_eq!(
            wakeups, 2,
            "startup reconciliation must not require an event"
        );
    }

    #[test]
    fn explicit_stop_wakes_an_idle_observer_and_waits_for_its_owner() {
        let root = fixture_root("stop");
        let worker_root = root.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _owner = acquire_owner(&worker_root).unwrap().unwrap();
            let cancelled = event(&worker_root, "stop", true).unwrap();
            let wake = event(&worker_root, "wake", false).unwrap();
            sender.send(()).unwrap();
            assert!(!wait(&cancelled, &wake).unwrap());
        });
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        stop(&root).unwrap();
        worker.join().unwrap();
        assert!(acquire_owner(&root).unwrap().is_some());
    }

    #[test]
    fn family_subscription_can_be_established_without_an_app_owner() {
        let _apartment = WindowsRuntimeApartment::initialize().unwrap();
        let wake = event(&fixture_root("subscribe"), "wake", false).unwrap();
        let subscription = subscribe(wake).unwrap();
        drop(subscription);
    }
}
