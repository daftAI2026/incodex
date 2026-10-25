/**
 * [INPUT]: 依赖 Runtime publisher 与隔离临时目录。
 * [OUTPUT]: 约束独立设备 helper 登记的发布契约。
 * [POS]: Runtime publisher 的设备密钥回归边界，不操作真实用户根目录。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::fs;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use serde_json::Value;

    fn scratch() -> PathBuf {
        std::env::temp_dir().join(format!(
            "incodex-device-key-publisher-{}-{}",
            std::process::id(),
            super::super::unique_suffix()
        ))
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
        assert_eq!(
            fs::metadata(user_root.join("macos-device-key"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
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
}
