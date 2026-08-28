use std::fs;
use std::io::Read;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::ptr;

use incodex_core::windows_path::{reject_reparse_ancestors, require_local_disk_absolute};
use incodex_core::windows_session::{ensure_private_windows_dir, verify_private_acl};
use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::Security::Cryptography::{
    BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};

use crate::windows_file::{canonical_regular_file, sha256_file};
use crate::windows_helper::installed_windows_helper_path_matches;
use crate::windows_runtime::replace_private_file;

const STATE_NAME: &str = "windows-install.json";
const STATE_SCHEMA: u32 = 1;
const STATE_LIMIT: u64 = 64 * 1024;
const UPDATE_REPAIR_INTENT_NAME: &str = "windows-update-repair.json";
const UPDATE_REPAIR_INTENT_SCHEMA: u32 = 1;
const INSTALL_MUTEX_NAME: &str = "Local\\Incodex-OpenAI.Codex-Install";
const INSTALL_MUTEX_TIMEOUT_MS: u32 = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowsInstallPhase {
    Staged,
    EnablePending,
    EnabledUnobserved,
    EnabledObserved,
    DisableRequested,
    DisablePending,
    Disabled,
    RecoveryRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum WindowsInstallDesired {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsInstallState {
    pub schema_version: u32,
    pub epoch: u64,
    pub registration_id: String,
    desired: WindowsInstallDesired,
    pub phase: WindowsInstallPhase,
    pub package_full_name: String,
    pub helper_path: PathBuf,
    pub helper_sha256: String,
    pub runtime_release: String,
    #[serde(skip)]
    pub state_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsUpdateRepairIntent {
    pub schema_version: u32,
    pub operation_id: String,
    pub source_registration_id: String,
    pub source_epoch: u64,
    pub source_package_full_name: String,
    pub target_package_full_name: String,
    pub helper_path: PathBuf,
    pub helper_sha256: String,
    pub runtime_release: String,
    #[serde(skip)]
    pub intent_path: PathBuf,
}

impl WindowsInstallState {
    pub fn desired_enabled(&self) -> bool {
        self.desired == WindowsInstallDesired::Enabled
    }
}

pub(crate) fn stage_windows_update_repair_intent(
    user_root: &Path,
    source: &WindowsInstallState,
    target_package_full_name: &str,
) -> Result<WindowsUpdateRepairIntent, String> {
    validate_state(source, HelperValidation::Required)?;
    validate_package_name(target_package_full_name)?;
    if source.package_full_name == target_package_full_name {
        return Err("Windows update repair target did not change generation".to_string());
    }
    let user_root = ensure_private_windows_dir(user_root)?;
    let intent = WindowsUpdateRepairIntent {
        schema_version: UPDATE_REPAIR_INTENT_SCHEMA,
        operation_id: random_registration_id()?,
        source_registration_id: source.registration_id.clone(),
        source_epoch: source.epoch,
        source_package_full_name: source.package_full_name.clone(),
        target_package_full_name: target_package_full_name.to_string(),
        helper_path: source.helper_path.clone(),
        helper_sha256: source.helper_sha256.clone(),
        runtime_release: source.runtime_release.clone(),
        intent_path: user_root.join(UPDATE_REPAIR_INTENT_NAME),
    };
    write_update_repair_intent(&user_root, &intent)?;
    Ok(intent)
}

pub fn read_windows_update_repair_intent(
    user_root: &Path,
) -> Result<Option<WindowsUpdateRepairIntent>, String> {
    require_local_disk_absolute(user_root, "Windows Incodex root")?;
    match fs::symlink_metadata(user_root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot inspect Windows Incodex root: {error}")),
        Ok(metadata)
            if !metadata.is_dir()
                || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 =>
        {
            return Err(format!(
                "Windows Incodex root is not a regular directory: {}",
                user_root.display()
            ));
        }
        Ok(_) => {}
    }
    reject_reparse_ancestors(user_root)?;
    verify_private_acl(user_root)?;
    let user_root = fs::canonicalize(user_root)
        .map_err(|error| format!("cannot resolve Windows Incodex root: {error}"))?;
    let intent_path = user_root.join(UPDATE_REPAIR_INTENT_NAME);
    let metadata = match fs::symlink_metadata(&intent_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "cannot inspect Windows update repair intent: {error}"
            ))
        }
    };
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!(
            "Windows update repair intent is not a regular file: {}",
            intent_path.display()
        ));
    }
    if metadata.len() > STATE_LIMIT {
        return Err("Windows update repair intent exceeds the size limit".to_string());
    }
    verify_private_acl(&intent_path)?;
    let file = fs::File::open(&intent_path)
        .map_err(|error| format!("cannot open Windows update repair intent: {error}"))?;
    let mut body = String::new();
    file.take(STATE_LIMIT + 1)
        .read_to_string(&mut body)
        .map_err(|error| format!("cannot read Windows update repair intent: {error}"))?;
    if body.len() as u64 > STATE_LIMIT {
        return Err("Windows update repair intent exceeds the size limit".to_string());
    }
    let mut intent: WindowsUpdateRepairIntent = serde_json::from_str(&body)
        .map_err(|error| format!("invalid Windows update repair intent: {error}"))?;
    intent.intent_path = intent_path;
    validate_update_repair_intent(&intent)?;
    Ok(Some(intent))
}

