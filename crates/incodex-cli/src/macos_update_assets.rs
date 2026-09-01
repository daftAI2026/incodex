use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_transaction::acquire_target_lock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const REGISTRATION_SCHEMA_VERSION: u32 = 2;
const PRIVATE_DIR_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;
const HELPER_FILE_MODE: u32 = 0o700;
const HELPER_FILE_NAME: &str = "incodex";
const COORDINATOR_APP_NAME: &str = "Incodex Update Coordinator.app";
const COORDINATOR_FILE_NAME: &str = "incodex-update-coordinator";
const INTERPOSER_FILE_NAME: &str = "libincodex-sparkle-interpose.dylib";
const COORDINATOR_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/incodex-update-coordinator"));
const INTERPOSER_BYTES: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/libincodex-sparkle-interpose.dylib"
));
const COORDINATOR_INFO_PLIST: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>incodex-update-coordinator</string>
<key>CFBundleIdentifier</key><string>com.daftai.incodex.update-coordinator</string>
<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
<key>CFBundleName</key><string>Incodex Update Coordinator</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>1</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSBackgroundOnly</key><true/>
<key>LSUIElement</key><true/>
</dict></plist>
"#;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRegistration {
    pub schema_version: u32,
    pub install_id: String,
    pub app_path: PathBuf,
    pub helper_path: PathBuf,
    pub helper_sha256: String,
    pub coordinator_app_path: PathBuf,
    pub coordinator_path: PathBuf,
    pub coordinator_sha256: String,
    pub interposer_path: PathBuf,
    pub interposer_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyRegistration {
    schema_version: u32,
    install_id: String,
    app_path: PathBuf,
    helper_path: PathBuf,
    helper_sha256: String,
}

struct RegistrationSeed {
    install_id: String,
    app_path: PathBuf,
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
    publish_registration_locked(root, helper_source, app_path, install_id)
}

pub fn refresh_registered_helper(root: &Path, helper_source: &Path) -> Result<bool, String> {
    let Some(observed) = read_registration_seed(root)? else {
        return Ok(false);
    };
    let path = registration_path(root);
    let _lock = acquire_target_lock(
        root,
        &path,
        "macos-update-registration-refresh",
        Some(&observed.install_id),
    )?;
    let Some(current) = read_registration_seed(root)? else {
        return Ok(false);
    };
    publish_registration_locked(root, helper_source, &current.app_path, &current.install_id)?;
    Ok(true)
}

fn read_registration_seed(root: &Path) -> Result<Option<RegistrationSeed>, String> {
    let Some(body) = read_registration_body(root)? else {
        return Ok(None);
    };
    let schema = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|value| value.get("schemaVersion")?.as_u64())
        .ok_or("macOS update registration has no schema version")?;
    if schema == u64::from(REGISTRATION_SCHEMA_VERSION) {
        let registration: UpdateRegistration = serde_json::from_slice(&body)
            .map_err(|error| format!("invalid registration: {error}"))?;
        validate_registration(root, &registration)?;
        return Ok(Some(RegistrationSeed {
            install_id: registration.install_id,
            app_path: registration.app_path,
        }));
    }
    if schema != 1 {
        return Err(format!(
            "unsupported macOS update registration schema: {schema}"
        ));
    }
    let legacy: LegacyRegistration = serde_json::from_slice(&body)
        .map_err(|error| format!("invalid legacy registration: {error}"))?;
    validate_legacy_registration(root, &legacy)?;
    Ok(Some(RegistrationSeed {
        install_id: legacy.install_id,
        app_path: legacy.app_path,
    }))
}

fn validate_legacy_registration(
    root: &Path,
    registration: &LegacyRegistration,
) -> Result<(), String> {
    if registration.schema_version != 1
        || registration.install_id.is_empty()
        || !registration.app_path.is_absolute()
        || !is_sha256(&registration.helper_sha256)
    {
        return Err("invalid legacy macOS update registration".into());
    }
    let expected = root
        .join("helpers")
        .join("macos-update")
        .join(&registration.helper_sha256)
        .join(HELPER_FILE_NAME);
    if registration.helper_path != expected {
        return Err("legacy macOS update helper escaped its private root".into());
    }
    let bytes = read_regular_file(&registration.helper_path, "legacy macOS update helper")?;
    if sha256_hex(&bytes) != registration.helper_sha256 {
        return Err("legacy macOS update helper failed its content hash".into());
    }
    Ok(())
}

