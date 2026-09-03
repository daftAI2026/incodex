use std::fs;
use std::io::Read;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use incodex_core::windows_path::{reject_reparse_ancestors, require_local_disk_absolute};
use incodex_core::windows_session::{ensure_private_windows_dir, verify_private_acl};
use serde::{Deserialize, Serialize};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use crate::windows_file::{canonical_regular_file, sha256_file};
use crate::windows_helper::installed_windows_helper_path_matches;
use crate::windows_install_state::{
    random_registration_id, read_windows_install_state, WindowsInstallState,
};
use crate::windows_runtime::replace_private_file;

const REGISTRATION_NAME: &str = "windows-registration.json";
const TRANSIENT_REGISTRATION_NAME: &str = "windows-transient-registration.json";
const REGISTRATION_SCHEMA: u32 = 1;
const REGISTRATION_LIMIT: u64 = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowsDebugRegistrationKind {
    Transient,
    Installed,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsDebugRegistrationEvidence {
    pub schema_version: u32,
    pub registration_id: String,
    pub kind: WindowsDebugRegistrationKind,
    pub package_full_name: String,
    pub helper_path: PathBuf,
    pub helper_sha256: String,
    #[serde(skip)]
    pub state_path: PathBuf,
}

pub fn stage_transient_windows_debug_registration(
    user_root: &Path,
    package_full_name: &str,
    helper_path: &Path,
) -> Result<WindowsDebugRegistrationEvidence, String> {
    stage_windows_debug_registration(
        user_root,
        WindowsDebugRegistrationKind::Transient,
        random_registration_id()?,
        package_full_name,
        helper_path,
    )
}

pub(crate) fn stage_installed_windows_debug_registration(
    user_root: &Path,
    state: &WindowsInstallState,
) -> Result<WindowsDebugRegistrationEvidence, String> {
    stage_windows_debug_registration(
        user_root,
        WindowsDebugRegistrationKind::Installed,
        state.registration_id.clone(),
        &state.package_full_name,
        &state.helper_path,
    )
}

fn stage_windows_debug_registration(
    user_root: &Path,
    kind: WindowsDebugRegistrationKind,
    registration_id: String,
    package_full_name: &str,
    helper_path: &Path,
) -> Result<WindowsDebugRegistrationEvidence, String> {
    validate_package_name(package_full_name)?;
    validate_registration_id(&registration_id)?;
    let helper_path = canonical_regular_file(helper_path, "Windows registration helper")?;
    verify_private_acl(&helper_path)?;
    let helper_sha256 = sha256_file(&helper_path)?;
    let user_root = ensure_private_windows_dir(user_root)?;
    let state_path = user_root.join(match kind {
        WindowsDebugRegistrationKind::Transient => TRANSIENT_REGISTRATION_NAME,
        WindowsDebugRegistrationKind::Installed => REGISTRATION_NAME,
    });
    match fs::symlink_metadata(&state_path) {
        Ok(_) => {
            return Err(format!(
                "Windows debugger registration evidence already exists: {}",
                state_path.display()
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "cannot inspect Windows debugger registration evidence: {error}"
            ))
        }
    }
    let evidence = WindowsDebugRegistrationEvidence {
        schema_version: REGISTRATION_SCHEMA,
        registration_id,
        kind,
        package_full_name: package_full_name.to_string(),
        helper_path,
        helper_sha256,
        state_path,
    };
    validate_evidence(&evidence)?;
    let body = serde_json::to_vec_pretty(&evidence)
        .map_err(|error| format!("cannot serialize Windows debugger registration: {error}"))?;
    replace_private_file(&user_root, &evidence.state_path, &body)?;
    Ok(evidence)
}

pub fn read_windows_debug_registration(
    user_root: &Path,
) -> Result<Option<WindowsDebugRegistrationEvidence>, String> {
    read_windows_debug_registration_file(user_root, REGISTRATION_NAME)
}

fn read_transient_windows_debug_registration(
    user_root: &Path,
) -> Result<Option<WindowsDebugRegistrationEvidence>, String> {
    read_windows_debug_registration_file(user_root, TRANSIENT_REGISTRATION_NAME)
}

pub(crate) fn transient_windows_debug_registration_exists(
    user_root: &Path,
) -> Result<bool, String> {
    if read_transient_windows_debug_registration(user_root)?.is_some() {
        return Ok(true);
    }
    Ok(read_windows_debug_registration(user_root)?
        .is_some_and(|evidence| evidence.kind == WindowsDebugRegistrationKind::Transient))
}

fn read_windows_debug_registration_file(
    user_root: &Path,
    name: &str,
) -> Result<Option<WindowsDebugRegistrationEvidence>, String> {
    let Some((user_root, state_path, metadata)) = registration_file(user_root, name)? else {
        return Ok(None);
    };
    if metadata.len() > REGISTRATION_LIMIT {
        return Err("Windows debugger registration evidence exceeds the size limit".to_string());
    }
    verify_private_acl(&state_path)?;
    let file = fs::File::open(&state_path)
        .map_err(|error| format!("cannot open Windows debugger registration evidence: {error}"))?;
    let mut body = String::new();
    file.take(REGISTRATION_LIMIT + 1)
        .read_to_string(&mut body)
        .map_err(|error| format!("cannot read Windows debugger registration evidence: {error}"))?;
    if body.len() as u64 > REGISTRATION_LIMIT {
        return Err("Windows debugger registration evidence exceeds the size limit".to_string());
    }
    let mut evidence: WindowsDebugRegistrationEvidence = serde_json::from_str(&body)
        .map_err(|error| format!("invalid Windows debugger registration evidence: {error}"))?;
    evidence.state_path = state_path;
    validate_evidence(&evidence)?;
    if evidence.state_path.parent() != Some(user_root.as_path()) {
        return Err("Windows debugger registration evidence escaped its private root".to_string());
    }
    Ok(Some(evidence))
}

pub(crate) fn retire_windows_debug_registration(
    user_root: &Path,
    expected_registration_id: &str,
) -> Result<(), String> {
    for (name, evidence) in [
        (
            TRANSIENT_REGISTRATION_NAME,
            read_transient_windows_debug_registration(user_root)?,
        ),
        (
            REGISTRATION_NAME,
            read_windows_debug_registration(user_root)?,
        ),
    ] {
        if evidence
            .as_ref()
            .is_some_and(|evidence| evidence.registration_id == expected_registration_id)
        {
            return retire_windows_debug_registration_named_file(user_root, name);
        }
    }
    Err("Windows debugger registration evidence changed or does not exist".to_string())
}

pub(crate) fn retire_windows_debug_registration_file(user_root: &Path) -> Result<(), String> {
    retire_windows_debug_registration_named_file(user_root, REGISTRATION_NAME)
}

fn retire_windows_debug_registration_named_file(
    user_root: &Path,
    name: &str,
) -> Result<(), String> {
    let Some((_, state_path, _)) = registration_file(user_root, name)? else {
        return Ok(());
    };
    fs::remove_file(&state_path).map_err(|error| {
        format!("cannot retire Windows debugger registration evidence: {error}")
    })?;
    match fs::symlink_metadata(&state_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => {
            Err("Windows debugger registration evidence still exists after retirement".to_string())
        }
        Err(error) => Err(format!(
            "cannot verify Windows debugger registration retirement: {error}"
        )),
    }
}

pub fn recover_transient_windows_debug_registration_with<R, P, D>(
    user_root: &Path,
    running_package_processes: R,
    package_is_installed: P,
    disable: D,
) -> Result<bool, String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnMut(&str) -> Result<bool, String>,
    D: FnMut(&str) -> Result<(), String>,
{
    recover_transient_windows_debug_registration_with_restore(
        user_root,
        running_package_processes,
        package_is_installed,
        disable,
        |_| {
            Err(
                "an installed Windows registration cannot be restored through this recovery path"
                    .to_string(),
            )
        },
    )
}

pub fn recover_transient_windows_debug_registration_with_restore<R, P, D, E>(
    user_root: &Path,
    mut running_package_processes: R,
    mut package_is_installed: P,
    mut disable: D,
    mut enable_installed: E,
) -> Result<bool, String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnMut(&str) -> Result<bool, String>,
    D: FnMut(&str) -> Result<(), String>,
    E: FnMut(&WindowsInstallState) -> Result<(), String>,
{
    let Some(evidence) = read_transient_windows_debug_registration(user_root)? else {
        let Some(evidence) = read_windows_debug_registration(user_root)? else {
            return Ok(false);
        };
        if evidence.kind == WindowsDebugRegistrationKind::Installed {
            let state = read_windows_install_state(user_root)?;
            if state
                .as_ref()
                .is_some_and(|state| registration_matches_install_state(&evidence, state))
            {
                return Ok(false);
            }
            return Err(
                "Windows Runtime registration requires `incodex uninstall` before this command"
                    .to_string(),
            );
        }
        return Ok(false);
    };
    let running = running_package_processes(&evidence.package_full_name)
        .map_err(|error| format!("cannot inspect running Windows Codex processes: {error}"))?;
    if !running.is_empty() {
        return Err(format!(
            "close Codex before recovering its transient Windows debugger registration (running package PIDs: {})",
            running
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let package_is_current = package_is_installed(&evidence.package_full_name)?;
    let installed_state = read_windows_install_state(user_root)?;
    let installed_evidence = read_windows_debug_registration(user_root)?;
    let installed_state_to_restore = installed_state.as_ref().filter(|state| {
        package_is_current
            && state.desired_enabled()
            && state.package_full_name == evidence.package_full_name
            && installed_evidence
                .as_ref()
                .is_some_and(|installed| registration_matches_install_state(installed, state))
    });
    if let Some(state) = installed_state_to_restore {
        enable_installed(state)?;
    } else if package_is_current {
        disable(&evidence.package_full_name)?;
    }
    let running_after_disable = running_package_processes(&evidence.package_full_name)
        .map_err(|error| {
            format!(
                "cannot prove Codex remained closed while recovering its transient Windows debugger registration: {error}"
            )
        })?;
    if !running_after_disable.is_empty() {
        return Err(format!(
            "Codex started while its transient Windows debugger registration was being recovered (running package PIDs: {})",
            running_after_disable
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    retire_windows_debug_registration(user_root, &evidence.registration_id)?;
    Ok(true)
}

pub(crate) fn registration_matches_install_state(
    evidence: &WindowsDebugRegistrationEvidence,
    state: &WindowsInstallState,
) -> bool {
    evidence.kind == WindowsDebugRegistrationKind::Installed
        && evidence.registration_id == state.registration_id
        && evidence.package_full_name == state.package_full_name
        && evidence.helper_path == state.helper_path
        && evidence.helper_sha256 == state.helper_sha256
}

fn registration_file(
    user_root: &Path,
    name: &str,
) -> Result<Option<(PathBuf, PathBuf, fs::Metadata)>, String> {
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
    let state_path = user_root.join(name);
    let metadata = match fs::symlink_metadata(&state_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "cannot inspect Windows debugger registration evidence: {error}"
            ))
        }
    };
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!(
            "Windows debugger registration evidence is not a regular file: {}",
            state_path.display()
        ));
    }
    Ok(Some((user_root, state_path, metadata)))
}

fn validate_evidence(evidence: &WindowsDebugRegistrationEvidence) -> Result<(), String> {
    if evidence.schema_version != REGISTRATION_SCHEMA {
        return Err("Windows debugger registration schema is invalid".to_string());
    }
    validate_registration_id(&evidence.registration_id)?;
    validate_package_name(&evidence.package_full_name)?;
    if evidence.helper_sha256.len() != 64
        || !evidence
            .helper_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Windows debugger registration helper identity is invalid".to_string());
    }
    let user_root = evidence
        .state_path
        .parent()
        .ok_or_else(|| "Windows debugger registration has no Incodex root".to_string())?;
    let expected_name = match evidence.kind {
        WindowsDebugRegistrationKind::Transient => TRANSIENT_REGISTRATION_NAME,
        WindowsDebugRegistrationKind::Installed => REGISTRATION_NAME,
    };
    if evidence
        .state_path
        .file_name()
        .and_then(|name| name.to_str())
        != Some(expected_name)
    {
        return Err(
            "Windows debugger registration evidence path does not match its kind".to_string(),
        );
    }
    let helper_matches = match evidence.kind {
        WindowsDebugRegistrationKind::Transient => {
            evidence.helper_path
                == user_root
                    .join("windows")
                    .join("t")
                    .join(&evidence.helper_sha256[..16])
                    .join("i.exe")
        }
        WindowsDebugRegistrationKind::Installed => installed_windows_helper_path_matches(
            user_root,
            &evidence.helper_sha256,
            &evidence.helper_path,
        ),
    };
    if !helper_matches {
        return Err("Windows debugger registration helper path is invalid".to_string());
    }
    Ok(())
}

fn validate_registration_id(registration_id: &str) -> Result<(), String> {
    if registration_id.len() != 32 || !registration_id.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Windows debugger registration id is invalid".to_string());
    }
    Ok(())
}

fn validate_package_name(package_full_name: &str) -> Result<(), String> {
    if package_full_name.trim().is_empty() || package_full_name.contains(['\0', '\r', '\n']) {
        Err("Windows debugger registration package name is invalid".to_string())
    } else {
        Ok(())
    }
}
