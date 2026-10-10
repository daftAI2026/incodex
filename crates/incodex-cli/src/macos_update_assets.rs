/**
 * [INPUT]: 依赖注册 root、更新 helper 与 macos_signing_assets 的稳定本地证书证明。
 * [OUTPUT]: 提供 schema-2 更新注册、按安装代际固定签名 fingerprint 与 helper 内容寻址。
 * [POS]: macOS 自动恢复的持久信任边界；legacy None 保持 ad-hoc，local 必须由当前 root 身份证明。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_certificate_sha256: Option<String>,
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
    signing_certificate_sha256: Option<String>,
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
    let signing_certificate_sha256 = match read_registration_seed(root)? {
        Some(current)
            if current.install_id == install_id || current.signing_certificate_sha256.is_some() =>
        {
            // 同 epoch 的 None 是既有 legacy；Some 则必须保持 root 稳定身份及新 app proof。
            signing_fingerprint_for_registered_generation(
                root,
                app_path,
                current.signing_certificate_sha256.as_deref(),
            )?
        }
        Some(_) => {
            // 不同 epoch 表示显式安装；legacy None 仅在新 app 已匹配 root 身份时才可升级。
            signing_fingerprint_for_new_registration(root, app_path)?
        }
        None => signing_fingerprint_for_new_registration(root, app_path)?,
    };
    publish_registration_locked(
        root,
        helper_source,
        app_path,
        install_id,
        signing_certificate_sha256,
    )
}

pub fn publish_registration_if_generation(
    root: &Path,
    helper_source: &Path,
    app_path: &Path,
    install_id: &str,
    expected_install_id: &str,
    expected_helper_sha256: &str,
) -> Result<UpdateRegistration, String> {
    let expected_signing_certificate_sha256 = read_registration(root)?
        .ok_or("macOS update registration disappeared before recovery commit")?
        .signing_certificate_sha256;
    publish_registration_if_signing_generation(
        root,
        helper_source,
        app_path,
        install_id,
        expected_install_id,
        expected_helper_sha256,
        expected_signing_certificate_sha256.as_deref(),
    )
}

pub fn publish_registration_if_signing_generation(
    root: &Path,
    helper_source: &Path,
    app_path: &Path,
    install_id: &str,
    expected_install_id: &str,
    expected_helper_sha256: &str,
    expected_signing_certificate_sha256: Option<&str>,
) -> Result<UpdateRegistration, String> {
    ensure_private_dir(root)?;
    let registration_path = registration_path(root);
    let _lock = acquire_target_lock(
        root,
        &registration_path,
        "macos-update-registration-recovery",
        Some(install_id),
    )?;
    let current = read_registration(root)?
        .ok_or("macOS update registration disappeared before recovery commit")?;
    if current.install_id != expected_install_id
        || current.app_path != app_path
        || current.helper_sha256 != expected_helper_sha256
    {
        return Err("macOS update registration generation changed before recovery commit".into());
    }
    if current.signing_certificate_sha256.as_deref() != expected_signing_certificate_sha256 {
        return Err(
            "macOS update registration signing generation changed before recovery commit".into(),
        );
    }
    let signing_certificate_sha256 = signing_fingerprint_for_registered_generation(
        root,
        app_path,
        current.signing_certificate_sha256.as_deref(),
    )?;
    publish_registration_locked(
        root,
        helper_source,
        app_path,
        install_id,
        signing_certificate_sha256,
    )
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
    if observed.signing_certificate_sha256 != current.signing_certificate_sha256 {
        return Err("macOS update registration signing mode changed before refresh".into());
    }
    let signing_certificate_sha256 = signing_fingerprint_for_registered_generation(
        root,
        &current.app_path,
        current.signing_certificate_sha256.as_deref(),
    )?;
    publish_registration_locked(
        root,
        helper_source,
        &current.app_path,
        &current.install_id,
        signing_certificate_sha256,
    )?;
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
            signing_certificate_sha256: registration.signing_certificate_sha256,
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
        signing_certificate_sha256: None,
    }))
}

fn signing_fingerprint_for_new_registration(
    root: &Path,
    app_path: &Path,
) -> Result<Option<String>, String> {
    let Some(identity) = crate::macos_signing_assets::read_signing_identity(root)? else {
        // 旧 ad-hoc 安装没有已登记身份；此处不得探测真实 app，因为创建身份只属于显式卸载/安装迁移。
        return Ok(None);
    };
    if !is_canonical_sha256(&identity.certificate_sha256) {
        return Err("registered local signing identity has an invalid SHA-256 fingerprint".into());
    }
    incodex_macos::verify_local_outer(app_path, &identity).map_err(|error| {
        format!("macOS update app does not match its registered local signing identity: {error}")
    })?;
    Ok(Some(identity.certificate_sha256))
}

fn signing_fingerprint_for_registered_generation(
    root: &Path,
    app_path: &Path,
    registered_fingerprint: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(registered_fingerprint) = registered_fingerprint else {
        // schema-1 与旧 schema-2 的 None 都明确表示 legacy ad-hoc；即使后来创建本地身份，也不可推断迁移。
        return Ok(None);
    };
    if !is_canonical_sha256(registered_fingerprint) {
        return Err(
            "macOS update registration has an invalid signing certificate fingerprint".into(),
        );
    }
    let identity = crate::macos_signing_assets::read_signing_identity(root)?
        .ok_or("macOS update registration has no matching stable local signing identity")?;
    if identity.certificate_sha256 != registered_fingerprint {
        return Err(
            "macOS update registration signing identity changed; explicitly uninstall then install to migrate"
                .into(),
        );
    }
    incodex_macos::verify_local_outer(app_path, &identity).map_err(|error| {
        format!("macOS update app does not match its registered local signing identity: {error}")
    })?;
    Ok(Some(registered_fingerprint.to_string()))
}

fn is_canonical_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
    signing_certificate_sha256: Option<String>,
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
        signing_certificate_sha256,
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
    let _pending_lock = pending_control_lock(root)?;
    remove_private_file_if_exists(&root.join("macos-update").join("pending.json"))?;
    match fs::remove_file(&path) {
        Ok(()) => sync_parent(&path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot remove macOS update registration: {error}")),
    }
}

struct PendingControlLock(File);

impl Drop for PendingControlLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn pending_control_lock(root: &Path) -> Result<PendingControlLock, String> {
    let directory = root.join("macos-update");
    ensure_private_dir(&directory)?;
    let path = directory.join(".pending.lock");
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .mode(PRIVATE_FILE_MODE)
        .custom_flags(libc::O_NOFOLLOW);
    let file = options
        .open(&path)
        .map_err(|error| format!("cannot open macOS update pending lock: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect macOS update pending lock: {error}"))?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o777 != PRIVATE_FILE_MODE
    {
        return Err("macOS update pending lock is not a private current-user file".into());
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(format!(
            "cannot lock macOS update pending state: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(PendingControlLock(file))
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
    if registration
        .signing_certificate_sha256
        .as_deref()
        .is_some_and(|fingerprint| !is_canonical_sha256(fingerprint))
    {
        return Err(
            "macOS update registration has an invalid signing certificate fingerprint".into(),
        );
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

pub(crate) fn is_sha256(value: &str) -> bool {
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

pub(crate) fn publish_content_addressed_file(
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

pub(crate) fn write_private_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
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
