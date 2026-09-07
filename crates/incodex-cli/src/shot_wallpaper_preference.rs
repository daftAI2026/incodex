//! [INPUT]: Incodex 私有根目录与用户明确获取当前桌面的动作。
//! [OUTPUT]: 跨隔离会话保存/读取一个启用标志，不持久化图片、绝对路径或聊天数据。
//! [POS]: Shot 本机偏好边界；目录 fd + no-follow 限定固定文件，不接受 renderer 路径。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use std::path::Path;
pub(crate) fn enabled(_root: &Path) -> bool { false }
pub(crate) fn remember(_root: &Path) -> Result<(), String> { Ok(()) }

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::{symlink, PermissionsExt}};
    #[test]
    fn survives_a_new_reader_and_contains_no_wallpaper_payload() {
        let root = std::env::temp_dir().join(format!("shot-pref-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!enabled(&root));
        remember(&root).unwrap();
        assert!(enabled(&root));
        assert_eq!(fs::read(root.join("shot-current-wallpaper-enabled")).unwrap(), b"1");
        remember(&root).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn refuses_symlink_root_and_marker() {
        let root = std::env::temp_dir().join(format!("shot-pref-link-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let alias = root.with_extension("alias");
        symlink(&root, &alias).unwrap();
        assert!(remember(&alias).is_err());
        fs::write(root.join("unrelated"), b"private").unwrap();
        symlink(root.join("unrelated"), root.join("shot-current-wallpaper-enabled")).unwrap();
        assert!(!enabled(&root));
        assert!(remember(&root).is_err());
        assert_eq!(fs::read(root.join("unrelated")).unwrap(), b"private");
        fs::remove_file(alias).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
