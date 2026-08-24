/**
 * [INPUT]: 接收 open 命令给出的临时名称与本地头像路径
 * [OUTPUT]: 对外提供 ProfileMask/ProfileAvatar、随机名称与安全本地图片校验
 * [POS]: incodex-cli 的隐私身份值对象；在 session 创建前完成离线解析，默认头像交给 Runtime Blobatar
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

pub const MAX_AVATAR_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_PROFILE_NAME_CHARS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileAvatar {
    Generated { seed: String },
    DataUrl(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileMask {
    pub name: String,
    pub avatar: ProfileAvatar,
}

static NAME_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn resolve_profile_mask(
    mask: bool,
    name: Option<&str>,
    avatar_path: Option<&Path>,
) -> Result<Option<ProfileMask>, String> {
    if !mask {
        if name.is_some() {
            return Err("--name requires --mask".into());
        }
        if avatar_path.is_some() {
            return Err("--avatar requires --mask".into());
        }
        return Ok(None);
    }

    let name = match name {
        Some(value) => validate_profile_name(value)?,
        None => random_profile_name(),
    };
    let avatar = match avatar_path {
        Some(path) => ProfileAvatar::DataUrl(read_avatar_data_url(path)?),
        None => ProfileAvatar::Generated { seed: name.clone() },
    };
    Ok(Some(ProfileMask { name, avatar }))
}

fn validate_profile_name(value: &str) -> Result<String, String> {
    let name = value.trim();
    if name.is_empty() {
        return Err("profile name must not be empty".into());
    }
    if name.chars().count() > MAX_PROFILE_NAME_CHARS {
        return Err(format!(
            "profile name must be at most {MAX_PROFILE_NAME_CHARS} characters"
        ));
    }
    if name.chars().any(char::is_control) {
        return Err("profile name must not contain control characters".into());
    }
    Ok(name.to_string())
}

fn random_profile_name() -> String {
    let sequence = NAME_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut entropy = [0_u8; 32];
    let random_ok = File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut entropy))
        .is_ok();
    if !random_ok {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        entropy[..16].copy_from_slice(&now.to_le_bytes());
        entropy[16..24].copy_from_slice(&(std::process::id() as u64).to_le_bytes());
        entropy[24..].copy_from_slice(&sequence.to_le_bytes());
    } else {
        entropy[..8]
            .iter_mut()
            .zip(sequence.to_le_bytes())
            .for_each(|(byte, extra)| {
                *byte ^= extra;
            });
    }
    let digest = Sha256::digest(entropy);
    format!("Incognito {}", hex_encode(&digest[..6]))
}

fn read_avatar_data_url(path: &Path) -> Result<String, String> {
    let link_metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot read avatar file {}: {error}", path.display()))?;
    if !link_metadata.file_type().is_file() {
        return Err(format!(
            "avatar path is not an ordinary file: {}",
            path.display()
        ));
    }

    let file = File::open(path)
        .map_err(|error| format!("cannot open avatar file {}: {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot stat avatar file {}: {error}", path.display()))?;
    if metadata.len() > MAX_AVATAR_BYTES {
        return Err(format!(
            "avatar file is too large (maximum {MAX_AVATAR_BYTES} bytes)"
        ));
    }

    let mut bytes = Vec::new();
    file.take(MAX_AVATAR_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read avatar file {}: {error}", path.display()))?;
    if bytes.len() as u64 > MAX_AVATAR_BYTES {
        return Err(format!(
            "avatar file is too large (maximum {MAX_AVATAR_BYTES} bytes)"
        ));
    }
    let mime = avatar_mime(&bytes).ok_or_else(|| {
        "avatar must be a PNG, JPEG, or WebP file with a recognized signature".to_string()
    })?;
    Ok(format!("data:{mime};base64,{}", base64_encode(&bytes)))
}

fn avatar_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        out.push(TABLE[(a >> 2) as usize] as char);
        out.push(TABLE[(((a & 0x03) << 4) | (b >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(((b & 0x0f) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(c & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

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
