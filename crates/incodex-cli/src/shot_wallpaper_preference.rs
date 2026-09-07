//! [INPUT]: Incodex 私有根目录与用户明确获取当前桌面的动作。
//! [OUTPUT]: 跨隔离会话保存/读取一个启用标志，不持久化图片、绝对路径或聊天数据。
//! [POS]: Shot 本机偏好边界；目录 fd + no-follow 限定固定文件，不接受 renderer 路径。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use std::path::Path;
pub(crate) fn enabled(root: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        platform::read(root).unwrap_or(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = root;
        false
    }
}
pub(crate) fn remember(root: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        platform::write(root).map_err(|error| error.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = root;
        Err("current wallpaper persistence is unavailable".into())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::{
        fs::{File, OpenOptions},
        io::{self, Read, Write},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::{MetadataExt, OpenOptionsExt},
        },
        path::Path,
    };
    const NAME: &std::ffi::CStr = c"shot-current-wallpaper-enabled";
    fn private_root(root: &Path) -> io::Result<File> {
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(root)?;
        let meta = directory.metadata()?;
        if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
            return Err(io::Error::other("Shot preference root is not private"));
        }
        Ok(directory)
    }
    fn open(directory: &File, flags: i32) -> io::Result<File> {
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                NAME.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    fn read_at(directory: &File) -> io::Result<bool> {
        let mut file = match open(directory, libc::O_RDONLY) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        let meta = file.metadata()?;
        if !meta.is_file()
            || meta.uid() != unsafe { libc::geteuid() }
            || meta.nlink() != 1
            || meta.mode() & 0o077 != 0
            || meta.len() != 1
        {
            return Err(io::Error::other("invalid Shot preference marker"));
        }
        let mut byte = [0];
        file.read_exact(&mut byte)?;
        Ok(byte == *b"1")
    }
    pub(super) fn read(root: &Path) -> io::Result<bool> {
        read_at(&private_root(root)?)
    }
    pub(super) fn write(root: &Path) -> io::Result<()> {
        let directory = private_root(root)?;
        if read_at(&directory)? {
            return Ok(());
        }
        let mut file = match open(&directory, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && read_at(&directory)? => {
                return Ok(())
            }
            Err(error) => return Err(error),
        };
        file.write_all(b"1")?;
        file.sync_all()?;
        directory.sync_all()
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{symlink, PermissionsExt},
    };
    #[test]
    fn survives_a_new_reader_and_contains_no_wallpaper_payload() {
        let root = std::env::temp_dir().join(format!("shot-pref-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!enabled(&root));
        remember(&root).unwrap();
        assert!(enabled(&root));
        assert_eq!(
            fs::read(root.join("shot-current-wallpaper-enabled")).unwrap(),
            b"1"
        );
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
        symlink(
            root.join("unrelated"),
            root.join("shot-current-wallpaper-enabled"),
        )
        .unwrap();
        assert!(!enabled(&root));
        assert!(remember(&root).is_err());
        assert_eq!(fs::read(root.join("unrelated")).unwrap(), b"private");
        fs::remove_file(alias).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
