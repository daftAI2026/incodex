use std::path::Path;
/**
 * [INPUT]: 依赖 windows_app 的可信 Store 包发现、windows_install 的既有事务、
 *          windows_install_state 的 helper/epoch 授权，以及 Windows PackageCatalog 事件。
 * [OUTPUT]: 对外提供包更新事件分类、主进程识别，以及 debugger helper 内的同进程协调器。
 * [POS]: Windows install 的更新后恢复适配层；在包外 debugger 生命周期内协调新旧 generation，
 *        不复制安装器、不修改 Store 包、不改变共享 Runtime 或 macOS 生命周期。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::ptr;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use windows::ApplicationModel::{PackageCatalog, PackageUpdatingEventArgs};
use windows::Foundation::TypedEventHandler;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, ERROR_INVALID_PARAMETER, FILETIME,
    WAIT_OBJECT_0,
};
use windows_sys::Win32::System::JobObjects::IsProcessInJob;
use windows_sys::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcess, GetProcessTimes, OpenProcess, WaitForSingleObject, INFINITE,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};

use crate::windows_activation::{
    disable_installed_runtime, enable_installed_runtime, installed_debugger_user_root,
    WindowsInstalledRuntimeRegistration,
};
use crate::windows_app::{
    codex_package_full_name_is_installed, discover_codex_package, validate_codex_package_full_name,
    CODEX_PACKAGE_FAMILY_NAME,
};
use crate::windows_install::{
    install_windows_runtime_locked_with, uninstall_windows_runtime_locked_with,
    WindowsUninstallOutcome,
};
use crate::windows_install_state::{
    acquire_windows_install_state, read_windows_install_state, read_windows_update_repair_intent,
    retire_windows_update_repair_intent, stage_windows_update_repair_intent, WindowsInstallState,
    WindowsUpdateRepairIntent,
};
use crate::windows_process::strict_running_codex_package_process_ids;
use crate::windows_system::windows_path_for_display;

const UPDATE_START_GRACE: Duration = Duration::from_secs(10);
const UPDATE_COMPLETION_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[link(name = "runtimeobject")]
extern "system" {
    fn RoInitialize(init_type: u32) -> i32;
    fn RoUninitialize();
}

const RO_INIT_MULTITHREADED: u32 = 1;
const S_OK: i32 = 0;
const S_FALSE: i32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageUpdateObservation {
    pub source_package_full_name: String,
    pub target_package_full_name: String,
    pub target_package_family_name: String,
    pub complete: bool,
    pub error_code: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageUpdateOutcome {
    Ignore,
    Updating,
    Failed,
    Ready { target_package_full_name: String },
}

pub fn classify_package_update(
    expected_family: &str,
    installed_package: &str,
    observation: &PackageUpdateObservation,
) -> PackageUpdateOutcome {
    if observation.target_package_family_name != expected_family {
        return PackageUpdateOutcome::Ignore;
    }
    if !observation.complete {
        return PackageUpdateOutcome::Updating;
    }
    if observation.error_code < 0 {
        return PackageUpdateOutcome::Failed;
    }
    if observation.target_package_full_name == installed_package
        || observation.source_package_full_name != installed_package
    {
        return PackageUpdateOutcome::Ignore;
    }
    PackageUpdateOutcome::Ready {
        target_package_full_name: observation.target_package_full_name.clone(),
    }
}

pub fn is_primary_package_process(command_line: &str) -> bool {
    !command_line
        .split_ascii_whitespace()
        .any(|argument| argument == "--type" || argument.starts_with("--type="))
}

pub fn await_package_quiescence_with<R, W>(
    target_package_full_name: &str,
    mut running_package_processes: R,
    mut wait_for_processes: W,
) -> Result<(), String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    W: FnMut(&[u32]) -> Result<(), String>,
{
    loop {
        let running = running_package_processes(target_package_full_name)
            .map_err(|error| format!("cannot inspect running Windows Codex processes: {error}"))?;
        if running.is_empty() {
            return Ok(());
        }
        wait_for_processes(&running)?;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WindowsUpdateRepairAuthorization<'a> {
    pub package_full_name: &'a str,
    pub epoch: u64,
    pub registration_id: &'a str,
    pub helper_source: &'a Path,
}

pub(crate) fn prepare_interrupted_update_repair_with<R, P, D>(
    user_root: &Path,
    target_package_full_name: &str,
    running_package_processes: &mut R,
    package_is_installed: &mut P,
    disable: &mut D,
) -> Result<(), String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnMut(&str) -> Result<bool, String>,
    D: FnMut(&str) -> Result<(), String>,
{
    if read_windows_update_repair_intent(user_root)?.is_none() {
        return Ok(());
    }
    require_target_quiescent(target_package_full_name, running_package_processes)?;
    match uninstall_windows_runtime_locked_with(
        user_root,
        running_package_processes,
        package_is_installed,
        disable,
    )? {
        WindowsUninstallOutcome::Removed | WindowsUninstallOutcome::NotInstalled => Ok(()),
        WindowsUninstallOutcome::CloseRequired { process_ids } => Err(format!(
            "close Codex before resuming the interrupted Windows update repair (running package PIDs: {})",
            process_ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub fn resume_windows_update_repair_with<R, P, D, E>(
    user_root: &Path,
    expected_intent: &WindowsUpdateRepairIntent,
    helper_source: &Path,
    mut running_package_processes: R,
    mut package_is_installed: P,
    mut disable: D,
    enable: E,
) -> Result<WindowsInstallState, String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnMut(&str) -> Result<bool, String>,
    D: FnMut(&str) -> Result<(), String>,
    E: FnOnce(&WindowsInstalledRuntimeRegistration) -> Result<(), String>,
{
    let _transaction = acquire_windows_install_state()?;
    let current_intent = read_windows_update_repair_intent(user_root)?
        .ok_or_else(|| "Windows update repair intent changed or was cancelled".to_string())?;
    if current_intent != *expected_intent || current_intent.helper_path != helper_source {
        return Err("Windows update repair intent changed or was cancelled".to_string());
    }
    require_target_quiescent(
        &current_intent.target_package_full_name,
        &mut running_package_processes,
    )?;
    match uninstall_windows_runtime_locked_with(
        user_root,
        &mut running_package_processes,
        &mut package_is_installed,
        &mut disable,
    )? {
        WindowsUninstallOutcome::Removed | WindowsUninstallOutcome::NotInstalled => {}
        WindowsUninstallOutcome::CloseRequired { process_ids } => {
            return Err(format!(
                "close Codex before resuming the interrupted Windows update repair (running package PIDs: {})",
                process_ids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    let retained_intent = read_windows_update_repair_intent(user_root)?
        .ok_or_else(|| "Windows update repair intent changed or was cancelled".to_string())?;
    if retained_intent != current_intent {
        return Err("Windows update repair intent changed or was cancelled".to_string());
    }
    let installed = install_windows_runtime_locked_with(
        user_root,
        &current_intent.target_package_full_name,
        helper_source,
        running_package_processes,
        package_is_installed,
        disable,
        enable,
    )?;
    retire_windows_update_repair_intent(user_root, Some(&current_intent.operation_id))?;
    Ok(installed)
}

fn require_target_quiescent<R>(
    target_package_full_name: &str,
    running_package_processes: &mut R,
) -> Result<(), String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
{
    let running = running_package_processes(target_package_full_name)
        .map_err(|error| format!("cannot inspect running Windows Codex processes: {error}"))?;
    if running.is_empty() {
        return Ok(());
    }
    Err(format!(
        "close Codex before resuming the interrupted Windows update repair (running package PIDs: {})",
        running
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

pub fn repair_windows_runtime_after_update_with<R, P, D, E>(
    user_root: &Path,
    authorization: WindowsUpdateRepairAuthorization<'_>,
    target_package_full_name: &str,
    running_package_processes: R,
    package_is_installed: P,
    disable: D,
    enable: E,
) -> Result<WindowsInstallState, String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnMut(&str) -> Result<bool, String>,
    D: FnMut(&str) -> Result<(), String>,
    E: FnOnce(&WindowsInstalledRuntimeRegistration) -> Result<(), String>,
{
    let _transaction = acquire_windows_install_state()?;
    let state = read_windows_install_state(user_root)?
        .ok_or_else(|| "Windows update repair install state does not exist".to_string())?;
    if !state.desired_enabled()
        || state.epoch != authorization.epoch
        || state.registration_id != authorization.registration_id
        || state.package_full_name != authorization.package_full_name
        || state.helper_path != authorization.helper_source
    {
        return Err("Windows update repair authorization changed".to_string());
    }
    if target_package_full_name == authorization.package_full_name {
        return Err("Windows update repair target did not change generation".to_string());
    }
    let intent = stage_windows_update_repair_intent(user_root, &state, target_package_full_name)?;
    let installed = install_windows_runtime_locked_with(
        user_root,
        target_package_full_name,
        authorization.helper_source,
        running_package_processes,
        package_is_installed,
        disable,
        enable,
    );
    match installed {
        Ok(installed) => {
            retire_windows_update_repair_intent(user_root, Some(&intent.operation_id))?;
            Ok(installed)
        }
        Err(error) => Err(format!(
            "{error}; Windows update repair intent retained at {}",
            windows_path_for_display(&intent.intent_path)
        )),
    }
}

pub(crate) fn run_update_repair_coordinator(
    state: &WindowsInstallState,
    owner_process_id: u32,
) -> Result<(), String> {
    if owner_process_id == 0 || !state.desired_enabled() {
        return Err("Windows update repair owner is invalid".to_string());
    }
    let mut in_job = 0;
    if unsafe { IsProcessInJob(GetCurrentProcess(), ptr::null_mut(), &mut in_job) } == 0 {
        return Err(format!(
            "cannot prove the Windows installed debugger is outside the Codex Job: {}",
            std::io::Error::last_os_error()
        ));
    }
    if in_job != 0 {
        return Err(
            "Windows installed debugger is inside the Codex Job; update repair is disabled"
                .to_string(),
        );
    }
    let owner_creation_time = process_creation_time(owner_process_id)?;
    let Some(_coordinator_lock) = UpdateRepairLock::acquire(
        &state.registration_id,
        owner_process_id,
        owner_creation_time,
    )?
    else {
        return Ok(());
    };

    let helper = std::env::current_exe()
        .map_err(|error| format!("cannot locate the Windows installed debugger: {error}"))?;
    let user_root = installed_debugger_user_root(&helper)?;
    let helper = std::fs::canonicalize(&helper)
        .map_err(|error| format!("cannot resolve the Windows installed debugger: {error}"))?;
    let current = read_windows_install_state(&user_root)?
        .ok_or_else(|| "Windows update repair install state does not exist".to_string())?;
    if !current.desired_enabled()
        || current.epoch != state.epoch
        || current.registration_id != state.registration_id
        || current.package_full_name != state.package_full_name
        || current.helper_path != helper
    {
        return Err("Windows update repair authorization changed".to_string());
    }
    validate_codex_package_full_name(&current.package_full_name)?;

    let _runtime = WindowsRuntimeApartment::initialize()?;
    let (sender, receiver) = mpsc::channel();
    let subscription = PackageUpdateSubscription::subscribe(
        sender.clone(),
        CODEX_PACKAGE_FAMILY_NAME,
        &current.package_full_name,
    )?;
    wait_for_owner(owner_process_id, sender);
    let target = wait_for_update_target(
        &receiver,
        &state.package_full_name,
        &state.package_full_name,
    )?;
    drop(subscription);
    let Some(target_package) = target else {
        return Ok(());
    };

    let discovered = discover_codex_package()?;
    if discovered.package_full_name != target_package {
        return Err(
            "Windows update repair target is not the current trusted Store generation".to_string(),
        );
    }
    await_package_quiescence_with(
        &target_package,
        strict_running_codex_package_process_ids,
        wait_for_process_ids,
    )?;
    let discovered = discover_codex_package()?;
    if discovered.package_full_name != target_package {
        return Err(
            "Windows update repair target changed while awaiting package quiescence".to_string(),
        );
    }
    let repair = repair_windows_runtime_after_update_with(
        &user_root,
        WindowsUpdateRepairAuthorization {
            package_full_name: &current.package_full_name,
            epoch: current.epoch,
            registration_id: &current.registration_id,
            helper_source: &helper,
        },
        &discovered.package_full_name,
        strict_running_codex_package_process_ids,
        codex_package_full_name_is_installed,
        disable_installed_runtime,
        enable_installed_runtime,
    );
    let Err(first_error) = repair else {
        return Ok(());
    };
    let retry_intent = read_windows_update_repair_intent(&user_root)?.filter(|intent| {
        intent.source_registration_id == current.registration_id
            && intent.source_package_full_name == current.package_full_name
            && intent.target_package_full_name == target_package
    });
    let Some(retry_intent) = retry_intent else {
        return Err(first_error);
    };
    resume_windows_update_repair_with(
        &user_root,
        &retry_intent,
        &helper,
        strict_running_codex_package_process_ids,
        codex_package_full_name_is_installed,
        disable_installed_runtime,
        enable_installed_runtime,
    )
    .map(|_| ())
    .map_err(|retry_error| {
        format!("{first_error}; automatic Windows update repair retry also failed: {retry_error}")
    })
}

fn wait_for_process_ids(process_ids: &[u32]) -> Result<(), String> {
    for process_id in process_ids {
        let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, *process_id) };
        if process.is_null() {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                continue;
            }
            return Err(format!(
                "cannot observe updated Windows Codex process {process_id}: {error}"
            ));
        }
        let result = unsafe { WaitForSingleObject(process, INFINITE) };
        unsafe { CloseHandle(process) };
        if result != WAIT_OBJECT_0 {
            return Err(format!(
                "cannot wait for updated Windows Codex process {process_id}: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(())
}

fn wait_for_update_target(
    receiver: &Receiver<CoordinatorEvent>,
    expected_package: &str,
    installed_package: &str,
) -> Result<Option<String>, String> {
    let mut owner_exited = false;
    let mut checked_generation_after_exit = false;
    let mut owner_exit_deadline = None;
    let mut updating = false;
    let mut target = None;
    let mut completion_deadline = None;
    loop {
        if owner_exited {
            if let Some(target) = target {
                return Ok(Some(target));
            }
            if !updating && !checked_generation_after_exit {
                checked_generation_after_exit = true;
                match discover_codex_package() {
                    Ok(app) if app.package_full_name != expected_package => {
                        return Ok(Some(app.package_full_name));
                    }
                    Ok(_) => {}
                    Err(_) => {}
                }
            }
        }

        let timeout = if updating {
            let deadline = completion_deadline
                .get_or_insert_with(|| Instant::now() + UPDATE_COMPLETION_TIMEOUT);
            deadline.saturating_duration_since(Instant::now())
        } else if owner_exited {
            let deadline =
                owner_exit_deadline.get_or_insert_with(|| Instant::now() + UPDATE_START_GRACE);
            deadline.saturating_duration_since(Instant::now())
        } else {
            Duration::MAX
        };
        let event = if timeout == Duration::MAX {
            receiver
                .recv()
                .map_err(|_| "Windows update repair event channel closed unexpectedly".to_string())
        } else {
            match receiver.recv_timeout(timeout) {
                Ok(event) => Ok(event),
                Err(RecvTimeoutError::Timeout) if owner_exited && !updating => return Ok(None),
                Err(RecvTimeoutError::Timeout) => {
                    return Err(
                        "Windows Store update did not complete before the repair timeout"
                            .to_string(),
                    )
                }
                Err(RecvTimeoutError::Disconnected) => {
                    Err("Windows update repair event channel closed unexpectedly".to_string())
                }
            }
        }?;
        match event {
            CoordinatorEvent::OwnerExited => {
                owner_exited = true;
                owner_exit_deadline = Some(Instant::now() + UPDATE_START_GRACE);
            }
            CoordinatorEvent::OwnerUnavailable(error) => return Err(error),
            CoordinatorEvent::Package(observation) => match classify_package_update(
                CODEX_PACKAGE_FAMILY_NAME,
                installed_package,
                &observation,
            ) {
                PackageUpdateOutcome::Ignore => {}
                PackageUpdateOutcome::Updating => updating = true,
                PackageUpdateOutcome::Failed => {
                    return Err("Windows Store reported that the Codex update failed".to_string())
                }
                PackageUpdateOutcome::Ready {
                    target_package_full_name,
                } => target = Some(target_package_full_name),
            },
        }
    }
}

enum CoordinatorEvent {
    OwnerExited,
    OwnerUnavailable(String),
    Package(PackageUpdateObservation),
}

fn wait_for_owner(process_id: u32, sender: Sender<CoordinatorEvent>) {
    thread::spawn(move || {
        let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, process_id) };
        if process.is_null() {
            let _ = sender.send(CoordinatorEvent::OwnerUnavailable(
                "cannot observe the Windows Codex owner process".to_string(),
            ));
            return;
        }
        let result = unsafe { WaitForSingleObject(process, INFINITE) };
        unsafe { CloseHandle(process) };
        let event = if result == WAIT_OBJECT_0 {
            CoordinatorEvent::OwnerExited
        } else {
            CoordinatorEvent::OwnerUnavailable(
                "cannot wait for the Windows Codex owner process".to_string(),
            )
        };
        let _ = sender.send(event);
    });
}

struct PackageUpdateSubscription {
    catalog: PackageCatalog,
    token: i64,
}

impl PackageUpdateSubscription {
    fn subscribe(
        sender: Sender<CoordinatorEvent>,
        expected_family: &str,
        expected_source_package: &str,
    ) -> Result<Self, String> {
        let catalog = PackageCatalog::OpenForCurrentUser()
            .map_err(|error| format!("cannot open the current-user package catalog: {error}"))?;
        let expected_family = expected_family.to_string();
        let expected_source_package = expected_source_package.to_string();
        let handler = TypedEventHandler::<PackageCatalog, PackageUpdatingEventArgs>::new(
            move |_catalog, arguments| {
                if let Some(arguments) = arguments.as_ref() {
                    if let Ok(observation) = package_update_observation(arguments) {
                        if observation.target_package_family_name == expected_family
                            && observation.source_package_full_name == expected_source_package
                        {
                            let _ = sender.send(CoordinatorEvent::Package(observation));
                        }
                    }
                }
                Ok(())
            },
        );
        let token = catalog.PackageUpdating(&handler).map_err(|error| {
            format!("cannot subscribe to Windows package update events: {error}")
        })?;
        Ok(Self { catalog, token })
    }
}

struct UpdateRepairLock(windows_sys::Win32::Foundation::HANDLE);

impl UpdateRepairLock {
    fn acquire(
        registration_id: &str,
        owner_process_id: u32,
        owner_creation_time: u64,
    ) -> Result<Option<Self>, String> {
        let name = format!(
            "Local\\Incodex-OpenAI.Codex-UpdateRepair-{registration_id}-{owner_process_id}-{owner_creation_time}"
        )
        .encode_utf16()
        .chain([0])
        .collect::<Vec<_>>();
        let handle = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(format!(
                "cannot create the Windows update repair coordinator lock: {}",
                std::io::Error::last_os_error()
            ));
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            return Ok(None);
        }
        Ok(Some(Self(handle)))
    }
}

fn process_creation_time(process_id: u32) -> Result<u64, String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return Err(format!(
            "cannot open the Windows Codex owner process: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let result =
        unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) };
    unsafe { CloseHandle(process) };
    if result == 0 {
        return Err(format!(
            "cannot read the Windows Codex owner identity: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64)
}

impl Drop for UpdateRepairLock {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

impl Drop for PackageUpdateSubscription {
    fn drop(&mut self) {
        let _ = self.catalog.RemovePackageUpdating(self.token);
    }
}

fn package_update_observation(
    arguments: &PackageUpdatingEventArgs,
) -> windows::core::Result<PackageUpdateObservation> {
    let source = arguments.SourcePackage()?;
    let target = arguments.TargetPackage()?;
    Ok(PackageUpdateObservation {
        source_package_full_name: source.Id()?.FullName()?.to_string(),
        target_package_full_name: target.Id()?.FullName()?.to_string(),
        target_package_family_name: target.Id()?.FamilyName()?.to_string(),
        complete: arguments.IsComplete()?,
        error_code: arguments.ErrorCode()?.0,
    })
}

struct WindowsRuntimeApartment {
    uninitialize: bool,
}

impl WindowsRuntimeApartment {
    fn initialize() -> Result<Self, String> {
        let result = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        if result < 0 {
            return Err(format!(
                "cannot initialize the Windows Runtime apartment: HRESULT 0x{:08X}",
                result as u32
            ));
        }
        Ok(Self {
            uninitialize: matches!(result, S_OK | S_FALSE),
        })
    }
}

impl Drop for WindowsRuntimeApartment {
    fn drop(&mut self) {
        if self.uninitialize {
            unsafe { RoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::{is_primary_package_process, PackageUpdateSubscription, WindowsRuntimeApartment};

    #[test]
    fn primary_package_process_detection_excludes_electron_children() {
        assert!(is_primary_package_process(
            r#"D:\WindowsApps\OpenAI.Codex\app\ChatGPT.exe"#
        ));
        assert!(!is_primary_package_process(
            r#"D:\WindowsApps\OpenAI.Codex\app\ChatGPT.exe --type=renderer"#
        ));
        assert!(!is_primary_package_process(
            r#"D:\WindowsApps\OpenAI.Codex\app\ChatGPT.exe --type utility"#
        ));
    }

    #[test]
    fn current_user_package_catalog_supports_event_subscription() {
        let _runtime = WindowsRuntimeApartment::initialize().expect("initialize WinRT");
        let (sender, _receiver) = mpsc::channel();
        let subscription = PackageUpdateSubscription::subscribe(
            sender,
            super::CODEX_PACKAGE_FAMILY_NAME,
            "OpenAI.Codex_1.0.0.0_x64__2p2nqsd0c76g0",
        )
        .expect("subscribe to current-user package updates");
        drop(subscription);
    }
}