pub(crate) fn retire_windows_update_repair_intent(
    user_root: &Path,
    expected_operation_id: Option<&str>,
) -> Result<(), String> {
    let Some(intent) = read_windows_update_repair_intent(user_root)? else {
        return Ok(());
    };
    if expected_operation_id.is_some_and(|expected| expected != intent.operation_id) {
        return Err("Windows update repair intent changed".to_string());
    }
    fs::remove_file(&intent.intent_path)
        .map_err(|error| format!("cannot retire Windows update repair intent: {error}"))?;
    match fs::symlink_metadata(&intent.intent_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err("Windows update repair intent still exists after retirement".to_string()),
        Err(error) => Err(format!(
            "cannot verify Windows update repair intent retirement: {error}"
        )),
    }
}

fn write_update_repair_intent(
    user_root: &Path,
    intent: &WindowsUpdateRepairIntent,
) -> Result<(), String> {
    validate_update_repair_intent(intent)?;
    let body = serde_json::to_vec_pretty(intent)
        .map_err(|error| format!("cannot serialize Windows update repair intent: {error}"))?;
    replace_private_file(user_root, &intent.intent_path, &body)
}

fn validate_update_repair_intent(intent: &WindowsUpdateRepairIntent) -> Result<(), String> {
    if intent.schema_version != UPDATE_REPAIR_INTENT_SCHEMA || intent.source_epoch == 0 {
        return Err("Windows update repair intent schema or epoch is invalid".to_string());
    }
    for (label, value) in [
        ("operation id", intent.operation_id.as_str()),
        (
            "source registration id",
            intent.source_registration_id.as_str(),
        ),
    ] {
        if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!("Windows update repair {label} is invalid"));
        }
    }
    validate_package_name(&intent.source_package_full_name)?;
    validate_package_name(&intent.target_package_full_name)?;
    if intent.source_package_full_name == intent.target_package_full_name {
        return Err("Windows update repair target did not change generation".to_string());
    }
    if intent.helper_sha256.len() != 64
        || !intent
            .helper_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Windows update repair helper identity is invalid".to_string());
    }
    require_local_disk_absolute(&intent.helper_path, "Windows update repair helper")?;
    validate_runtime_release(&intent.runtime_release)
}

