/*
 * [INPUT]: 依赖 transaction target lock、私有资产发布与 Security 一次性工具；不依赖宿主/TCC。
 * [OUTPUT]: 提供稳定 LocalSigningIdentity 的显式首建、纯只读读取及显式非交互解锁。
 * [POS]: CLI 注册与签名策略之间唯一的本机身份资产边界；坏状态拒绝重生，私钥不进入 metadata。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use incodex_macos::LocalSigningIdentity;
use incodex_transaction::acquire_target_lock;
use serde::{Deserialize, Serialize};

use crate::macos_update_assets::{is_sha256, publish_content_addressed_file, sha256_hex};

#[path = "macos_signing_files.rs"]
mod files;
#[path = "macos_signing_process.rs"]
mod process;
use files::{
    create_empty_private_file, ensure_owned_private_directory, lstat_exists, read_private_file,
    read_regular_file_at_mode, set_directory_mode, sync_parent, validate_file_metadata_at_mode,
    validate_private_directory, validate_private_file, validate_root_if_present,
    write_new_private_atomic, write_new_private_file, StagingDirectory,
};
use process::{run_bounded, run_native_store, run_openssl};

const SIGNING_DIRECTORY: &str = "macos-signing";
const REGISTRATION_FILE: &str = "identity.json";
const KEYCHAIN_FILE: &str = "identity.keychain-db";
const CERTIFICATE_FILE: &str = "certificate.der";
const PASSWORD_FILE: &str = "identity.password";
const NATIVE_STORE_FILE: &str = "incodex-signing-store";
// Apple Security 的 AtomicFile 锁名为 .fl + 固定 identity.keychain-db basename 的 SHA-1 前四字节大写 hex。
const SECURITY_LOCK_FILE: &str = ".fl49A880D4";
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;
const HELPER_FILE_MODE: u32 = 0o700;
const MAX_REGISTRATION_BYTES: usize = 4096;
const MAX_CERTIFICATE_BYTES: usize = 64 * 1024;
const MAX_KEYCHAIN_BYTES: u64 = 64 * 1024 * 1024;
const MAX_NATIVE_STORE_BYTES: usize = 64 * 1024 * 1024;
const MAX_OPENSSL_OUTPUT_BYTES: usize = 1024;
const OPENSSL_TIMEOUT: Duration = Duration::from_secs(120);
const NATIVE_STORE_TIMEOUT: Duration = Duration::from_secs(20);
const OPENSSL_PATH: &str = "/usr/bin/openssl";
const NATIVE_STORE_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/incodex-signing-store"));

static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SigningIdentityRegistration {
    schema_version: u32,
    keychain_path: PathBuf,
    certificate_sha1: String,
    certificate_sha256: String,
    store_path: PathBuf,
    store_sha256: String,
}

struct NativeStoreAsset {
    path: PathBuf,
    sha256: String,
}

/// 只允许显式安装入口创建身份；并发安装由稳定 registration target lock 串行化。
pub fn ensure_signing_identity(root: &Path) -> Result<LocalSigningIdentity, String> {
    require_absolute_root(root)?;
    ensure_private_root_for_write(root)?;
    let registration_path = registration_path(root);
    let _lock = acquire_target_lock(root, &registration_path, "macos-signing-identity", None)?;

    if let Some(identity) = read_signing_identity(root)? {
        return Ok(identity);
    }
    let state_directory = state_directory(root);
    if lstat_exists(&state_directory)? {
        return Err(
            "macOS signing identity state is partial; refusing automatic regeneration".into(),
        );
    }

    // 发布无秘密工具可以重试；私有身份目录一经创建，任何失败都留下失败关闭状态。
    let native_store = publish_native_store(root)?;
    fs::create_dir(&state_directory)
        .map_err(|error| format!("cannot create private signing identity directory: {error}"))?;
    set_directory_mode(&state_directory, PRIVATE_DIRECTORY_MODE)?;
    validate_private_directory(&state_directory)?;
    let marker = state_directory.join(".initializing");
    write_new_private_file(&marker, b"identity initialization in progress\n")?;

    let result = create_identity_material(&state_directory, &native_store);
    let (identity, registration) = match result {
        Ok(created) => created,
        Err(error) => return Err(error),
    };

    validate_identity_assets(root, &registration, true)?;
    fs::remove_file(&marker)
        .map_err(|error| format!("cannot finalize signing identity initialization: {error}"))?;
    sync_parent(&marker)?;
    let bytes = format!(
        "{}\n",
        serde_json::to_string(&registration)
            .map_err(|error| format!("cannot encode signing identity registration: {error}"))?
    );
    write_new_private_atomic(&registration_path, bytes.as_bytes())?;

    // Registration is the final publish point. A failed read here is not repaired in place.
    read_signing_identity(root)?
        .filter(|registered| registered == &identity)
        .ok_or_else(|| "new macOS signing identity failed registration validation".into())
}

/// 纯读取已登记身份；不创建目录、不发布 native asset、不解锁 Keychain。
pub fn read_signing_identity(root: &Path) -> Result<Option<LocalSigningIdentity>, String> {
    require_absolute_root(root)?;
    read_identity_registration(root)?
        .map(identity_from_registration)
        .transpose()
}

fn read_identity_registration(root: &Path) -> Result<Option<SigningIdentityRegistration>, String> {
    if !validate_root_if_present(root)? {
        return Ok(None);
    }
    let state_directory = state_directory(root);
    if !lstat_exists(&state_directory)? {
        return Ok(None);
    }
    validate_private_directory(&state_directory)?;

    let path = registration_path(root);
    let Some(bytes) = read_private_file(&path, 1, MAX_REGISTRATION_BYTES, "signing registration")?
    else {
        return Err(
            "macOS signing identity directory has no registration; refusing partial state".into(),
        );
    };
    let registration: SigningIdentityRegistration = serde_json::from_slice(&bytes)
        .map_err(|_| "macOS signing identity registration is malformed".to_string())?;
    validate_identity_assets(root, &registration, false)?;
    Ok(Some(registration))
}

/// 仅使用该 root 已登记的身份解锁；调用者提供的指纹/路径不能替代 registration。
pub fn unlock_signing_identity(root: &Path, identity: &LocalSigningIdentity) -> Result<(), String> {
    require_absolute_root(root)?;
    let registration_path = registration_path(root);
    let _lock = acquire_target_lock(
        root,
        &registration_path,
        "macos-signing-identity-unlock",
        None,
    )?;
    let registration =
        read_identity_registration(root)?.ok_or("macOS signing identity is not registered")?;
    let registered = identity_from_registration(registration.clone())?;
    if &registered != identity {
        return Err("requested macOS signing identity does not match this Incodex root".into());
    }

    let state_directory = state_directory(root);
    let keychain_path = state_directory.join(KEYCHAIN_FILE);
    let password_path = state_directory.join(PASSWORD_FILE);
    validate_private_file(&keychain_path, 1, MAX_KEYCHAIN_BYTES, "signing Keychain")?;
    validate_private_file(&password_path, 32, 1024, "signing Keychain password")?;
    run_native_store(
        &registration.store_path,
        &[
            "unlock",
            path_arg(&keychain_path)?,
            path_arg(&password_path)?,
        ],
        b"{\"privateIdentityUnlocked\":true}\n",
    )?;
    validate_private_file(&keychain_path, 1, MAX_KEYCHAIN_BYTES, "signing Keychain")?;
    Ok(())
}

fn create_identity_material(
    state_directory: &Path,
    native_store: &NativeStoreAsset,
) -> Result<(LocalSigningIdentity, SigningIdentityRegistration), String> {
    let mut password = random_hex(32)?.into_bytes();
    let password_path = state_directory.join(PASSWORD_FILE);
    write_new_private_file(&password_path, &password)?;
    password.fill(0);

    let staging = create_staging_directory(state_directory)?;
    let key_path = staging.0.join("identity.key.pem");
    let certificate_pem_path = staging.0.join("identity.cert.pem");
    let certificate_der_staging_path = staging.0.join(CERTIFICATE_FILE);
    let pkcs12_path = staging.0.join("identity.p12");
    for path in [
        &key_path,
        &certificate_pem_path,
        &certificate_der_staging_path,
        &pkcs12_path,
    ] {
        create_empty_private_file(path)?;
    }

    run_openssl(
        Command::new(OPENSSL_PATH)
            .args([
                "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-batch", "-keyout",
            ])
            .arg(&key_path)
            .arg("-out")
            .arg(&certificate_pem_path)
            .args([
                "-days",
                "36500",
                "-subj",
                "/CN=Incodex Local Signing Identity",
                "-addext",
                "basicConstraints=critical,CA:FALSE",
                "-addext",
                "keyUsage=critical,digitalSignature",
                "-addext",
                "extendedKeyUsage=critical,codeSigning",
            ]),
        "generate local signing certificate",
    )?;
    run_openssl(
        Command::new(OPENSSL_PATH)
            .args(["x509", "-in"])
            .arg(&certificate_pem_path)
            .arg("-outform")
            .arg("DER")
            .arg("-out")
            .arg(&certificate_der_staging_path),
        "encode local signing certificate",
    )?;
    run_openssl(
        Command::new(OPENSSL_PATH)
            .args(["pkcs12", "-export", "-inkey"])
            .arg(&key_path)
            .arg("-in")
            .arg(&certificate_pem_path)
            .arg("-out")
            .arg(&pkcs12_path)
            .arg("-passout")
            .arg(format!("file:{}", password_path.display()))
            .args(["-name", "Incodex Local Signing Identity"]),
        "package local signing identity for private Keychain import",
    )?;

    for (path, label, maximum) in [
        (
            &key_path,
            "temporary private key",
            MAX_CERTIFICATE_BYTES * 4,
        ),
        (
            &certificate_pem_path,
            "temporary signing certificate",
            MAX_CERTIFICATE_BYTES,
        ),
        (
            &certificate_der_staging_path,
            "temporary DER certificate",
            MAX_CERTIFICATE_BYTES,
        ),
        (
            &pkcs12_path,
            "temporary PKCS12 identity",
            MAX_CERTIFICATE_BYTES * 8,
        ),
    ] {
        validate_private_file(path, 1, maximum as u64, label)?;
    }

    let certificate_der = read_private_file(
        &certificate_der_staging_path,
        1,
        MAX_CERTIFICATE_BYTES,
        "temporary DER certificate",
    )?
    .ok_or("generated local signing certificate disappeared")?;
    let certificate_sha1 = openssl_digest(&certificate_der_staging_path, "-sha1")?;
    let certificate_sha256 = sha256_hex(&certificate_der);
    validate_fingerprint(&certificate_sha1, 40, "certificate SHA-1")?;
    validate_fingerprint(&certificate_sha256, 64, "certificate SHA-256")?;

    let keychain_path = state_directory.join(KEYCHAIN_FILE);
    run_native_store(
        &native_store.path,
        &[
            "create",
            path_arg(&keychain_path)?,
            path_arg(&password_path)?,
            path_arg(&pkcs12_path)?,
        ],
        b"{\"privateIdentityReady\":true}\n",
    )?;
    validate_private_file(&keychain_path, 1, MAX_KEYCHAIN_BYTES, "signing Keychain")?;

    let certificate_path = state_directory.join(CERTIFICATE_FILE);
    write_new_private_file(&certificate_path, &certificate_der)?;
    let registration = SigningIdentityRegistration {
        schema_version: 1,
        keychain_path: keychain_path.clone(),
        certificate_sha1,
        certificate_sha256,
        store_path: native_store.path.clone(),
        store_sha256: native_store.sha256.clone(),
    };
    let identity = identity_from_registration(registration.clone())?;
    Ok((identity, registration))
}

fn validate_identity_assets(
    root: &Path,
    registration: &SigningIdentityRegistration,
    allow_initializing_marker: bool,
) -> Result<(), String> {
    if registration.schema_version != 1 {
        return Err(format!(
            "unsupported macOS signing identity schema: {}",
            registration.schema_version
        ));
    }
    let state_directory = state_directory(root);
    let expected_keychain = state_directory.join(KEYCHAIN_FILE);
    if registration.keychain_path != expected_keychain {
        return Err("registered signing Keychain escaped its private identity directory".into());
    }
    validate_fingerprint(&registration.certificate_sha1, 40, "certificate SHA-1")?;
    validate_fingerprint(&registration.certificate_sha256, 64, "certificate SHA-256")?;
    validate_private_file(
        &registration.keychain_path,
        1,
        MAX_KEYCHAIN_BYTES,
        "signing Keychain",
    )?;
    let password_path = state_directory.join(PASSWORD_FILE);
    validate_private_file(&password_path, 32, 1024, "signing Keychain password")?;
    let certificate_path = state_directory.join(CERTIFICATE_FILE);
    let certificate = read_private_file(
        &certificate_path,
        1,
        MAX_CERTIFICATE_BYTES,
        "registered signing certificate",
    )?
    .ok_or("registered signing certificate is missing")?;
    let observed_sha256 = sha256_hex(&certificate);
    if observed_sha256 != registration.certificate_sha256 {
        return Err("registered signing certificate SHA-256 does not match its metadata".into());
    }
    validate_certificate_with_openssl(&certificate_path)?;
    let observed_sha1 = openssl_digest(&certificate_path, "-sha1")?;
    if observed_sha1 != registration.certificate_sha1 {
        return Err("registered signing certificate SHA-1 does not match its metadata".into());
    }
    let certificate_path = state_directory.join(CERTIFICATE_FILE);
    validate_registered_store(root, registration)?;
    run_native_store(
        &registration.store_path,
        &[
            "verify",
            path_arg(&registration.keychain_path)?,
            path_arg(&certificate_path)?,
        ],
        b"{\"privateIdentityVerified\":true}\n",
    )?;
    validate_state_contents(&state_directory, allow_initializing_marker)?;
    Ok(())
}

fn validate_certificate_with_openssl(path: &Path) -> Result<(), String> {
    run_openssl(
        Command::new(OPENSSL_PATH)
            .args(["x509", "-inform", "DER", "-in"])
            .arg(path)
            .args(["-noout"]),
        "validate registered signing certificate",
    )
}

fn openssl_digest(path: &Path, algorithm: &str) -> Result<String, String> {
    let output = run_bounded(
        Command::new(OPENSSL_PATH)
            .args(["dgst", algorithm, "-hex"])
            .arg(path),
        OPENSSL_TIMEOUT,
        MAX_OPENSSL_OUTPUT_BYTES,
        "read signing certificate fingerprint",
    )?;
    let text = std::str::from_utf8(&output)
        .map_err(|_| "openssl returned malformed certificate fingerprint".to_string())?;
    let digest = text
        .split_once('=')
        .map(|(_, value)| value.trim())
        .ok_or("openssl returned malformed certificate fingerprint")?;
    let digest = digest.to_ascii_lowercase();
    if !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("openssl returned malformed certificate fingerprint".into());
    }
    Ok(digest)
}

fn identity_from_registration(
    registration: SigningIdentityRegistration,
) -> Result<LocalSigningIdentity, String> {
    LocalSigningIdentity::new(
        registration.keychain_path,
        registration.certificate_sha1,
        registration.certificate_sha256,
    )
}

fn validate_state_contents(
    state_directory: &Path,
    allow_initializing_marker: bool,
) -> Result<(), String> {
    for entry in fs::read_dir(state_directory)
        .map_err(|error| format!("cannot inspect signing identity directory: {error}"))?
    {
        let entry =
            entry.map_err(|error| format!("cannot inspect signing identity entry: {error}"))?;
        let name = entry.file_name();
        if name == SECURITY_LOCK_FILE {
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|error| format!("cannot inspect Security Keychain lock file: {error}"))?;
            if !metadata.file_type().is_file()
                || metadata.uid() != unsafe { libc::geteuid() }
                || metadata.permissions().mode() & 0o777 != 0o444
                || metadata.nlink() != 1
                || metadata.len() != 0
            {
                return Err("Apple Security Keychain lock file has unexpected metadata".into());
            }
            continue;
        }
        if name != REGISTRATION_FILE
            && name != KEYCHAIN_FILE
            && name != CERTIFICATE_FILE
            && name != PASSWORD_FILE
            && !(allow_initializing_marker && name == ".initializing")
        {
            return Err(
                "macOS signing identity directory contains unexpected partial state".into(),
            );
        }
    }
    Ok(())
}

fn publish_native_store(root: &Path) -> Result<NativeStoreAsset, String> {
    let helpers = root.join("helpers");
    ensure_owned_private_directory(&helpers, true)?;
    let signing_helpers = helpers.join("macos-signing");
    ensure_owned_private_directory(&signing_helpers, true)?;
    let digest = sha256_hex(NATIVE_STORE_BYTES);
    let release = signing_helpers.join(&digest);
    ensure_owned_private_directory(&release, true)?;
    let path = release.join(NATIVE_STORE_FILE);
    publish_content_addressed_file(
        &path,
        NATIVE_STORE_BYTES,
        &digest,
        HELPER_FILE_MODE,
        "macOS signing identity store",
    )?;
    validate_file_metadata_at_mode(
        &path,
        NATIVE_STORE_BYTES.len() as u64,
        NATIVE_STORE_BYTES.len() as u64,
        HELPER_FILE_MODE,
        "macOS signing identity store",
    )?;
    let published = read_regular_file_at_mode(
        &path,
        NATIVE_STORE_BYTES.len(),
        NATIVE_STORE_BYTES.len(),
        HELPER_FILE_MODE,
        "macOS signing identity store",
    )?
    .ok_or("published macOS signing identity store is missing")?;
    if published != NATIVE_STORE_BYTES || !is_sha256(&digest) {
        return Err("published macOS signing identity store failed its content hash".into());
    }
    Ok(NativeStoreAsset {
        path,
        sha256: digest,
    })
}

fn validate_registered_store(
    root: &Path,
    registration: &SigningIdentityRegistration,
) -> Result<(), String> {
    validate_fingerprint(&registration.store_sha256, 64, "native store SHA-256")?;
    let helpers = root.join("helpers");
    let signing_helpers = helpers.join("macos-signing");
    let release = signing_helpers.join(&registration.store_sha256);
    let expected_path = release.join(NATIVE_STORE_FILE);
    if registration.store_path != expected_path {
        return Err("registered macOS signing identity store escaped its private root".into());
    }
    for directory in [&helpers, &signing_helpers, &release] {
        validate_private_directory(directory)?;
    }
    let bytes = read_regular_file_at_mode(
        &registration.store_path,
        1,
        MAX_NATIVE_STORE_BYTES,
        HELPER_FILE_MODE,
        "registered macOS signing identity store",
    )?
    .ok_or("registered macOS signing identity store is missing")?;
    if sha256_hex(&bytes) != registration.store_sha256 {
        return Err("registered macOS signing identity store failed its content hash".into());
    }
    Ok(())
}

fn create_staging_directory(state_directory: &Path) -> Result<StagingDirectory, String> {
    for _ in 0..8 {
        let suffix = random_hex(16)?;
        let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = state_directory.join(format!(
            ".staging-{}-{sequence}-{suffix}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => {
                set_directory_mode(&path, PRIVATE_DIRECTORY_MODE)?;
                validate_private_directory(&path)?;
                return Ok(StagingDirectory(path));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "cannot create private signing staging directory: {error}"
                ))
            }
        }
    }
    Err("cannot allocate unique private signing staging directory".into())
}

fn random_hex(length: usize) -> Result<String, String> {
    let mut bytes = vec![0u8; length];
    File::open("/dev/urandom")
        .and_then(|mut random| random.read_exact(&mut bytes))
        .map_err(|_| "cannot obtain randomness for local signing identity".to_string())?;
    let value = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    bytes.fill(0);
    Ok(value)
}

fn require_absolute_root(root: &Path) -> Result<(), String> {
    if root.is_absolute() {
        Ok(())
    } else {
        Err("macOS signing identity needs an absolute Incodex root".into())
    }
}

fn ensure_private_root_for_write(root: &Path) -> Result<(), String> {
    ensure_owned_private_directory(root, true)
}

fn registration_path(root: &Path) -> PathBuf {
    state_directory(root).join(REGISTRATION_FILE)
}

fn state_directory(root: &Path) -> PathBuf {
    root.join(SIGNING_DIRECTORY)
}

fn validate_fingerprint(value: &str, length: usize, label: &str) -> Result<(), String> {
    if value.len() != length
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("macOS signing identity has invalid {label}"));
    }
    Ok(())
}

fn path_arg(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| "macOS signing identity path is not UTF-8".to_string())
}
