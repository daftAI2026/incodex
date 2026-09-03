use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_macos::add_load_dylib;
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
const PROVIDER_FILE_MODE: u32 = 0o644;
const PROVIDER_FILE_NAME: &str = "IncodexKeyProvider.dylib";
const PROVIDER_HELPER_HASH_MARKER: &[u8; 64] =
    b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const FRAMEWORK_RELATIVE_PATH: &str =
    "Contents/Frameworks/Codex Framework.framework/Codex Framework";
const BUNDLED_HELPER_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/incodex-keychain-helper"));
const BUNDLED_PROVIDER_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/IncodexKeyProvider.dylib"));

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KeychainRegistration {
    pub schema_version: u32,
    pub app_path: PathBuf,
    pub helper_path: PathBuf,
    pub helper_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeychainAuthorization {
    Authorized,
    ItemMissing,
}

pub fn ensure_registration(
    root: &Path,
    app_path: &Path,
    helper_source: &Path,
) -> Result<KeychainRegistration, String> {
    ensure_registration_with(root, app_path, || {
        read_regular_file(helper_source, "macOS Keychain helper source")
    })
}

pub fn ensure_bundled_registration(
    root: &Path,
    app_path: &Path,
) -> Result<KeychainRegistration, String> {
    ensure_registration_with(root, app_path, || Ok(BUNDLED_HELPER_BYTES.to_vec()))
}

pub fn bundled_helper_bytes() -> &'static [u8] {
    BUNDLED_HELPER_BYTES
}

pub fn bundled_provider_bytes() -> &'static [u8] {
    BUNDLED_PROVIDER_BYTES
}

/// 在显式前台安装中触发一次系统授权，但不把 Keychain 数据交给 CLI。
///
/// exit 44 表示 Codex 尚未创建 storage key；调用方可以继续安装，但必须保留
/// 这条边界。其他失败（包括用户取消）均在修改应用前中止。
pub fn authorize_registration(
    registration: &KeychainRegistration,
) -> Result<KeychainAuthorization, String> {
    let output = Command::new(&registration.helper_path)
        .arg("--authorize")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("cannot start macOS Keychain authorization: {error}"))?;
    if !output.stdout.is_empty() || !output.stderr.is_empty() {
        return Err("macOS Keychain authorization helper emitted unexpected output".into());
    }
    match output.status.code() {
        Some(0) => Ok(KeychainAuthorization::Authorized),
        Some(44) => Ok(KeychainAuthorization::ItemMissing),
        Some(68) => Err("macOS Keychain authorization was not granted".into()),
        Some(code) => Err(format!(
            "macOS Keychain authorization helper failed with exit code {code}"
        )),
        None => Err("macOS Keychain authorization helper ended by signal".into()),
    }
}

/// 把固定 provider 放进 staged app，并只在 Framework 的现有 padding 中增加普通依赖。
///
/// 调用方必须把整个 staged app 纳入外层安装事务；本函数仍保证解析或本地写入失败时
/// 不留下半写 Framework，且只移除由本次调用新建的 provider。
pub fn install_keychain_provider(staged_app: &Path, helper_sha256: &str) -> Result<(), String> {
    if helper_sha256.len() != 64
        || !helper_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("Keychain provider helper hash is invalid".into());
    }
    let provider_bytes = provider_for_helper(helper_sha256)?;
    let app_root = fs::canonicalize(staged_app)
        .map_err(|error| format!("cannot resolve staged app for Keychain provider: {error}"))?;
    let framework_link = app_root.join(FRAMEWORK_RELATIVE_PATH);
    let framework = fs::canonicalize(&framework_link).map_err(|error| {
        format!("cannot resolve staged Codex Framework for Keychain provider: {error}")
    })?;
    if !framework.starts_with(&app_root) {
        return Err("staged Codex Framework escaped the staged app".into());
    }
    let metadata = fs::symlink_metadata(&framework)
        .map_err(|error| format!("cannot inspect staged Codex Framework: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("staged Codex Framework is not a regular file".into());
    }
    let original_mode = metadata.permissions().mode() & 0o777;
    let mut framework_bytes = read_regular_file(&framework, "staged Codex Framework")?;
    add_load_dylib(&mut framework_bytes)
        .map_err(|error| format!("cannot add Keychain provider dependency: {error}"))?;

    let provider = framework
        .parent()
        .ok_or("staged Codex Framework has no containing directory")?
        .join(PROVIDER_FILE_NAME);
    let provider_created = match fs::symlink_metadata(&provider) {
        Ok(provider_metadata) => {
            if provider_metadata.file_type().is_symlink()
                || !provider_metadata.file_type().is_file()
                || read_regular_file(&provider, "staged Keychain provider")? != provider_bytes
            {
                return Err("staged Keychain provider conflicts with bundled bytes".into());
            }
            false
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            write_bundle_file_atomic(&provider, &provider_bytes, PROVIDER_FILE_MODE)?;
            true
        }
        Err(error) => return Err(format!("cannot inspect staged Keychain provider: {error}")),
    };

    if let Err(error) = write_bundle_file_atomic(&framework, &framework_bytes, original_mode) {
        if provider_created {
            let _ = fs::remove_file(&provider);
        }
        return Err(error);
    }
    Ok(())
}