pub fn stage_windows_install_state(
    user_root: &Path,
    package_full_name: &str,
    helper_path: &Path,
    runtime_release: &str,
) -> Result<WindowsInstallState, String> {
    let _lock = InstallStateLock::acquire()?;
    validate_package_name(package_full_name)?;
    validate_runtime_release(runtime_release)?;
    let helper_path = validate_helper(helper_path)?;
    let user_root = ensure_private_windows_dir(user_root)?;
    let state_path = user_root.join(STATE_NAME);
    match fs::symlink_metadata(&state_path) {
        Ok(_) => {
            return Err(format!(
                "Windows install state already exists: {}",
                state_path.display()
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect Windows install state: {error}")),
    }
    let state = WindowsInstallState {
        schema_version: STATE_SCHEMA,
        epoch: 1,
        registration_id: random_registration_id()?,
        desired: WindowsInstallDesired::Enabled,
        phase: WindowsInstallPhase::Staged,
        package_full_name: package_full_name.to_string(),
        helper_sha256: sha256_file(&helper_path)?,
        helper_path,
        runtime_release: runtime_release.to_string(),
        state_path,
    };
    write_state(&user_root, &state)?;
    Ok(state)
}

pub fn transition_windows_install_state(
    user_root: &Path,
    expected_epoch: u64,
    next_phase: WindowsInstallPhase,
) -> Result<WindowsInstallState, String> {
    transition_windows_install_state_with(
        user_root,
        expected_epoch,
        next_phase,
        HelperValidation::Required,
    )
}

pub fn transition_windows_uninstall_state(
    user_root: &Path,
    expected_epoch: u64,
    next_phase: WindowsInstallPhase,
) -> Result<WindowsInstallState, String> {
    transition_windows_install_state_with(
        user_root,
        expected_epoch,
        next_phase,
        HelperValidation::RecordedIdentity,
    )
}

fn transition_windows_install_state_with(
    user_root: &Path,
    expected_epoch: u64,
    next_phase: WindowsInstallPhase,
    helper_validation: HelperValidation,
) -> Result<WindowsInstallState, String> {
    let _lock = InstallStateLock::acquire()?;
    let user_root = ensure_private_windows_dir(user_root)?;
    let mut state = read_state_from_root(&user_root, helper_validation)?
        .ok_or_else(|| "Windows install state does not exist".to_string())?;
    if state.epoch != expected_epoch {
        return Err(format!(
            "Windows install state changed: expected epoch {expected_epoch}, found {}",
            state.epoch
        ));
    }
    if !allowed_transition(state.phase, next_phase) {
        return Err(format!(
            "invalid Windows install transition: {:?} -> {:?}",
            state.phase, next_phase
        ));
    }
    state.epoch = state
        .epoch
        .checked_add(1)
        .ok_or_else(|| "Windows install state epoch overflowed".to_string())?;
    state.phase = next_phase;
    state.desired = desired_for_phase(next_phase);
    write_state_with(&user_root, &state, helper_validation)?;
    Ok(state)
}

pub fn read_windows_install_state(user_root: &Path) -> Result<Option<WindowsInstallState>, String> {
    read_state_from_root(user_root, HelperValidation::Required)
}

pub fn synchronize_windows_install_runtime_release(
    user_root: &Path,
    runtime_release: &str,
) -> Result<Option<WindowsInstallState>, String> {
    let _lock = InstallStateLock::acquire()?;
    validate_runtime_release(runtime_release)?;
    let user_root = ensure_private_windows_dir(user_root)?;
    let Some(mut state) = read_state_from_root(&user_root, HelperValidation::Required)? else {
        return Ok(None);
    };
    crate::windows_runtime::verify_installed_windows_runtime(&user_root, runtime_release)?;
    if !state.desired_enabled()
        || !matches!(
            state.phase,
            WindowsInstallPhase::EnabledUnobserved | WindowsInstallPhase::EnabledObserved
        )
    {
        return Err("Windows Runtime integration is not in an enabled state".to_string());
    }
    let current_version = runtime_release_version(&state.runtime_release)?;
    let candidate_version = runtime_release_version(runtime_release)?;
    if current_version > candidate_version {
        return Err("a newer installed Windows Runtime is already active".to_string());
    }
    if state.runtime_release == runtime_release {
        return Ok(Some(state));
    }
    state.epoch = state
        .epoch
        .checked_add(1)
        .ok_or_else(|| "Windows install state epoch overflowed".to_string())?;
    state.runtime_release = runtime_release.to_string();
    write_state(&user_root, &state)?;
    Ok(Some(state))
}

pub fn read_windows_install_state_for_uninstall(
    user_root: &Path,
) -> Result<Option<WindowsInstallState>, String> {
    read_state_from_root(user_root, HelperValidation::RecordedIdentity)
}

pub fn retire_disabled_windows_install_state(
    user_root: &Path,
    expected_epoch: u64,
) -> Result<(), String> {
    let _lock = InstallStateLock::acquire()?;
    let state = read_state_from_root(user_root, HelperValidation::RecordedIdentity)?
        .ok_or_else(|| "Windows install state does not exist".to_string())?;
    if state.epoch != expected_epoch || state.phase != WindowsInstallPhase::Disabled {
        return Err("Windows install state is not the expected disabled generation".to_string());
    }
    fs::remove_file(&state.state_path)
        .map_err(|error| format!("cannot retire disabled Windows install state: {error}"))?;
    match fs::symlink_metadata(&state.state_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err("disabled Windows install state still exists after retirement".to_string()),
        Err(error) => Err(format!(
            "cannot verify disabled Windows install state retirement: {error}"
        )),
    }
}

fn read_state_from_root(
    user_root: &Path,
    helper_validation: HelperValidation,
) -> Result<Option<WindowsInstallState>, String> {
    require_local_disk_absolute(user_root, "Windows Incodex root")?;
    match fs::symlink_metadata(user_root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot inspect Windows Incodex root: {error}")),
        Ok(metadata)
            if !metadata.is_dir()
                || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 =>
        {
            return Err(format!(
                "Windows Incodex root is not a regular directory: {}",
                user_root.display()
            ));
        }
        Ok(_) => {}
    }
    reject_reparse_ancestors(user_root)?;
    verify_private_acl(user_root)?;
    let user_root = fs::canonicalize(user_root)
        .map_err(|error| format!("cannot resolve Windows Incodex root: {error}"))?;
    let state_path = user_root.join(STATE_NAME);
    let metadata = match fs::symlink_metadata(&state_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot inspect Windows install state: {error}")),
    };
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!(
            "Windows install state is not a regular file: {}",
            state_path.display()
        ));
    }
    if metadata.len() > STATE_LIMIT {
        return Err("Windows install state exceeds the size limit".to_string());
    }
    verify_private_acl(&state_path)?;
    let file = fs::File::open(&state_path)
        .map_err(|error| format!("cannot open Windows install state: {error}"))?;
    let mut body = String::new();
    file.take(STATE_LIMIT + 1)
        .read_to_string(&mut body)
        .map_err(|error| format!("cannot read Windows install state: {error}"))?;
    if body.len() as u64 > STATE_LIMIT {
        return Err("Windows install state exceeds the size limit".to_string());
    }
    let mut state: WindowsInstallState = serde_json::from_str(&body)
        .map_err(|error| format!("invalid Windows install state: {error}"))?;
    state.state_path = state_path;
    validate_state(&state, helper_validation)?;
    Ok(Some(state))
}

fn write_state(user_root: &Path, state: &WindowsInstallState) -> Result<(), String> {
    write_state_with(user_root, state, HelperValidation::Required)
}

fn write_state_with(
    user_root: &Path,
    state: &WindowsInstallState,
    helper_validation: HelperValidation,
) -> Result<(), String> {
    validate_state(state, helper_validation)?;
    let body = serde_json::to_vec_pretty(state)
        .map_err(|error| format!("cannot serialize Windows install state: {error}"))?;
    replace_private_file(user_root, &state.state_path, &body)
}

#[derive(Clone, Copy)]
enum HelperValidation {
    Required,
    RecordedIdentity,
}

fn validate_state(
    state: &WindowsInstallState,
    helper_validation: HelperValidation,
) -> Result<(), String> {
    if state.schema_version != STATE_SCHEMA || state.epoch == 0 {
        return Err("Windows install state schema or epoch is invalid".to_string());
    }
    if state.registration_id.len() != 32
        || !state
            .registration_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Windows install registration id is invalid".to_string());
    }
    validate_package_name(&state.package_full_name)?;
    validate_runtime_release(&state.runtime_release)?;
    if state.helper_sha256.len() != 64
        || !state
            .helper_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Windows install helper identity is invalid".to_string());
    }
    let user_root = state
        .state_path
        .parent()
        .ok_or_else(|| "Windows install state has no Incodex root".to_string())?;
    if matches!(helper_validation, HelperValidation::Required)
        || !installed_windows_helper_path_matches(
            user_root,
            &state.helper_sha256,
            &state.helper_path,
        )
    {
        let helper = validate_helper(&state.helper_path)?;
        if sha256_file(&helper)? != state.helper_sha256 {
            return Err("Windows install helper identity is invalid".to_string());
        }
    }
    if state.desired != desired_for_phase(state.phase) {
        return Err("Windows install desired state does not match its phase".to_string());
    }
    Ok(())
}

