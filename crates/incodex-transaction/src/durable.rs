use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use std::cell::Cell;
#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::path::PathBuf;

const FILE_MODE: u32 = 0o600;
const DIR_MODE: u32 = 0o700;

#[derive(Debug)]
pub(crate) struct AtomicWriteError {
    pub(crate) message: String,
    pub(crate) renamed: bool,
}

impl AtomicWriteError {
    pub(crate) fn new(message: impl Into<String>, renamed: bool) -> Self {
        Self {
            message: message.into(),
            renamed,
        }
    }
}

impl std::fmt::Display for AtomicWriteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

#[cfg(test)]
thread_local! {
    static SYNC_TRACE: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    static FAIL_NEXT_WRITE_BEFORE_RENAME: Cell<bool> = const { Cell::new(false) };
    static FAIL_NEXT_WRITE_AFTER_RENAME: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn reset_sync_trace() {
    SYNC_TRACE.with(|trace| trace.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) fn sync_trace() -> Vec<PathBuf> {
    SYNC_TRACE.with(|trace| trace.borrow().clone())
}

#[cfg(test)]
pub(crate) fn fail_next_write_after_rename() {
    FAIL_NEXT_WRITE_AFTER_RENAME.with(|failure| failure.set(true));
}

#[cfg(test)]
pub(crate) fn fail_next_write_before_rename() {
    FAIL_NEXT_WRITE_BEFORE_RENAME.with(|failure| failure.set(true));
}

#[cfg(test)]
fn record_sync(path: &Path) {
    SYNC_TRACE.with(|trace| trace.borrow_mut().push(path.to_path_buf()));
}

#[cfg(not(test))]
fn record_sync(_path: &Path) {}

pub fn ensure_private_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    let mut perms = fs::metadata(dir)
        .map_err(|err| err.to_string())?
        .permissions();
    perms.set_mode(DIR_MODE);
    fs::set_permissions(dir, perms).map_err(|err| err.to_string())?;
    Ok(())
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_atomic_tracked(path, bytes).map_err(|error| error.message)
}

pub(crate) fn write_atomic_tracked(path: &Path, bytes: &[u8]) -> Result<(), AtomicWriteError> {
    let parent = path
        .parent()
        .ok_or_else(|| AtomicWriteError::new("durable write needs a parent directory", false))?;
    ensure_private_dir(parent).map_err(|error| AtomicWriteError::new(error, false))?;
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = parent.join(format!(".{}.tmp", n));
    {
        let mut opts = OpenOptions::new();
        opts.write(true)
            .create_new(true)
            .mode(FILE_MODE)
            .custom_flags(libc::O_NOFOLLOW);
        let mut file = opts
            .open(&tmp)
            .map_err(|err| AtomicWriteError::new(err.to_string(), false))?;
        file.write_all(bytes)
            .map_err(|err| AtomicWriteError::new(err.to_string(), false))?;
        file.sync_data()
            .map_err(|err| AtomicWriteError::new(err.to_string(), false))?;
        unsafe {
            libc::fchmod(file.as_raw_fd(), FILE_MODE as libc::mode_t);
        }
    }
    #[cfg(test)]
    if FAIL_NEXT_WRITE_BEFORE_RENAME.with(|failure| failure.replace(false)) {
        let _ = fs::remove_file(&tmp);
        return Err(AtomicWriteError::new(
            "injected pre-rename journal write failure",
            false,
        ));
    }
    fs::rename(&tmp, path).map_err(|err| AtomicWriteError::new(err.to_string(), false))?;
    #[cfg(test)]
    if FAIL_NEXT_WRITE_AFTER_RENAME.with(|failure| failure.replace(false)) {
        return Err(AtomicWriteError::new(
            "injected post-rename journal write failure",
            true,
        ));
    }
    sync_dir(parent).map_err(|error| AtomicWriteError::new(error, true))?;
    let mut perms = fs::metadata(path)
        .map_err(|err| AtomicWriteError::new(err.to_string(), true))?
        .permissions();
    perms.set_mode(FILE_MODE);
    fs::set_permissions(path, perms).map_err(|err| AtomicWriteError::new(err.to_string(), true))?;
    Ok(())
}

pub(crate) fn sync_dir(dir: &Path) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY);
    let file = options.open(dir).map_err(|err| err.to_string())?;
    file.sync_all().map_err(|err| err.to_string())?;
    record_sync(dir);
    Ok(())
}

pub(crate) fn sync_tree_and_ancestors(tree: &Path, boundary: &Path) -> Result<(), String> {
    if !tree.starts_with(boundary) || tree == boundary {
        return Err(format!(
            "durable tree {} is outside ancestor boundary {}",
            tree.display(),
            boundary.display()
        ));
    }
    sync_tree(tree)?;
    let mut current = tree
        .parent()
        .ok_or_else(|| format!("durable tree has no parent: {}", tree.display()))?;
    loop {
        sync_dir(current)
            .map_err(|error| format!("cannot flush directory {}: {error}", current.display()))?;
        if current == boundary {
            return Ok(());
        }
        current = current.parent().ok_or_else(|| {
            format!(
                "durable tree {} escaped ancestor boundary {}",
                tree.display(),
                boundary.display()
            )
        })?;
    }
}

fn sync_tree(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(format!(
            "durable tree root is not a real directory: {}",
            path.display()
        ));
    }
    let mut children = fs::read_dir(path)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    children.sort_by_key(|entry| entry.file_name());
    for entry in children {
        let child = entry.path();
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            sync_tree(&child)?;
            continue;
        }
        if !file_type.is_file() {
            return Err(format!(
                "unsupported durable backup entry type: {}",
                child.display()
            ));
        }
        let mut options = OpenOptions::new();
        options.read(true).custom_flags(libc::O_NOFOLLOW);
        let file = options.open(&child).map_err(|error| {
            format!(
                "cannot open backup file {} for flush: {error}",
                child.display()
            )
        })?;
        file.sync_all()
            .map_err(|error| format!("cannot flush backup file {}: {error}", child.display()))?;
        record_sync(&child);
    }
    sync_dir(path).map_err(|error| format!("cannot flush directory {}: {error}", path.display()))
}
