/**
 * [INPUT]: 依赖 Runtime root guard、发布锁、durable write 和嵌入 helper bytes。
 * [OUTPUT]: 发布 protocol-1 登记与内容寻址 helper，并校验私有元数据。
 * [POS]: macOS Runtime helper 边界；不改写 Storage 密钥。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};

const DEVICE_KEY_DIRECTORY: &str = "macos-device-key";
const HELPER_DIRECTORY: &str = "helpers/macos-device-key";
const HELPER_NAME: &str = super::native::DEVICE_KEY_HELPER_NAME;
const REGISTRATION_NAME: &str = "registration.json";
const DEVICE_KEY_PROTOCOL_VERSION: u64 = 1;
const MAX_REGISTRATION_BYTES: u64 = 64 * 1024;
const PRIVATE_DIRECTORY_MODE: u32 = super::DIR_MODE;
const PRIVATE_FILE_MODE: u32 = super::FILE_MODE;
const EXECUTABLE_MODE: u32 = super::EXECUTABLE_FILE_MODE;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Registration {
    schema_version: u64,
    protocol_version: u64,
    helper_sha256: String,
}

/// 验证 protocol-1 登记和不可变 helper。调用方必须持有 Runtime 发布锁。
pub(crate) fn registration_available(user_root: &Path) -> Result<bool, String> {
    Ok(inspect_registration(user_root)?.is_some())
}

/// 验证 Runtime 快路径；缺少登记时重新发布。
pub(crate) fn ensure_current(
    user_root: &Path,
    identity: &super::RuntimeIdentity,
) -> Result<super::PublishedRuntime, String> {
    let deployed = super::inspect_deployed(user_root).ok().flatten();
    if deployed
        .as_ref()
        .is_some_and(|deployed| identity.matches(deployed))
    {
        let _user_root_guard = super::ensure_user_root(user_root)?;
        let runtime = user_root.join("runtime");
        let lock = super::RuntimePublishLock::acquire(&runtime)?;
        let deployed = super::inspect_deployed(user_root)?;
        if let Some(deployed) = deployed.filter(|deployed| identity.matches(deployed)) {
            if registration_available(user_root)? {
                drop(lock);
                return Ok(super::PublishedRuntime {
                    version: identity.version.clone(),
                    release: deployed.release,
                });
            }
        }
        drop(lock);
    }
    super::publish(user_root)
}

/// 永久保留有效登记。缺少登记时发布本代 helper。
/// 调用方必须持有 Runtime 发布锁。
pub(crate) fn ensure_registered(user_root: &Path, helper_bytes: &[u8]) -> Result<(), String> {
    if helper_bytes.is_empty() {
        return Err("embedded device helper is empty".into());
    }
    let _user_root_guard = super::ensure_user_root(user_root)?;
    if inspect_registration(user_root)?.is_some() {
        return Ok(());
    }

    let helper_hash = super::sha256_hex(helper_bytes);
    publish_helper(user_root, &helper_hash, helper_bytes)?;
    publish_registration(user_root, &helper_hash)?;
    if inspect_registration(user_root)?.is_none() {
        return Err("device helper registration did not become available".into());
    }
    Ok(())
}

fn inspect_registration(user_root: &Path) -> Result<Option<Registration>, String> {
    let Some(_user_root_guard) = super::inspect_user_root(user_root)? else {
        return Ok(None);
    };
    let namespace = user_root.join(DEVICE_KEY_DIRECTORY);
    if !is_private_directory_entry(&namespace, "device key namespace")? {
        return Ok(None);
    }
    let path = namespace.join(REGISTRATION_NAME);
    let Some(file) = open_optional_private_file(&path, PRIVATE_FILE_MODE, "registration")? else {
        return Ok(None);
    };
    verify_directory(&namespace, "device key namespace")?;
    let mut body = Vec::new();
    file.take(MAX_REGISTRATION_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    if body.len() as u64 > MAX_REGISTRATION_BYTES {
        return Err("device helper registration is too large".into());
    }
    let registration: Registration = serde_json::from_slice(&body)
        .map_err(|error| format!("invalid device helper registration JSON: {error}"))?;
    if registration.schema_version != 1 {
        return Err("unsupported device helper registration schema".into());
    }
    if registration.protocol_version != DEVICE_KEY_PROTOCOL_VERSION {
        return Err("unsupported device helper protocol version".into());
    }
    if !is_sha256(&registration.helper_sha256) {
        return Err("invalid device helper SHA-256".into());
    }
    verify_registered_helper(user_root, &registration.helper_sha256)?;
    Ok(Some(registration))
}

fn verify_registered_helper(user_root: &Path, helper_hash: &str) -> Result<(), String> {
    let helpers = user_root.join("helpers");
    let namespace = user_root.join(HELPER_DIRECTORY);
    let release = namespace.join(helper_hash);
    verify_required_directory(&helpers, "helpers root")?;
    verify_required_directory(&namespace, "device helper root")?;
    verify_required_directory(&release, "device helper release")?;
    let path = release.join(HELPER_NAME);
    let mut file = open_required_private_file(&path, EXECUTABLE_MODE, "device helper")?;
    let actual = hash_file(&mut file)?;
    if actual != helper_hash {
        return Err("registered device helper hash mismatch".into());
    }
    Ok(())
}

fn publish_helper(user_root: &Path, helper_hash: &str, helper_bytes: &[u8]) -> Result<(), String> {
    let helpers = user_root.join("helpers");
    let namespace = user_root.join(HELPER_DIRECTORY);
    ensure_private_directory(&helpers)?;
    ensure_private_directory(&namespace)?;
    let release = namespace.join(helper_hash);
    match fs::symlink_metadata(&release) {
        Ok(_) => return verify_unregistered_helper(&release, helper_hash, helper_bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }

    let staging = namespace.join(format!(
        ".staging-{helper_hash}-{}-{}",
        std::process::id(),
        super::unique_suffix()
    ));
    create_private_directory(&staging)?;
    let staged_helper = staging.join(HELPER_NAME);
    let result = (|| {
        super::write_durable(&staged_helper, helper_bytes)?;
        super::sync_dir(&staging)?;
        verify_directory(&staging, "staged device helper")?;
        super::rename_noreplace(&staging, &release)?;
        super::sync_dir(&namespace)?;
        verify_unregistered_helper(&release, helper_hash, helper_bytes)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn verify_unregistered_helper(
    release: &Path,
    helper_hash: &str,
    helper_bytes: &[u8],
) -> Result<(), String> {
    verify_directory(release, "device helper release")?;
    let path = release.join(HELPER_NAME);
    let mut file = open_required_private_file(&path, EXECUTABLE_MODE, "device helper")?;
    let body = read_file(&mut file, helper_bytes.len() as u64 + 1)?;
    if body != helper_bytes || super::sha256_hex(&body) != helper_hash {
        return Err("device helper release does not match its content address".into());
    }
    Ok(())
}

fn publish_registration(user_root: &Path, helper_hash: &str) -> Result<(), String> {
    let namespace = user_root.join(DEVICE_KEY_DIRECTORY);
    ensure_private_directory(&namespace)?;
    let path = namespace.join(REGISTRATION_NAME);
    let body = format!(
        "{}\n",
        serde_json::to_string_pretty(&serde_json::json!({
            "schemaVersion": 1,
            "protocolVersion": DEVICE_KEY_PROTOCOL_VERSION,
            "helperSha256": helper_hash,
        }))
        .map_err(|error| error.to_string())?
    );
    let temporary = namespace.join(format!(
        ".registration.json.tmp-{}-{}",
        std::process::id(),
        super::unique_suffix()
    ));
    let result = (|| {
        super::write_durable(&temporary, body.as_bytes())?;
        super::rename_noreplace(&temporary, &path)?;
        super::sync_dir(&namespace)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn ensure_private_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!("{} is a symlink", path.display()))
        }
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(format!("{} is not a directory", path.display()))
        }
        Ok(metadata) if metadata.uid() != current_uid() => {
            return Err(format!(
                "{} is not owned by the current user",
                path.display()
            ))
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            create_private_directory(path)?;
            return Ok(());
        }
        Err(error) => return Err(error.to_string()),
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.permissions().mode() & 0o777 != PRIVATE_DIRECTORY_MODE {
        let mut permissions = metadata.permissions();
        permissions.set_mode(PRIVATE_DIRECTORY_MODE);
        fs::set_permissions(path, permissions).map_err(|error| error.to_string())?;
        super::sync_dir(path)?;
    }
    verify_directory(path, "device helper directory")
}

fn create_private_directory(path: &Path) -> Result<(), String> {
    let mut builder = fs::DirBuilder::new();
    builder.mode(PRIVATE_DIRECTORY_MODE);
    builder.create(path).map_err(|error| error.to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_DIRECTORY_MODE))
        .map_err(|error| error.to_string())?;
    verify_directory(path, "new device helper directory")?;
    super::sync_dir(path)?;
    super::sync_dir(
        path.parent()
            .ok_or("device helper directory needs a parent")?,
    )
}

fn verify_required_directory(path: &Path, label: &str) -> Result<(), String> {
    if verify_optional_directory(path, label)? {
        Ok(())
    } else {
        Err(format!("{} is missing", path.display()))
    }
}

fn verify_optional_directory(path: &Path, label: &str) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            verify_directory(path, label)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

fn is_private_directory_entry(path: &Path, label: &str) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("{} is a symlink", path.display()))
        }
        Ok(metadata) if !metadata.file_type().is_dir() => {
            Err(format!("{} is not a directory", path.display()))
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("cannot inspect {label}: {error}")),
    }
}

fn verify_directory(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{} is a symlink", path.display()));
    }
    if !metadata.file_type().is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }
    let directory = super::open_directory(path)?;
    let metadata = directory.metadata().map_err(|error| error.to_string())?;
    if metadata.uid() != current_uid() {
        return Err(format!("{label} is not owned by the current user"));
    }
    if metadata.permissions().mode() & 0o777 != PRIVATE_DIRECTORY_MODE {
        return Err(format!("{label} mode is not 0700"));
    }
    Ok(())
}

fn open_optional_private_file(
    path: &Path,
    expected_mode: u32,
    label: &str,
) -> Result<Option<File>, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => open_required_private_file(path, expected_mode, label).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn open_required_private_file(
    path: &Path,
    expected_mode: u32,
    label: &str,
) -> Result<File, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{} is a symlink", path.display()));
    }
    if !metadata.file_type().is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let mut options = OpenOptions::new();
    options.read(true).custom_flags(libc::O_NOFOLLOW);
    let file = options.open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() {
        return Err(format!("{label} is not a regular file"));
    }
    if metadata.uid() != current_uid() {
        return Err(format!("{label} is not owned by the current user"));
    }
    if metadata.nlink() != 1 {
        return Err(format!("{label} has multiple links"));
    }
    if metadata.permissions().mode() & 0o777 != expected_mode {
        return Err(format!("{label} mode is not {expected_mode:04o}"));
    }
    Ok(file)
}

fn read_file(file: &mut File, max_bytes: u64) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    if body.len() as u64 > max_bytes {
        return Err("device helper is too large".into());
    }
    Ok(body)
}

fn hash_file(file: &mut File) -> Result<String, String> {
    let mut digest = Sha256::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&chunk[..read]);
    }
    Ok(hex_digest(digest.finalize()))
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn current_uid() -> u32 {
    unsafe { libc::geteuid() }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::fs;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::Value;

    static NEXT_SCRATCH: AtomicUsize = AtomicUsize::new(0);

    fn scratch() -> PathBuf {
        std::env::temp_dir().join(format!(
            "incodex-device-key-publisher-{}-{}-{}",
            std::process::id(),
            super::super::unique_suffix(),
            NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn helper_path(user_root: &Path, hash: &str) -> PathBuf {
        user_root
            .join("helpers/macos-device-key")
            .join(hash)
            .join("incodex-device-key-helper")
    }

    fn seed_registration(user_root: &Path, protocol: u64) -> (PathBuf, Vec<u8>) {
        let helper_bytes = b"test-only helper fixture; not a shipped executable".to_vec();
        let helper_hash = super::super::sha256_hex(&helper_bytes);
        let helper = helper_path(user_root, &helper_hash);
        fs::create_dir_all(helper.parent().unwrap()).unwrap();
        for directory in [
            user_root.join("helpers"),
            user_root.join("helpers/macos-device-key"),
            helper.parent().unwrap().to_path_buf(),
        ] {
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::write(&helper, &helper_bytes).unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();

        let registration_dir = user_root.join("macos-device-key");
        fs::create_dir_all(&registration_dir).unwrap();
        fs::set_permissions(&registration_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let registration_path = registration_dir.join("registration.json");
        fs::write(
            &registration_path,
            format!(
                "{{\"schemaVersion\":1,\"protocolVersion\":{protocol},\"helperSha256\":\"{helper_hash}\"}}\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&registration_path, fs::Permissions::from_mode(0o600)).unwrap();
        (registration_path, helper_bytes)
    }

    fn ensure_fixture_registration(user_root: &Path, helper_bytes: &[u8]) -> Result<(), String> {
        let _user_root_guard = super::super::ensure_user_root(user_root)?;
        let runtime = user_root.join("runtime");
        super::super::mkdir_mode(&runtime)?;
        let _lock = super::super::RuntimePublishLock::acquire(&runtime)?;
        super::ensure_registered(user_root, helper_bytes)
    }

    #[test]
    fn macos_publish_registers_a_private_content_addressed_device_helper() {
        let user_root = scratch();
        super::super::publish(&user_root).expect("Runtime publication must register its helper");

        let registration_path = user_root.join("macos-device-key/registration.json");
        let registration: Value = serde_json::from_slice(
            &fs::read(&registration_path).expect("device helper registration must exist"),
        )
        .expect("device helper registration must be JSON");
        let object = registration.as_object().expect("registration object");
        assert_eq!(object.len(), 3, "registration schema must stay strict");
        assert_eq!(registration["schemaVersion"], 1);
        assert_eq!(registration["protocolVersion"], 1);

        let helper_hash = registration["helperSha256"]
            .as_str()
            .expect("helperSha256 string");
        assert!(
            helper_hash.len() == 64
                && helper_hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "helperSha256 must be a lowercase SHA-256 digest"
        );

        let helper = user_root
            .join("helpers/macos-device-key")
            .join(helper_hash)
            .join("incodex-device-key-helper");
        let metadata = fs::symlink_metadata(&helper).expect("registered helper must exist");
        assert!(metadata.file_type().is_file(), "helper must be regular");
        assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
        assert_eq!(metadata.nlink(), 1, "helper must have a single link");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        assert_eq!(
            super::super::sha256_hex(&fs::read(&helper).unwrap()),
            helper_hash,
            "registered helper must match its content address"
        );
        for directory in [
            user_root.join("helpers"),
            user_root.join("helpers/macos-device-key"),
            helper.parent().unwrap().to_path_buf(),
            user_root.join("macos-device-key"),
        ] {
            let metadata = fs::symlink_metadata(directory).unwrap();
            assert!(metadata.file_type().is_dir());
            assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
            assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        }
        assert_eq!(
            fs::symlink_metadata(&registration_path).unwrap().nlink(),
            1,
            "registration must have a single link"
        );
        assert_eq!(fs::metadata(&registration_path).unwrap().uid(), unsafe {
            libc::geteuid()
        });
        assert_eq!(
            fs::metadata(&registration_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        fs::remove_dir_all(user_root).unwrap();
    }

    #[test]
    fn macos_publish_rejects_unknown_device_helper_protocol_before_pointer_commit() {
        let user_root = scratch();
        super::super::publish(&user_root).unwrap();
        let current = user_root.join("runtime/current.json");
        let current_before = fs::read(&current).unwrap();
        let (registration, _) = seed_registration(&user_root, 2);

        let error = super::super::publish(&user_root)
            .expect_err("unknown helper protocol must fail closed");

        assert!(
            error.contains("protocol") || error.contains("registration"),
            "{error}"
        );
        assert_eq!(fs::read(current).unwrap(), current_before);
        assert!(registration.is_file());
        fs::remove_dir_all(user_root).unwrap();
    }

    #[test]
    fn macos_ensure_current_rejects_a_damaged_device_helper_registration() {
        let user_root = scratch();
        super::super::publish(&user_root).unwrap();
        let current = user_root.join("runtime/current.json");
        let current_before = fs::read(&current).unwrap();
        let (registration, _) = seed_registration(&user_root, 1);
        fs::write(&registration, b"not json").unwrap();
        fs::set_permissions(&registration, fs::Permissions::from_mode(0o600)).unwrap();

        let error = super::super::ensure_current(&user_root)
            .expect_err("damaged registration must not pass the Runtime fast path");

        assert!(
            error.contains("registration") || error.contains("JSON"),
            "{error}"
        );
        assert_eq!(fs::read(current).unwrap(), current_before);
        fs::remove_dir_all(user_root).unwrap();
    }

    #[test]
    fn macos_publish_rejects_a_symlinked_device_registration_before_pointer_commit() {
        let user_root = scratch();
        super::super::publish(&user_root).unwrap();
        let current = user_root.join("runtime/current.json");
        let current_before = fs::read(&current).unwrap();
        let outside = user_root.join("outside-registration.json");
        fs::write(&outside, b"{}\n").unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
        let registration_dir = user_root.join("macos-device-key");
        fs::create_dir_all(&registration_dir).unwrap();
        fs::set_permissions(&registration_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let registration = registration_dir.join("registration.json");
        let _ = fs::remove_file(&registration);
        std::os::unix::fs::symlink(&outside, &registration).unwrap();

        let error = super::super::publish(&user_root)
            .expect_err("registration symlink must not be followed");

        assert!(error.contains("symlink"), "{error}");
        assert_eq!(fs::read(current).unwrap(), current_before);
        assert_eq!(fs::read(outside).unwrap(), b"{}\n");
        fs::remove_dir_all(user_root).unwrap();
    }

    #[test]
    fn macos_ensure_current_repairs_a_missing_device_registration() {
        let user_root = scratch();
        super::super::publish(&user_root).unwrap();
        let registration = user_root.join("macos-device-key/registration.json");
        let _ = fs::remove_file(&registration);

        super::super::ensure_current(&user_root).unwrap();

        assert!(
            registration.is_file(),
            "missing registration must be republished"
        );
        fs::remove_dir_all(user_root).unwrap();
    }

    #[test]
    fn macos_ensure_current_repairs_a_missing_registration_in_an_insecure_namespace() {
        let user_root = scratch();
        super::super::publish(&user_root).unwrap();
        let namespace = user_root.join("macos-device-key");
        fs::create_dir_all(&namespace).unwrap();
        let _ = fs::remove_file(namespace.join("registration.json"));
        fs::set_permissions(&namespace, fs::Permissions::from_mode(0o777)).unwrap();

        super::super::ensure_current(&user_root).unwrap();

        assert!(
            namespace.join("registration.json").is_file(),
            "missing registration must enter the publisher repair path"
        );
        assert_eq!(
            fs::metadata(&namespace).unwrap().permissions().mode() & 0o777,
            0o700,
            "the repair path must restore the namespace mode"
        );
        fs::remove_dir_all(user_root).unwrap();
    }

    #[test]
    fn device_key_publication_retains_valid_protocol_one_helper_across_candidate_bytes() {
        let user_root = scratch();
        let old_helper = b"test-only old device helper bytes";
        let candidate_helper = b"test-only new device helper bytes";
        ensure_fixture_registration(&user_root, old_helper).unwrap();
        let registration_path = user_root.join("macos-device-key/registration.json");
        let registration_before = fs::read(&registration_path).unwrap();
        let old_hash = super::super::sha256_hex(old_helper);
        let old_path = helper_path(&user_root, &old_hash);
        let candidate_hash = super::super::sha256_hex(candidate_helper);

        ensure_fixture_registration(&user_root, candidate_helper).unwrap();

        assert_eq!(fs::read(registration_path).unwrap(), registration_before);
        assert_eq!(fs::read(old_path).unwrap(), old_helper);
        assert!(
            !helper_path(&user_root, &candidate_hash).exists(),
            "a valid protocol-1 helper must never rotate with the publisher"
        );
        fs::remove_dir_all(user_root).unwrap();
    }

    #[test]
    fn device_key_publication_rejects_damaged_registration_without_changing_current() {
        let user_root = scratch();
        ensure_fixture_registration(&user_root, b"test-only device helper bytes").unwrap();
        let current = user_root.join("runtime/current.json");
        fs::write(&current, b"current-before\n").unwrap();
        fs::set_permissions(&current, fs::Permissions::from_mode(0o600)).unwrap();
        let current_before = fs::read(&current).unwrap();
        let registration = user_root.join("macos-device-key/registration.json");
        fs::write(&registration, b"not json\n").unwrap();
        fs::set_permissions(&registration, fs::Permissions::from_mode(0o600)).unwrap();

        let error = ensure_fixture_registration(&user_root, b"candidate bytes")
            .expect_err("damaged protocol state must fail closed");

        assert!(error.contains("registration"), "{error}");
        assert_eq!(fs::read(current).unwrap(), current_before);
        fs::remove_dir_all(user_root).unwrap();
    }

    #[test]
    fn device_key_publication_rejects_a_linked_helper_without_changing_current() {
        let user_root = scratch();
        let helper_bytes = b"test-only device helper bytes";
        ensure_fixture_registration(&user_root, helper_bytes).unwrap();
        let current = user_root.join("runtime/current.json");
        fs::write(&current, b"current-before\n").unwrap();
        fs::set_permissions(&current, fs::Permissions::from_mode(0o600)).unwrap();
        let current_before = fs::read(&current).unwrap();
        let helper_hash = super::super::sha256_hex(helper_bytes);
        let helper = helper_path(&user_root, &helper_hash);
        fs::remove_file(&helper).unwrap();
        let outside = user_root.join("outside-helper");
        fs::write(&outside, helper_bytes).unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o700)).unwrap();
        std::os::unix::fs::symlink(&outside, &helper).unwrap();

        let error = ensure_fixture_registration(&user_root, b"candidate bytes")
            .expect_err("registered helper links must be rejected");

        assert!(error.contains("symlink"), "{error}");
        assert_eq!(fs::read(current).unwrap(), current_before);
        assert_eq!(fs::read(outside).unwrap(), helper_bytes);
        fs::remove_dir_all(user_root).unwrap();
    }

    #[test]
    fn device_key_registration_and_helper_must_have_one_link() {
        let user_root = scratch();
        let helper_bytes = b"test-only device helper bytes";
        ensure_fixture_registration(&user_root, helper_bytes).unwrap();
        let helper_hash = super::super::sha256_hex(helper_bytes);
        let helper = helper_path(&user_root, &helper_hash);
        let helper_alias = user_root.join("helper-alias");
        fs::hard_link(&helper, &helper_alias).unwrap();

        let helper_error = ensure_fixture_registration(&user_root, b"candidate bytes")
            .expect_err("helper hard links must be rejected");
        assert!(helper_error.contains("multiple links"), "{helper_error}");
        fs::remove_file(helper_alias).unwrap();

        let registration = user_root.join("macos-device-key/registration.json");
        let registration_alias = user_root.join("registration-alias");
        fs::hard_link(&registration, &registration_alias).unwrap();
        let registration_error = ensure_fixture_registration(&user_root, b"candidate bytes")
            .expect_err("registration hard links must be rejected");
        assert!(
            registration_error.contains("multiple links"),
            "{registration_error}"
        );
        fs::remove_dir_all(user_root).unwrap();
    }
}