fn validate_package_name(package_full_name: &str) -> Result<(), String> {
    if package_full_name.trim().is_empty() || package_full_name.contains(['\0', '\r', '\n']) {
        return Err("Windows install package name is invalid".to_string());
    }
    Ok(())
}

fn validate_runtime_release(runtime_release: &str) -> Result<(), String> {
    if runtime_release.is_empty()
        || runtime_release == "."
        || runtime_release == ".."
        || runtime_release.contains(['/', '\\', '\0', '\r', '\n'])
        || !runtime_release
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return Err("Windows Runtime release name is invalid".to_string());
    }
    Ok(())
}

fn runtime_release_version(runtime_release: &str) -> Result<[u64; 3], String> {
    let version = runtime_release
        .split_once('-')
        .map_or(runtime_release, |(version, _)| version);
    crate::stable_release::parse_stable_version(version)
        .ok_or_else(|| "Windows Runtime release version is invalid".to_string())
}

fn validate_helper(helper_path: &Path) -> Result<PathBuf, String> {
    canonical_regular_file(helper_path, "Windows install helper")
}

pub(crate) fn random_registration_id() -> Result<String, String> {
    let mut random = [0u8; 16];
    let status = unsafe {
        BCryptGenRandom(
            ptr::null_mut(),
            random.as_mut_ptr(),
            random.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err(format!(
            "cannot generate Windows install registration id: NTSTATUS 0x{:08X}",
            status as u32
        ));
    }
    Ok(random.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(crate) fn retire_unreadable_windows_install_state(user_root: &Path) -> Result<(), String> {
    require_local_disk_absolute(user_root, "Windows Incodex root")?;
    reject_reparse_ancestors(user_root)?;
    verify_private_acl(user_root)?;
    let user_root = fs::canonicalize(user_root)
        .map_err(|error| format!("cannot resolve Windows Incodex root: {error}"))?;
    let state_path = user_root.join(STATE_NAME);
    let metadata = match fs::symlink_metadata(&state_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("cannot inspect Windows install state: {error}")),
    };
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!(
            "Windows install state is not a regular file: {}",
            state_path.display()
        ));
    }
    verify_private_acl(&state_path)?;
    fs::remove_file(&state_path)
        .map_err(|error| format!("cannot retire unreadable Windows install state: {error}"))?;
    match fs::symlink_metadata(&state_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err("unreadable Windows install state still exists after retirement".to_string()),
        Err(error) => Err(format!(
            "cannot verify unreadable Windows install state retirement: {error}"
        )),
    }
}

