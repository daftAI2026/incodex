/**
 * [INPUT]: 接收 open 命令给出的临时名称与本地头像路径
 * [OUTPUT]: 对外提供 ProfileMask/ProfileAvatar、随机名称与安全本地图片校验
 * [POS]: incodex-cli 的隐私身份值对象；在 session 创建前完成离线解析，默认头像交给 Runtime Blobatar
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::fs::{self, File};
// RED: implementation follows in the next commit.
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("incodex-profile-mask-{now}-{sequence}"));
        fs::create_dir_all(&root).expect("profile mask temp root");
        root
    }

    fn write_avatar(root: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = root.join(name);
        fs::write(&path, bytes).expect("avatar");
        path
    }

    #[test]
    fn generated_avatar_is_deterministic_from_the_final_name() {
        let first = resolve_profile_mask(true, Some("Temporary"), None)
            .unwrap()
            .expect("mask");
        let second = resolve_profile_mask(true, Some("Temporary"), None)
            .unwrap()
            .expect("mask");
        let other = resolve_profile_mask(true, Some("Another"), None)
            .unwrap()
            .expect("mask");

        assert_eq!(first.name, "Temporary");
        assert_eq!(
            first.avatar,
            ProfileAvatar::Generated {
                seed: "Temporary".into()
            }
        );
        assert_eq!(first.avatar, second.avatar);
        assert_ne!(first.avatar, other.avatar);
    }

    #[test]
    fn default_mask_has_a_temporary_name_and_uses_that_name_for_the_avatar() {
        let generated = resolve_profile_mask(true, None, None)
            .unwrap()
            .expect("mask");
        let replayed = resolve_profile_mask(true, Some(&generated.name), None)
            .unwrap()
            .expect("mask");

        assert!(generated.name.starts_with("Incognito "));
        assert_eq!(
            generated.avatar,
            ProfileAvatar::Generated {
                seed: generated.name.clone()
            }
        );
        assert_eq!(generated.avatar, replayed.avatar);
    }

    #[test]
    fn local_png_jpeg_and_webp_are_converted_to_safe_data_urls() {
        let root = temp_root();
        let png = write_avatar(&root, "avatar.png", b"\x89PNG\r\n\x1a\nfixture");
        let jpeg = write_avatar(&root, "avatar.jpeg", b"\xff\xd8\xff\xe0fixture");
        let webp = write_avatar(&root, "avatar.webp", b"RIFF\x08\x00\x00\x00WEBPfixture");

        for (path, mime) in [
            (png, "data:image/png;base64,"),
            (jpeg, "data:image/jpeg;base64,"),
            (webp, "data:image/webp;base64,"),
        ] {
            let mask = resolve_profile_mask(true, Some("Local"), Some(&path))
                .unwrap()
                .expect("mask");
            let expected = format!(
                "{mime}{}",
                base64_encode(fs::read(&path).unwrap().as_slice())
            );
            assert_eq!(mask.avatar, ProfileAvatar::DataUrl(expected));
        }
    }

    #[test]
    fn avatar_validation_rejects_non_images_and_oversized_files() {
        let root = temp_root();
        let text = write_avatar(&root, "avatar.txt", b"not an image");
        let huge = write_avatar(
            &root,
            "avatar.png",
            &vec![0_u8; MAX_AVATAR_BYTES as usize + 1],
        );

        let unsupported = resolve_profile_mask(true, Some("Local"), Some(&text)).unwrap_err();
        assert!(unsupported.contains("PNG, JPEG, or WebP"), "{unsupported}");
        let oversized = resolve_profile_mask(true, Some("Local"), Some(&huge)).unwrap_err();
        assert!(oversized.contains("too large"), "{oversized}");
    }

    #[test]
    fn profile_names_are_bounded_and_control_free() {
        for invalid in [
            "",
            "   ",
            "bad\nname",
            &"x".repeat(MAX_PROFILE_NAME_CHARS + 1),
        ] {
            let error = resolve_profile_mask(true, Some(invalid), None).unwrap_err();
            assert!(error.contains("name"), "{error}");
        }
    }

    #[test]
    fn no_mask_means_no_profile_mutation() {
        assert_eq!(resolve_profile_mask(false, None, None).unwrap(), None);
    }
}
