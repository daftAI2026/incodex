use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
const AUTHORIZATION_TIMEOUT: Duration = Duration::from_secs(300);
const AUTHORIZATION_POLL_INTERVAL: Duration = Duration::from_millis(25);
const AUTHORIZATION_CANCEL_GRACE: Duration = Duration::from_millis(500);
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
    #[serde(default)]
    pub authorization_ready: bool,
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
    root: &Path,
    registration: &KeychainRegistration,
) -> Result<KeychainAuthorization, String> {
    authorize_registration_with_timeout(root, registration, AUTHORIZATION_TIMEOUT)
}

fn authorize_registration_with_timeout(
    root: &Path,
    registration: &KeychainRegistration,
    timeout: Duration,
) -> Result<KeychainAuthorization, String> {
    let current = read_registration(root)?
        .ok_or("macOS Keychain registration disappeared before authorization")?;
    ensure_same_registration_identity(&current, registration)?;
    if current.authorization_ready {
        return Ok(KeychainAuthorization::Authorized);
    }
    let mut command = Command::new(&registration.helper_path);
    command
        .arg("--authorize")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            persist_authorization_readiness(root, registration, false)?;
            return Err(format!(
                "cannot start macOS Keychain authorization: {error}"
            ));
        }
    };
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(
                    AUTHORIZATION_POLL_INTERVAL.min(timeout.saturating_sub(started.elapsed())),
                );
            }
            Ok(None) => {
                terminate_authorization_child(&mut child);
                persist_authorization_readiness(root, registration, false)?;
                return Err("macOS Keychain authorization timed out".into());
            }
            Err(error) => {
                terminate_authorization_child(&mut child);
                persist_authorization_readiness(root, registration, false)?;
                return Err(format!(
                    "cannot wait for macOS Keychain authorization: {error}"
                ));
            }
        }
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_end(&mut stdout)
            .map_err(|error| format!("cannot read macOS Keychain authorization output: {error}"))?;
    }
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_end(&mut stderr)
            .map_err(|error| format!("cannot read macOS Keychain authorization errors: {error}"))?;
    }
    if !stdout.is_empty() || !stderr.is_empty() {
        persist_authorization_readiness(root, registration, false)?;
        return Err("macOS Keychain authorization helper emitted unexpected output".into());
    }
    let authorization = match status.code() {
        Some(0) => KeychainAuthorization::Authorized,
        Some(44) => KeychainAuthorization::ItemMissing,
        Some(68) => {
            persist_authorization_readiness(root, registration, false)?;
            return Err("macOS Keychain authorization was not granted".into());
        }
        Some(code) => {
            persist_authorization_readiness(root, registration, false)?;
            return Err(format!(
                "macOS Keychain authorization helper failed with exit code {code}"
            ));
        }
        None => {
            persist_authorization_readiness(root, registration, false)?;
            return Err("macOS Keychain authorization helper ended by signal".into());
        }
    };
    persist_authorization_readiness(
        root,
        registration,
        authorization == KeychainAuthorization::Authorized,
    )?;
    Ok(authorization)
}

fn terminate_authorization_child(child: &mut Child) {
    let pid = child.id() as i32;
    if pid > 0 {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
        let started = Instant::now();
        while started.elapsed() < AUTHORIZATION_CANCEL_GRACE {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => std::thread::sleep(AUTHORIZATION_POLL_INTERVAL),
                Err(_) => break,
            }
        }
        unsafe {
            // 主 helper 即使已退出，也要清理同一进程组里的异常后代。
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn persist_authorization_readiness(
    root: &Path,
    expected: &KeychainRegistration,
    authorization_ready: bool,
) -> Result<(), String> {
    let path = registration_path(root);
    let _lock = acquire_target_lock(root, &path, "macos-keychain-registration", None)?;
    let mut current = read_registration(root)?
        .ok_or("macOS Keychain registration disappeared during authorization")?;
    ensure_same_registration_identity(&current, expected)?;
    if current.authorization_ready == authorization_ready {
        return Ok(());
    }
    current.authorization_ready = authorization_ready;
    write_registration(&path, &current)
}

fn ensure_same_registration_identity(
    current: &KeychainRegistration,
    expected: &KeychainRegistration,
) -> Result<(), String> {
    if current.schema_version != expected.schema_version
        || current.app_path != expected.app_path
        || current.helper_path != expected.helper_path
        || current.helper_sha256 != expected.helper_sha256
    {
        return Err("macOS Keychain registration changed during authorization".into());
    }
    Ok(())
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
                || provider_metadata.uid() != unsafe { libc::geteuid() }
                || provider_metadata.permissions().mode() & 0o777 != PROVIDER_FILE_MODE
                || provider_metadata.nlink() != 1
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

    let current = read_registration(root)?;
    if let Some(current) = current.as_ref() {
        if current.app_path != app_path {
            return Err("macOS Keychain app path change requires an explicit migration".into());
        }
        if current.authorization_ready {
            return Ok(current.clone());
        }
    }

    let helper_bytes = load_helper()?;
    let helper_sha256 = sha256_hex(&helper_bytes);
    if let Some(current) = current {
        if current.helper_sha256 == helper_sha256 {
            return Ok(current);
        }
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
        authorization_ready: false,
    };
    write_registration(&registration_path, &registration)?;
    Ok(registration)
}

fn write_registration(path: &Path, registration: &KeychainRegistration) -> Result<(), String> {
    let body = format!(
        "{}\n",
        serde_json::to_string(registration).map_err(|error| error.to_string())?
    );
    write_private_atomic(path, body.as_bytes())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn foreground_authorization_allows_human_interaction() {
        assert!(
            AUTHORIZATION_TIMEOUT >= Duration::from_secs(300),
            "a visible system password prompt must remain available long enough for the user to notice and complete it"
        );
    }

    #[test]
    fn foreground_authorization_has_a_deadline_and_kills_its_process_group() {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let home = std::env::temp_dir().join(format!(
            "incodex-keychain-authorization-timeout-{}-{sequence}",
            std::process::id()
        ));
        let root = home.join(".incodex");
        let app = home.join("Applications/ChatGPT.app");
        let marker = home.join("orphan-finished");
        let terminated = home.join("terminated-cleanly");
        let helper = home.join("blocking-helper");
        fs::create_dir_all(&app).unwrap();
        fs::write(
            &helper,
            format!(
                "#!/bin/sh\ntrap 'printf cancelled > {}; exit 143' TERM\n/bin/sh -c 'trap \"\" TERM; /bin/sleep 2; printf orphan > {}' &\nwhile :; do :; done\n",
                terminated.display(),
                marker.display(),
            ),
        )
        .unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
        let registration = ensure_registration(&root, &app, &helper).unwrap();

        let started = Instant::now();
        let error =
            authorize_registration_with_timeout(&root, &registration, Duration::from_secs(1))
                .unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(
            fs::read(&terminated).unwrap(),
            b"cancelled",
            "authorization timeout must first give the helper a chance to dismiss native UI"
        );
        std::thread::sleep(Duration::from_millis(2_100));
        assert!(
            !marker.exists(),
            "authorization timeout left a descendant running"
        );

        fs::remove_dir_all(home).unwrap();
    }
}
