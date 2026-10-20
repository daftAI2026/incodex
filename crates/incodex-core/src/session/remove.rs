// ============================================================================
// remove: 固定 session 目录对象后，仅为已验证目录补齐 owner rwx
// ============================================================================
// [INPUT]: 依赖 Unix openat/fstatat、session root 与 userRoot/sessions 双层 inode
// [OUTPUT]: 递归增加 session 子树目录 owner rwx；不改文件或跟随 symlink
// [POS]: session 销毁前的窄权限适配器；session.rs 负责业务身份与最终删除
// [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use std::ffi::{CStr, CString, OsString};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::{assert_inside_parent, assert_not_symlink, real_existing};

const DIRECTORY_FLAGS: i32 =
    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;

pub(super) fn ensure_owner_writable_tree(
    root_path: &Path,
    sessions: &SessionsParent,
    expected_ino: u64,
    expected_dev: u64,
) -> Result<(), String> {
    ensure_owner_writable_tree_inner(root_path, sessions, expected_ino, expected_dev, || {})
}

#[cfg(test)]
pub(super) fn ensure_owner_writable_tree_with_hook<F>(
    root_path: &Path,
    sessions_root: &Path,
    expected_ino: u64,
    expected_dev: u64,
    before_walk: F,
) -> Result<(), String>
where
    F: FnOnce(),
{
    let sessions_path = sessions_root
        .parent()
        .ok_or_else(|| "sessions directory has no user-root parent".to_string())?;
    let sessions = validated_sessions_parent(sessions_path)?;
    if sessions.canonical_sessions != sessions_root {
        return Err("sessions parent changed before permission repair".into());
    }
    ensure_owner_writable_tree_inner(
        root_path,
        &sessions,
        expected_ino,
        expected_dev,
        before_walk,
    )
}

fn ensure_owner_writable_tree_inner<F>(
    root_path: &Path,
    sessions: &SessionsParent,
    expected_ino: u64,
    expected_dev: u64,
    before_walk: F,
) -> Result<(), String>
where
    F: FnOnce(),
{
    sessions.verify()?;
    let root_real = fs::canonicalize(root_path).map_err(|error| error.to_string())?;
    assert_inside_parent(&root_real, &sessions.canonical_sessions)?;

    let root_fd = open_directory(root_path)?;
    let root = RootBinding {
        fd: &root_fd,
        path: root_path,
        real_path: &root_real,
        sessions,
        dev: expected_dev,
        ino: expected_ino,
        uid: unsafe { libc::geteuid() },
    };
    root.verify()?;
    before_walk();
    root.verify()?;
    walk_directory(&root_fd, root_path, &root)?;
    root.verify()?;
    Ok(())
}

struct RootBinding<'a> {
    fd: &'a File,
    path: &'a Path,
    real_path: &'a Path,
    sessions: &'a SessionsParent,
    dev: u64,
    ino: u64,
    uid: libc::uid_t,
}

impl RootBinding<'_> {
    fn verify(&self) -> Result<(), String> {
        self.sessions.verify()?;
        let metadata = self.verify_directory(self.fd, self.path)?;
        if metadata.dev() != self.dev || metadata.ino() != self.ino {
            return Err("session root identity changed during permission repair".into());
        }
        let current_real = fs::canonicalize(self.path).map_err(|error| error.to_string())?;
        if current_real != self.real_path {
            return Err("session root path changed during permission repair".into());
        }
        Ok(())
    }

    fn verify_directory(&self, directory: &File, path: &Path) -> Result<fs::Metadata, String> {
        let opened = directory.metadata().map_err(|error| error.to_string())?;
        let path_stat = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        if path_stat.file_type().is_symlink() || !path_stat.is_dir() {
            return Err(format!(
                "session directory changed or became a symlink: {}",
                path.display()
            ));
        }
        if !same_identity(&opened, &path_stat) {
            return Err(format!(
                "session directory path no longer names opened object: {}",
                path.display()
            ));
        }
        if opened.dev() != self.dev {
            return Err(format!(
                "session directory crosses device boundary: {}",
                path.display()
            ));
        }
        if opened.uid() != self.uid {
            return Err(format!(
                "session directory owner changed: {}",
                path.display()
            ));
        }
        let real_path = fs::canonicalize(path).map_err(|error| error.to_string())?;
        assert_inside_parent(&real_path, self.real_path)?;
        assert_inside_parent(&real_path, &self.sessions.canonical_sessions)?;
        Ok(opened)
    }
}

