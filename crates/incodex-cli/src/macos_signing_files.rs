/*
 * [INPUT]: 依赖 macOS 文件系统权限、O_NOFOLLOW 与文件锁边界，只处理本机身份私有资产。
 * [OUTPUT]: 提供限长安全文件读写、原子首次发布、私有目录校验与 staging 清理。
 * [POS]: macOS signing assets 的窄文件安全层；不解释身份 metadata、不调用 Security 或 codesign。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

pub(super) struct StagingDirectory(pub(super) PathBuf);

impl Drop for StagingDirectory {
    fn drop(&mut self) {
        // 这里只负责删除私有 staging，不承诺 APFS 上的物理擦除。
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn write_new_private_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .mode(super::PRIVATE_FILE_MODE)
        .custom_flags(libc::O_NOFOLLOW);
    let mut file = options
        .open(path)
        .map_err(|error| format!("cannot create private signing asset: {error}"))?;
    file.write_all(bytes)
        .map_err(|error| format!("cannot write private signing asset: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("cannot flush private signing asset: {error}"))?;
    set_file_mode(&file, super::PRIVATE_FILE_MODE)
}

pub(super) fn create_empty_private_file(path: &Path) -> Result<(), String> {
    write_new_private_file(path, b"")
}

pub(super) fn write_new_private_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("signing registration has no containing directory")?;
    let sequence = super::STAGING_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let temporary = parent.join(format!(
        ".registration-{}-{timestamp}-{sequence}.tmp",
        std::process::id()
    ));
    let result = (|| {
        write_new_private_file(&temporary, bytes)?;
        fs::hard_link(&temporary, path)
            .map_err(|error| format!("cannot publish signing identity registration: {error}"))?;
        fs::remove_file(&temporary)
            .map_err(|error| format!("cannot finalize signing identity registration: {error}"))?;
        sync_parent(path)
    })();
    let _ = fs::remove_file(&temporary);
    result
}

pub(super) fn read_private_file(
    path: &Path,
    minimum: usize,
    maximum: usize,
    label: &str,
) -> Result<Option<Vec<u8>>, String> {
    read_regular_file_at_mode(path, minimum, maximum, super::PRIVATE_FILE_MODE, label)
}

pub(super) fn read_regular_file_at_mode(
    path: &Path,
    minimum: usize,
    maximum: usize,
    expected_mode: u32,
    label: &str,
) -> Result<Option<Vec<u8>>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot inspect {label}: {error}")),
    };
    validate_file_metadata(
        &metadata,
        minimum as u64,
        maximum as u64,
        expected_mode,
        label,
    )?;
    let mut options = OpenOptions::new();
    options.read(true).custom_flags(libc::O_NOFOLLOW);
    let file = options
        .open(path)
        .map_err(|error| format!("cannot open {label}: {error}"))?;
    let opened = file
        .metadata()
        .map_err(|error| format!("cannot inspect opened {label}: {error}"))?;
    validate_file_metadata(
        &opened,
        minimum as u64,
        maximum as u64,
        expected_mode,
        label,
    )?;
    if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
        return Err(format!("{label} changed during read"));
    }
    let mut bytes = Vec::with_capacity(maximum.min(opened.len() as usize));
    file.take(maximum.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {label}: {error}"))?;
    if bytes.len() > maximum || bytes.len() < minimum || bytes.len() as u64 != opened.len() {
        return Err(format!("{label} has an invalid size"));
    }
    Ok(Some(bytes))
}

pub(super) fn validate_private_file(
    path: &Path,
    minimum: u64,
    maximum: u64,
    label: &str,
) -> Result<(), String> {
    validate_file_metadata_at_mode(path, minimum, maximum, super::PRIVATE_FILE_MODE, label)
}

pub(super) fn validate_file_metadata_at_mode(
    path: &Path,
    minimum: u64,
    maximum: u64,
    mode: u32,
    label: &str,
) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("cannot inspect {label}: {error}"))?;
    validate_file_metadata(&metadata, minimum, maximum, mode, label)
}

fn validate_file_metadata(
    metadata: &fs::Metadata,
    minimum: u64,
    maximum: u64,
    expected_mode: u32,
    label: &str,
) -> Result<(), String> {
    if !metadata.file_type().is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o777 != expected_mode
        || metadata.nlink() != 1
        || metadata.len() < minimum
        || metadata.len() > maximum
    {
        return Err(format!(
            "{label} is not a private current-user regular file"
        ));
    }
    Ok(())
}

pub(super) fn ensure_owned_private_directory(path: &Path, create: bool) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!(
                "refuse symlink private directory: {}",
                path.display()
            ));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(format!("expected private directory: {}", path.display()));
        }
        Ok(metadata) if metadata.uid() != unsafe { libc::geteuid() } => {
            return Err(format!(
                "private directory has another owner: {}",
                path.display()
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
            fs::create_dir_all(path)
                .map_err(|error| format!("cannot create private directory: {error}"))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("private directory is missing: {}", path.display()));
        }
        Err(error) => return Err(format!("cannot inspect private directory: {error}")),
    }
    set_directory_mode(path, super::PRIVATE_DIRECTORY_MODE)?;
    validate_private_directory(path)
}

pub(super) fn validate_private_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect private directory: {error}"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o777 != super::PRIVATE_DIRECTORY_MODE
    {
        return Err(format!(
            "directory is not private to the current user: {}",
            path.display()
        ));
    }
    Ok(())
}

pub(super) fn set_directory_mode(path: &Path, mode: u32) -> Result<(), String> {
    let mut permissions = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect directory permissions: {error}"))?
        .permissions();
    permissions.set_mode(mode);
    fs::set_permissions(path, permissions)
        .map_err(|error| format!("cannot protect private directory: {error}"))
}

fn set_file_mode(file: &File, mode: u32) -> Result<(), String> {
    let result = unsafe { libc::fchmod(file.as_raw_fd(), mode as libc::mode_t) };
    if result == 0 {
        Ok(())
    } else {
        Err("cannot protect private signing asset".into())
    }
}

pub(super) fn validate_root_if_present(root: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(format!(
            "Incodex storage root is not a real directory: {}",
            root.display()
        )),
        Ok(metadata)
            if metadata.uid() != unsafe { libc::geteuid() }
                || metadata.permissions().mode() & 0o777 != super::PRIVATE_DIRECTORY_MODE =>
        {
            Err("Incodex storage root is not private to the current user".into())
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("cannot inspect Incodex storage root: {error}")),
    }
}

pub(super) fn sync_parent(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("private file has no containing directory")?;
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(parent)
        .map_err(|error| format!("cannot open private directory for sync: {error}"))?;
    directory
        .sync_all()
        .map_err(|error| format!("cannot sync private directory: {error}"))
}

pub(super) fn lstat_exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("cannot inspect signing identity state: {error}")),
    }
}