fn allowed_transition(current: WindowsInstallPhase, next: WindowsInstallPhase) -> bool {
    use WindowsInstallPhase::{
        DisablePending, DisableRequested, Disabled, EnablePending, EnabledObserved,
        EnabledUnobserved, RecoveryRequired, Staged,
    };
    matches!(
        (current, next),
        (Staged, EnablePending | Disabled)
            | (EnablePending, EnabledUnobserved | RecoveryRequired)
            | (
                EnabledUnobserved,
                EnabledObserved | DisableRequested | RecoveryRequired
            )
            | (EnabledObserved, DisableRequested | RecoveryRequired)
            | (DisableRequested, DisablePending | RecoveryRequired)
            | (DisablePending, Disabled | RecoveryRequired)
            | (RecoveryRequired, DisablePending)
    )
}

fn desired_for_phase(phase: WindowsInstallPhase) -> WindowsInstallDesired {
    match phase {
        WindowsInstallPhase::Staged
        | WindowsInstallPhase::EnablePending
        | WindowsInstallPhase::EnabledUnobserved
        | WindowsInstallPhase::EnabledObserved => WindowsInstallDesired::Enabled,
        WindowsInstallPhase::DisableRequested
        | WindowsInstallPhase::DisablePending
        | WindowsInstallPhase::Disabled
        | WindowsInstallPhase::RecoveryRequired => WindowsInstallDesired::Disabled,
    }
}

struct InstallStateLock(windows_sys::Win32::Foundation::HANDLE);

pub struct WindowsInstallStateGuard {
    _lock: InstallStateLock,
}

pub fn acquire_windows_install_state() -> Result<WindowsInstallStateGuard, String> {
    InstallStateLock::acquire().map(|lock| WindowsInstallStateGuard { _lock: lock })
}

impl InstallStateLock {
    fn acquire() -> Result<Self, String> {
        let name = INSTALL_MUTEX_NAME
            .encode_utf16()
            .chain([0])
            .collect::<Vec<_>>();
        let handle = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(format!(
                "cannot create Windows install state lock: {}",
                std::io::Error::last_os_error()
            ));
        }
        match unsafe { WaitForSingleObject(handle, INSTALL_MUTEX_TIMEOUT_MS) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Self(handle)),
            WAIT_TIMEOUT => {
                unsafe { CloseHandle(handle) };
                Err("timed out waiting for Windows install state lock".to_string())
            }
            _ => {
                let error = std::io::Error::last_os_error();
                unsafe { CloseHandle(handle) };
                Err(format!(
                    "cannot acquire Windows install state lock: {error}"
                ))
            }
        }
    }
}

impl Drop for InstallStateLock {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}