pub(super) struct SessionsParent {
    user_root: PathBuf,
    user_root_real: PathBuf,
    user_root_dev: u64,
    user_root_ino: u64,
    sessions_path: PathBuf,
    canonical_sessions: PathBuf,
    sessions_dev: u64,
    sessions_ino: u64,
}

pub(super) fn validated_sessions_parent(user_root: &Path) -> Result<SessionsParent, String> {
    let user_stats = assert_not_symlink(user_root, "session user root")?
        .ok_or_else(|| format!("session user root missing: {}", user_root.display()))?;
    if !user_stats.is_dir() {
        return Err(format!(
            "session user root is not a directory: {}",
            user_root.display()
        ));
    }

    let sessions_path = user_root.join(super::SESSIONS_NAME);
    let sessions_stats = assert_not_symlink(&sessions_path, "sessions directory")?
        .ok_or_else(|| format!("sessions directory missing: {}", sessions_path.display()))?;
    if !sessions_stats.is_dir() {
        return Err(format!(
            "sessions path is not a directory: {}",
            sessions_path.display()
        ));
    }

    let parent = SessionsParent {
        user_root: user_root.to_path_buf(),
        user_root_real: real_existing(user_root)?,
        user_root_dev: user_stats.dev(),
        user_root_ino: user_stats.ino(),
        canonical_sessions: real_existing(&sessions_path)?,
        sessions_path,
        sessions_dev: sessions_stats.dev(),
        sessions_ino: sessions_stats.ino(),
    };
    assert_inside_parent(&parent.canonical_sessions, &parent.user_root_real)?;
    parent.verify()?;
    Ok(parent)
}

impl SessionsParent {
    pub(super) fn canonical_path(&self) -> &Path {
        &self.canonical_sessions
    }

    pub(super) fn verify(&self) -> Result<(), String> {
        let user_stats = assert_not_symlink(&self.user_root, "session user root")?
            .ok_or_else(|| format!("session user root missing: {}", self.user_root.display()))?;
        if !user_stats.is_dir()
            || user_stats.dev() != self.user_root_dev
            || user_stats.ino() != self.user_root_ino
        {
            return Err("session user root identity changed during burn".into());
        }

        let sessions_stats = assert_not_symlink(&self.sessions_path, "sessions directory")?
            .ok_or_else(|| {
                format!(
                    "sessions directory missing: {}",
                    self.sessions_path.display()
                )
            })?;
        if !sessions_stats.is_dir()
            || sessions_stats.dev() != self.sessions_dev
            || sessions_stats.ino() != self.sessions_ino
        {
            return Err("sessions directory identity changed during burn".into());
        }

        let user_root_real = real_existing(&self.user_root)?;
        let sessions_real = real_existing(&self.sessions_path)?;
        if user_root_real != self.user_root_real || sessions_real != self.canonical_sessions {
            return Err("session ancestry changed during burn".into());
        }
        assert_inside_parent(&sessions_real, &user_root_real)
    }
}

fn walk_directory(
    directory: &File,
    directory_path: &Path,
    root: &RootBinding<'_>,
) -> Result<(), String> {
    ensure_directory_owner_rwx(directory, directory_path, root)?;

    for name in directory_names(directory)? {
        if name.as_bytes() == b"." || name.as_bytes() == b".." {
            continue;
        }
        let c_name = CString::new(name.as_bytes()).map_err(|_| {
            format!(
                "invalid session entry name under {}",
                directory_path.display()
            )
        })?;
        let entry_stat = stat_at(directory, &c_name, directory_path)?;
        if entry_stat.st_mode & libc::S_IFMT != libc::S_IFDIR {
            continue;
        }
        if entry_stat.st_dev as u64 != root.dev {
            return Err(format!(
                "session directory crosses device boundary: {}",
                directory_path.join(&name).display()
            ));
        }
        if entry_stat.st_uid != root.uid {
            return Err(format!(
                "session directory owner changed: {}",
                directory_path.join(&name).display()
            ));
        }

        root.verify()?;
        let child = open_directory_at(directory, &c_name, directory_path)?;
        let child_path = directory_path.join(&name);
        let child_meta = root.verify_directory(&child, &child_path)?;
        if child_meta.dev() != entry_stat.st_dev as u64
            || child_meta.ino() != entry_stat.st_ino
            || child_meta.uid() != entry_stat.st_uid
        {
            return Err(format!(
                "session directory changed while opening: {}",
                child_path.display()
            ));
        }
        walk_directory(&child, &child_path, root)?;
    }
    Ok(())
}

