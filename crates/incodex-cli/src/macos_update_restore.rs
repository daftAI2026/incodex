use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_transaction::acquire_target_lock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const REGISTRATION_SCHEMA_VERSION: u32 = 1;
const PRIVATE_DIR_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;
const HELPER_FILE_MODE: u32 = 0o700;
const HELPER_FILE_NAME: &str = "incodex";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRegistration {
    pub schema_version: u32,
    pub install_id: String,
    pub app_path: PathBuf,
    pub helper_path: PathBuf,
    pub helper_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinatorSnapshot {
    pub source_build: u64,
    pub observed_build: Option<u64>,
    pub parent_running: bool,
    pub app_running: bool,
    pub integration_installed: bool,
    pub registered: bool,
    pub grace_expired: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorAction {
    Wait,
    ExitNoUpdate,
    ExitCancelled,
    Reinstall { expected_build: u64 },
}

pub fn next_action(snapshot: CoordinatorSnapshot) -> CoordinatorAction {
    if !snapshot.registered {
        return CoordinatorAction::ExitCancelled;
    }
    if snapshot.parent_running || snapshot.app_running {
        return CoordinatorAction::Wait;
    }

    let Some(observed_build) = snapshot.observed_build else {
        return CoordinatorAction::Wait;
    };
    if !snapshot.integration_installed {
        return CoordinatorAction::Reinstall {
            expected_build: observed_build,
        };
    }
    if observed_build != snapshot.source_build {
        return CoordinatorAction::Wait;
    }
    if snapshot.grace_expired {
        CoordinatorAction::ExitNoUpdate
    } else {
        CoordinatorAction::Wait
    }
}

pub fn publish_registration(
    root: &Path,
    helper_source: &Path,
    app_path: &Path,
    install_id: &str,
) -> Result<UpdateRegistration, String> {
    if install_id.is_empty() {
        return Err("macOS update registration needs an install epoch".into());
    }
    if !app_path.is_absolute() {
        return Err("macOS update registration needs an absolute app path".into());
    }

    ensure_private_dir(root)?;
    let registration_path = registration_path(root);
    let _lock = acquire_target_lock(
        root,
        &registration_path,
        "macos-update-registration",
        Some(install_id),
    )?;
    let helper_bytes = read_regular_file(helper_source, "macOS update helper source")?;
    let helper_sha256 = sha256_hex(&helper_bytes);
    let helpers_dir = root.join("helpers");
    ensure_private_dir(&helpers_dir)?;
    let update_helpers_dir = helpers_dir.join("macos-update");
    ensure_private_dir(&update_helpers_dir)?;
    let release_dir = update_helpers_dir.join(&helper_sha256);
    ensure_private_dir(&release_dir)?;
    let helper_path = release_dir.join(HELPER_FILE_NAME);
    publish_helper(&helper_path, &helper_bytes, &helper_sha256)?;

    let registration = UpdateRegistration {
        schema_version: REGISTRATION_SCHEMA_VERSION,
        install_id: install_id.to_string(),
        app_path: app_path.to_path_buf(),
        helper_path,
        helper_sha256,
    };
    let body = format!(
        "{}\n",
        serde_json::to_string(&registration).map_err(|error| error.to_string())?
    );
    write_private_atomic(&registration_path, body.as_bytes())?;
    Ok(registration)
}

pub fn read_registration(root: &Path) -> Result<Option<UpdateRegistration>, String> {
    let path = registration_path(root);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot inspect macOS update registration: {error}")),
    };
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "refuse to read symlink macOS update registration: {}",
            path.display()
        ));
    }
    if !metadata.file_type().is_file() {
        return Err(format!(
            "macOS update registration is not a regular file: {}",
            path.display()
        ));
    }

    let body = read_regular_file(&path, "macOS update registration")?;
    let registration: UpdateRegistration =
        serde_json::from_slice(&body).map_err(|error| format!("invalid registration: {error}"))?;
    validate_registration(root, &registration)?;
    Ok(Some(registration))
}