fn provider_for_helper(helper_sha256: &str) -> Result<Vec<u8>, String> {
    let mut provider = BUNDLED_PROVIDER_BYTES.to_vec();
    if provider.len() < PROVIDER_HELPER_HASH_MARKER.len() {
        return Err("bundled Keychain provider is truncated".into());
    }
    let mut replacements = 0;
    for offset in 0..=provider.len() - PROVIDER_HELPER_HASH_MARKER.len() {
        if &provider[offset..offset + PROVIDER_HELPER_HASH_MARKER.len()]
            == PROVIDER_HELPER_HASH_MARKER
        {
            provider[offset..offset + PROVIDER_HELPER_HASH_MARKER.len()]
                .copy_from_slice(helper_sha256.as_bytes());
            replacements += 1;
        }
    }
    if replacements == 0 {
        return Err("bundled Keychain provider has no helper hash marker".into());
    }
    Ok(provider)
}

fn ensure_registration_with<F>(
    root: &Path,
    app_path: &Path,
    load_helper: F,
) -> Result<KeychainRegistration, String>
where
    F: FnOnce() -> Result<Vec<u8>, String>,
{
    if !app_path.is_absolute() {
        return Err("macOS Keychain registration needs an absolute app path".into());
    }

    ensure_private_dir(root)?;
    let registration_path = registration_path(root);
    let _lock = acquire_target_lock(
        root,
        &registration_path,
        "macos-keychain-registration",
        None,
    )?;

    if let Some(current) = read_registration(root)? {
        if current.app_path != app_path {
            return Err("macOS Keychain app path change requires an explicit migration".into());
        }
        return Ok(current);
    }

    let helper_bytes = load_helper()?;
    let helper_sha256 = sha256_hex(&helper_bytes);

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

fn write_bundle_file_atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("bundle file has no parent: {}", path.display()))?;
    let parent_metadata = fs::symlink_metadata(parent)
        .map_err(|error| format!("cannot inspect bundle file parent: {error}"))?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.file_type().is_dir() {
        return Err(format!(
            "bundle file parent is not a real directory: {}",
            parent.display()
        ));
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let temporary = parent.join(format!(".incodex-keychain-{}-{nonce}", std::process::id()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW);
        let mut file = options
            .open(&temporary)
            .map_err(|error| format!("cannot stage bundle file: {error}"))?;
        file.write_all(bytes)
            .map_err(|error| format!("cannot write staged bundle file: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("cannot flush staged bundle file: {error}"))?;
        file.set_permissions(fs::Permissions::from_mode(mode))
            .map_err(|error| format!("cannot set staged bundle file mode: {error}"))?;
        drop(file);
        fs::rename(&temporary, path)
            .map_err(|error| format!("cannot publish staged bundle file: {error}"))?;
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("cannot flush staged bundle directory: {error}"))
    })();
    let _ = fs::remove_file(temporary);
    result
}