fn ensure_directory_owner_rwx(
    directory: &File,
    directory_path: &Path,
    root: &RootBinding<'_>,
) -> Result<(), String> {
    let metadata = root.verify_directory(directory, directory_path)?;
    if metadata.uid() != root.uid {
        return Err(format!(
            "session directory owner changed: {}",
            directory_path.display()
        ));
    }
    let current_mode = metadata.permissions().mode() & 0o7777;
    let owner_writable_mode = current_mode | 0o700;
    if current_mode == owner_writable_mode {
        return Ok(());
    }

    root.verify()?;
    let current = root.verify_directory(directory, directory_path)?;
    if current.uid() != root.uid {
        return Err(format!(
            "session directory owner changed before permission repair: {}",
            directory_path.display()
        ));
    }
    let mut permissions = current.permissions();
    permissions.set_mode(owner_writable_mode);
    directory
        .set_permissions(permissions)
        .map_err(|error| format!("cannot make session directory removable: {error}"))?;
    root.verify()?;
    root.verify_directory(directory, directory_path)?;
    Ok(())
}

fn same_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino() && left.uid() == right.uid()
}

fn open_directory(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true).custom_flags(DIRECTORY_FLAGS);
    options.open(path).map_err(|error| error.to_string())
}

fn open_directory_at(parent: &File, name: &CString, parent_path: &Path) -> Result<File, String> {
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), DIRECTORY_FLAGS) };
    if fd < 0 {
        return Err(format!(
            "cannot open session directory {}: {}",
            parent_path.display(),
            io::Error::last_os_error()
        ));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn stat_at(parent: &File, name: &CString, parent_path: &Path) -> Result<libc::stat, String> {
    let mut output = MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            output.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        return Err(format!(
            "cannot inspect session entry under {}: {}",
            parent_path.display(),
            io::Error::last_os_error()
        ));
    }
    Ok(unsafe { output.assume_init() })
}

fn directory_names(directory: &File) -> Result<Vec<OsString>, String> {
    let duplicated = unsafe { libc::fcntl(directory.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
    if duplicated < 0 {
        return Err(format!(
            "cannot duplicate session directory handle: {}",
            io::Error::last_os_error()
        ));
    }
    let stream = unsafe { libc::fdopendir(duplicated) };
    if stream.is_null() {
        let error = io::Error::last_os_error();
        unsafe { libc::close(duplicated) };
        return Err(format!("cannot enumerate session directory: {error}"));
    }
    let stream = DirectoryStream(stream);
    let mut names = Vec::new();
    loop {
        set_errno(0);
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            let error = errno();
            if error != 0 {
                return Err(format!(
                    "cannot enumerate session directory: {}",
                    io::Error::from_raw_os_error(error)
                ));
            }
            break;
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        names.push(OsString::from_vec(name.to_vec()));
    }
    Ok(names)
}

struct DirectoryStream(*mut libc::DIR);

impl Drop for DirectoryStream {
    fn drop(&mut self) {
        unsafe { libc::closedir(self.0) };
    }
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
fn errno_ptr() -> *mut libc::c_int {
    unsafe { libc::__error() }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn errno_ptr() -> *mut libc::c_int {
    unsafe { libc::__errno_location() }
}

fn set_errno(value: libc::c_int) {
    unsafe { *errno_ptr() = value };
}

fn errno() -> libc::c_int {
    unsafe { *errno_ptr() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_names_preserves_dot_entries_for_caller_filtering() {
        let root = std::env::temp_dir().join(format!(
            "incodex-session-remove-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("entry"), b"x").unwrap();
        let dir = open_directory(&root).unwrap();
        let names = directory_names(&dir).unwrap();
        assert_eq!(names.iter().filter(|name| *name == ".").count(), 1);
        assert_eq!(names.iter().filter(|name| *name == "..").count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
