/**
 * [INPUT]: 依赖嵌入 adapter 清单、helper 的私有发布原语与 Runtime 锁。
 * [OUTPUT]: 发布独立内容寻址 adapter，不改 Runtime catalog 或 helper 登记。
 * [POS]: macOS 热资源边界；只在 Runtime 提交前发布不可变 adapter。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::fs;
use std::path::Path;

const ADAPTER_DIRECTORY: &str = "helpers/macos-device-key-adapter";
const MANIFEST_NAME: &str = "manifest.json";
const MAX_ADAPTER_BYTES: u64 = 64 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// 调用方必须持有 Runtime 发布锁。
pub(super) fn ensure_published(user_root: &Path) -> Result<(), String> {
    let adapter = super::native::device_key_adapter_bytes();
    let manifest = super::native::device_key_adapter_manifest_bytes();
    let adapter_hash = super::sha256_hex(adapter);
    let manifest_hash = super::sha256_hex(manifest);
    let helpers = user_root.join("helpers");
    let namespace = user_root.join(ADAPTER_DIRECTORY);

    super::device_key::ensure_private_directory(&helpers)?;
    super::device_key::ensure_private_directory(&namespace)?;
    let release = namespace.join(&adapter_hash);
    match fs::symlink_metadata(&release) {
        Ok(_) => return verify_release(&release, &adapter_hash, &manifest_hash),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }

    let staging = namespace.join(format!(
        ".staging-{adapter_hash}-{}-{}",
        std::process::id(),
        super::unique_suffix()
    ));
    super::device_key::create_private_directory(&staging)?;
    let result = (|| {
        super::write_durable(
            &staging.join(super::native::DEVICE_KEY_ADAPTER_NAME),
            adapter,
        )?;
        super::write_durable(&staging.join(MANIFEST_NAME), manifest)?;
        super::sync_dir(&staging)?;
        verify_release(&staging, &adapter_hash, &manifest_hash)?;
        super::rename_noreplace(&staging, &release)?;
        super::sync_dir(&namespace)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn verify_release(release: &Path, adapter_hash: &str, manifest_hash: &str) -> Result<(), String> {
    super::device_key::verify_directory(release, "device key adapter release")?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(release).map_err(|error| error.to_string())? {
        entries.push(entry.map_err(|error| error.to_string())?.file_name());
    }
    if entries.len() != 2
        || !entries
            .iter()
            .any(|name| name == super::native::DEVICE_KEY_ADAPTER_NAME)
        || !entries.iter().any(|name| name == MANIFEST_NAME)
    {
        return Err("device key adapter release contains unexpected files".into());
    }

    let adapter_path = release.join(super::native::DEVICE_KEY_ADAPTER_NAME);
    let mut adapter = super::device_key::open_required_private_file(
        &adapter_path,
        super::device_key::PRIVATE_FILE_MODE,
        "device key adapter",
    )?;
    if adapter.metadata().map_err(|error| error.to_string())?.len() > MAX_ADAPTER_BYTES
        || super::device_key::hash_file(&mut adapter)? != adapter_hash
    {
        return Err("device key adapter bytes do not match their content address".into());
    }

    let manifest_path = release.join(MANIFEST_NAME);
    let mut manifest = super::device_key::open_required_private_file(
        &manifest_path,
        super::device_key::PRIVATE_FILE_MODE,
        "device key adapter manifest",
    )?;
    if manifest
        .metadata()
        .map_err(|error| error.to_string())?
        .len()
        > MAX_MANIFEST_BYTES
        || super::device_key::hash_file(&mut manifest)? != manifest_hash
    {
        return Err("device key adapter manifest does not match its embedded pin".into());
    }
    Ok(())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn publisher_refuses_to_repair_a_damaged_content_address() {
        let root = std::env::temp_dir().join(format!(
            "incodex-device-key-adapter-damaged-{}-{}",
            std::process::id(),
            super::super::unique_suffix()
        ));
        let _user_root = super::super::ensure_user_root(&root).unwrap();
        let namespace = root.join(ADAPTER_DIRECTORY);
        super::super::device_key::ensure_private_directory(namespace.parent().unwrap()).unwrap();
        super::super::device_key::ensure_private_directory(&namespace).unwrap();
        let hash = super::super::sha256_hex(super::super::native::device_key_adapter_bytes());
        let release = namespace.join(hash);
        super::super::device_key::create_private_directory(&release).unwrap();
        let adapter = release.join(super::super::native::DEVICE_KEY_ADAPTER_NAME);
        fs::write(&adapter, b"damaged adapter").unwrap();
        fs::set_permissions(&adapter, fs::Permissions::from_mode(0o600)).unwrap();
        let manifest = release.join(MANIFEST_NAME);
        fs::write(&manifest, b"damaged manifest").unwrap();
        fs::set_permissions(&manifest, fs::Permissions::from_mode(0o600)).unwrap();

        let error = ensure_published(&root).expect_err("damaged immutable release must fail");

        assert!(error.contains("content address"), "{error}");
        assert_eq!(fs::read(adapter).unwrap(), b"damaged adapter");
        fs::remove_dir_all(root).unwrap();
    }
}