fn publish_registration_locked(
    root: &Path,
    helper_source: &Path,
    app_path: &Path,
    install_id: &str,
) -> Result<UpdateRegistration, String> {
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
    let coordinator_app_path = release_dir.join(COORDINATOR_APP_NAME);
    let coordinator_contents = coordinator_app_path.join("Contents");
    let coordinator_macos = coordinator_contents.join("MacOS");
    ensure_private_dir(&coordinator_macos)?;
    let coordinator_path = coordinator_macos.join(COORDINATOR_FILE_NAME);
    let coordinator_sha256 = sha256_hex(COORDINATOR_BYTES);
    publish_content_addressed_file(
        &coordinator_path,
        COORDINATOR_BYTES,
        &coordinator_sha256,
        HELPER_FILE_MODE,
        "macOS update coordinator",
    )?;
    let info_plist = coordinator_contents.join("Info.plist");
    let info_sha256 = sha256_hex(COORDINATOR_INFO_PLIST);
    publish_content_addressed_file(
        &info_plist,
        COORDINATOR_INFO_PLIST,
        &info_sha256,
        PRIVATE_FILE_MODE,
        "macOS update coordinator Info.plist",
    )?;
    let interposer_path = release_dir.join(INTERPOSER_FILE_NAME);
    let interposer_sha256 = sha256_hex(INTERPOSER_BYTES);
    publish_content_addressed_file(
        &interposer_path,
        INTERPOSER_BYTES,
        &interposer_sha256,
        PRIVATE_FILE_MODE,
        "macOS Sparkle interposer",
    )?;

    let registration = UpdateRegistration {
        schema_version: REGISTRATION_SCHEMA_VERSION,
        install_id: install_id.to_string(),
        app_path: app_path.to_path_buf(),
        helper_path,
        helper_sha256,
        coordinator_app_path,
        coordinator_path,
        coordinator_sha256,
        interposer_path,
        interposer_sha256,
    };
    let body = format!(
        "{}\n",
        serde_json::to_string(&registration).map_err(|error| error.to_string())?
    );
    write_private_atomic(&registration_path(root), body.as_bytes())?;
    Ok(registration)
}

pub fn read_registration(root: &Path) -> Result<Option<UpdateRegistration>, String> {
    let Some(body) = read_registration_body(root)? else {
        return Ok(None);
    };
    let registration: UpdateRegistration =
        serde_json::from_slice(&body).map_err(|error| format!("invalid registration: {error}"))?;
    validate_registration(root, &registration)?;
    Ok(Some(registration))
}

fn read_registration_body(root: &Path) -> Result<Option<Vec<u8>>, String> {
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

    read_regular_file(&path, "macOS update registration").map(Some)
}

pub fn remove_registration(root: &Path, install_id: &str) -> Result<(), String> {
    let path = registration_path(root);
    let _lock = acquire_target_lock(
        root,
        &path,
        "macos-update-registration-remove",
        Some(install_id),
    )?;
    let Some(registration) = read_registration_seed(root)? else {
        return Ok(());
    };
    if registration.install_id != install_id {
        return Ok(());
    }
    remove_private_file_if_exists(&root.join("macos-update").join("pending.json"))?;
    match fs::remove_file(&path) {
        Ok(()) => sync_parent(&path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot remove macOS update registration: {error}")),
    }
}

fn remove_private_file_if_exists(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("cannot inspect private state: {error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(format!(
            "private state is not a regular file: {}",
            path.display()
        ));
    }
    fs::remove_file(path).map_err(|error| format!("cannot remove private state: {error}"))?;
    sync_parent(path)
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
    let release_dir = expected_helper_path
        .parent()
        .ok_or("macOS update helper has no release directory")?;
    let expected_coordinator_app = release_dir.join(COORDINATOR_APP_NAME);
    let expected_coordinator = expected_coordinator_app
        .join("Contents")
        .join("MacOS")
        .join(COORDINATOR_FILE_NAME);
    if registration.coordinator_app_path != expected_coordinator_app
        || registration.coordinator_path != expected_coordinator
    {
        return Err("macOS update coordinator escaped its private root".into());
    }
    if registration.interposer_path != release_dir.join(INTERPOSER_FILE_NAME) {
        return Err("macOS Sparkle interposer escaped its private root".into());
    }
    for (label, digest) in [
        ("helper", &registration.helper_sha256),
        ("coordinator", &registration.coordinator_sha256),
        ("interposer", &registration.interposer_sha256),
    ] {
        if !is_sha256(digest) {
            return Err(format!(
                "macOS update registration has an invalid {label} hash"
            ));
        }
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn publish_helper(path: &Path, bytes: &[u8], expected_sha256: &str) -> Result<(), String> {
    publish_content_addressed_file(
        path,
        bytes,
        expected_sha256,
        HELPER_FILE_MODE,
        "macOS update helper",
    )
}

fn publish_content_addressed_file(
    path: &Path,
    bytes: &[u8],
    expected_sha256: &str,
    mode: u32,
    label: &str,
) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                return Err(format!("{label} is not a regular file: {}", path.display()));
            }
            if sha256_hex(&read_regular_file(path, label)?) != expected_sha256 {
                return Err(format!("{label} does not match its content address"));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            write_private_atomic_with_mode(path, bytes, mode)?;
        }
        Err(error) => return Err(format!("cannot inspect {label}: {error}")),
    }
    set_mode(path, mode)
}

pub(crate) fn read_regular_file(path: &Path, label: &str) -> Result<Vec<u8>, String> {
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

pub(crate) fn ensure_private_dir(path: &Path) -> Result<(), String> {
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

pub(crate) fn set_file_mode(file: &File, mode: u32) -> Result<(), String> {
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

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
