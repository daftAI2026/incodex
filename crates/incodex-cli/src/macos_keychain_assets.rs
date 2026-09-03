use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use incodex_transaction::acquire_target_lock;
use serde::{Deserialize, Serialize};

use crate::macos_update_assets::{
    ensure_private_dir, is_sha256, publish_content_addressed_file, read_regular_file, sha256_hex,
    write_private_atomic,
};

const REGISTRATION_SCHEMA_VERSION: u32 = 1;
const PRIVATE_FILE_MODE: u32 = 0o600;
const HELPER_FILE_MODE: u32 = 0o700;
const HELPER_FILE_NAME: &str = "incodex-keychain-helper";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KeychainRegistration {
    pub schema_version: u32,
    pub app_path: PathBuf,
    pub helper_path: PathBuf,
    pub helper_sha256: String,
}

pub fn ensure_registration(
    root: &Path,
    app_path: &Path,
    helper_source: &Path,
) -> Result<KeychainRegistration, String> {
    if !app_path.is_absolute() {
        return Err("macOS Keychain registration needs an absolute app path".into());
    }

    let helper_bytes = read_regular_file(helper_source, "macOS Keychain helper source")?;
    let helper_sha256 = sha256_hex(&helper_bytes);
    ensure_private_dir(root)?;
    let registration_path = registration_path(root);
    let _lock = acquire_target_lock(
        root,
        &registration_path,
        "macos-keychain-registration",
        None,
    )?;

    if let Some(current) = read_registration(root)? {
        if current.app_path != app_path || current.helper_sha256 != helper_sha256 {
            return Err(
                "macOS Keychain helper identity change requires an explicit migration".into(),
            );
        }
        return Ok(current);
    }

    let helpers_root = root.join("helpers").join("macos-keychain");
    ensure_private_dir(&helpers_root)?;
    let release_dir = helpers_root.join(&helper_sha256);
    ensure_private_dir(&release_dir)?;
    let helper_path = release_dir.join(HELPER_FILE_NAME);
    publish_content_addressed_file(
        &helper_path,
        &helper_bytes,
        &helper_sha256,
        HELPER_FILE_MODE,
        "macOS Keychain helper",
    )?;

    let registration = KeychainRegistration {
        schema_version: REGISTRATION_SCHEMA_VERSION,
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

pub fn read_registration(root: &Path) -> Result<Option<KeychainRegistration>, String> {
    let path = registration_path(root);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "cannot inspect macOS Keychain registration: {error}"
            ))
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "refuse to read symlink macOS Keychain registration: {}",
            path.display()
        ));
    }
    if !metadata.file_type().is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o777 != PRIVATE_FILE_MODE
    {
        return Err("macOS Keychain registration is not a private current-user file".into());
    }

    let body = read_regular_file(&path, "macOS Keychain registration")?;
    let registration: KeychainRegistration = serde_json::from_slice(&body)
        .map_err(|error| format!("invalid macOS Keychain registration: {error}"))?;
    validate_registration(root, &registration)?;
    Ok(Some(registration))
}

fn validate_registration(root: &Path, registration: &KeychainRegistration) -> Result<(), String> {
    if registration.schema_version != REGISTRATION_SCHEMA_VERSION {
        return Err(format!(
            "unsupported macOS Keychain registration schema: {}",
            registration.schema_version
        ));
    }
    if !registration.app_path.is_absolute() || !is_sha256(&registration.helper_sha256) {
        return Err("invalid macOS Keychain registration".into());
    }
    let expected = root
        .join("helpers")
        .join("macos-keychain")
        .join(&registration.helper_sha256)
        .join(HELPER_FILE_NAME);
    if registration.helper_path != expected {
        return Err("macOS Keychain helper escaped its private root".into());
    }
    let metadata = fs::symlink_metadata(&registration.helper_path)
        .map_err(|error| format!("cannot inspect macOS Keychain helper: {error}"))?;
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o777 != HELPER_FILE_MODE
    {
        return Err("macOS Keychain helper is not a private current-user executable".into());
    }
    let helper = read_regular_file(&registration.helper_path, "macOS Keychain helper")?;
    if sha256_hex(&helper) != registration.helper_sha256 {
        return Err("macOS Keychain helper failed its content hash".into());
    }
    Ok(())
}

fn registration_path(root: &Path) -> PathBuf {
    root.join("macos-keychain").join("registration.json")
}