pub fn remove_registration(root: &Path, install_id: &str) -> Result<(), String> {
    let path = registration_path(root);
    let _lock = acquire_target_lock(
        root,
        &path,
        "macos-update-registration-remove",
        Some(install_id),
    )?;
    let Some(registration) = read_registration(root)? else {
        return Ok(());
    };
    if registration.install_id != install_id {
        return Ok(());
    }
    match fs::remove_file(&path) {
        Ok(()) => sync_parent(&path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot remove macOS update registration: {error}")),
    }
}

fn registration_path(root: &Path) -> PathBuf {
    root.join("macos-update").join("registration.json")
}

fn validate_registration(root: &Path, registration: &UpdateRegistration) -> Result<(), String> {
    if registration.schema_version != REGISTRATION_SCHEMA_VERSION {
        return Err(format!(
            "unsupported macOS update registration schema: {}",
            registration.schema_version
        ));
    }
    if registration.install_id.is_empty() {
        return Err("macOS update registration has no install epoch".into());
    }
    if !registration.app_path.is_absolute() {
        return Err("macOS update registration app path is not absolute".into());
    }
    let expected_helper_path = root
        .join("helpers")
        .join("macos-update")
        .join(&registration.helper_sha256)
        .join(HELPER_FILE_NAME);
    if registration.helper_path != expected_helper_path {
        return Err("macOS update registration helper escaped its private root".into());
    }
    if registration.helper_sha256.len() != 64
        || !registration
            .helper_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("macOS update registration has an invalid helper hash".into());
    }
    Ok(())
}

fn publish_helper(path: &Path, bytes: &[u8], expected_sha256: &str) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                return Err(format!(
                    "macOS update helper is not a regular file: {}",
                    path.display()
                ));
            }
            if sha256_hex(&read_regular_file(path, "macOS update helper")?) != expected_sha256 {
                return Err("macOS update helper does not match its content address".into());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            write_private_atomic_with_mode(path, bytes, HELPER_FILE_MODE)?;
        }
        Err(error) => return Err(format!("cannot inspect macOS update helper: {error}")),
    }
    set_mode(path, HELPER_FILE_MODE)
}

fn read_regular_file(path: &Path, label: &str) -> Result<Vec<u8>, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("cannot inspect {label}: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(format!("{label} is not a regular file: {}", path.display()));
    }
    let mut options = OpenOptions::new();
    options.read(true).custom_flags(libc::O_NOFOLLOW);
    let mut file = options
        .open(path)
        .map_err(|error| format!("cannot open {label}: {error}"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {label}: {error}"))?;
    Ok(bytes)
}

fn ensure_private_dir(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!(
                "refuse to use symlink directory: {}",
                path.display()
            ));
        }
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(format!("expected directory: {}", path.display()));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path)
                .map_err(|error| format!("cannot create private directory: {error}"))?;
        }
        Err(error) => return Err(format!("cannot inspect private directory: {error}")),
    }
    set_mode(path, PRIVATE_DIR_MODE)
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_private_atomic_with_mode(path, bytes, PRIVATE_FILE_MODE)
}

fn write_private_atomic_with_mode(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("private file has no parent: {}", path.display()))?;
    ensure_private_dir(parent)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let temporary = parent.join(format!(".tmp-{}-{nonce}", std::process::id()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW);
        let mut file = options
            .open(&temporary)
            .map_err(|error| format!("cannot stage private file: {error}"))?;
        file.write_all(bytes)
            .map_err(|error| format!("cannot write private file: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("cannot flush private file: {error}"))?;
        set_file_mode(&file, mode)?;
        drop(file);
        fs::rename(&temporary, path)
            .map_err(|error| format!("cannot publish private file: {error}"))?;
        sync_parent(path)
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn set_file_mode(file: &File, mode: u32) -> Result<(), String> {
    let result = unsafe { libc::fchmod(file.as_raw_fd(), mode as libc::mode_t) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    let mut permissions = fs::metadata(path)
        .map_err(|error| error.to_string())?
        .permissions();
    permissions.set_mode(mode);
    fs::set_permissions(path, permissions).map_err(|error| error.to_string())
}

fn sync_parent(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("file has no parent: {}", path.display()))?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY);
    options
        .open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| error.to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
