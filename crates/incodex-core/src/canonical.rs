use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::paths::DEFAULT_APP;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalTarget {
    pub requested_path: PathBuf,
    pub real_path: PathBuf,
    pub parent_real_path: PathBuf,
    pub target_device: u64,
    pub target_inode: u64,
    pub parent_device: u64,
    pub parent_inode: u64,
    pub is_official: bool,
}

pub fn canonical_path(input: impl AsRef<Path>) -> PathBuf {
    let requested = absolute(input.as_ref());
    if let Ok(real) = fs::canonicalize(&requested) {
        return real;
    }
    if let Some(parent) = requested.parent() {
        if let Ok(real_parent) = fs::canonicalize(parent) {
            if let Some(name) = requested.file_name() {
                return real_parent.join(name);
            }
        }
    }
    requested
}

pub fn is_official_app(app_path: impl AsRef<Path>, official_path: Option<&Path>) -> bool {
    let official = official_path.unwrap_or_else(|| Path::new(DEFAULT_APP));
    canonical_path(app_path.as_ref()) == canonical_path(official)
}

pub fn inspect_target(
    requested: impl AsRef<Path>,
    official: Option<&Path>,
) -> Result<CanonicalTarget, String> {
    let requested_path = absolute(requested.as_ref());
    let real_path = fs::canonicalize(&requested_path).map_err(|err| err.to_string())?;
    let parent_real_path = real_path
        .parent()
        .ok_or("target has no parent")?
        .to_path_buf();
    let target_meta = fs::metadata(&real_path).map_err(|err| err.to_string())?;
    let parent_meta = fs::metadata(&parent_real_path).map_err(|err| err.to_string())?;
    let official_real = canonical_path(official.unwrap_or_else(|| Path::new(DEFAULT_APP)));
    Ok(CanonicalTarget {
        requested_path,
        is_official: real_path == official_real,
        target_device: target_meta.dev(),
        target_inode: target_meta.ino(),
        parent_device: parent_meta.dev(),
        parent_inode: parent_meta.ino(),
        real_path,
        parent_real_path,
    })
}

pub fn recheck_target(expected: &CanonicalTarget) -> Result<(), String> {
    let now = inspect_target(&expected.requested_path, Some(&expected.real_path))?;
    if now.real_path != expected.real_path {
        return Err("target real path changed before swap".into());
    }
    if now.target_inode != expected.target_inode || now.target_device != expected.target_device {
        return Err("target inode/device changed before swap".into());
    }
    if now.parent_inode != expected.parent_inode || now.parent_device != expected.parent_device {
        return Err("parent inode/device changed before swap".into());
    }
    Ok(())
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return normalize(path);
    }
    match std::env::current_dir() {
        Ok(cwd) => normalize(&cwd.join(path)),
        Err(_) => normalize(path),
    }
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_child_uses_realpath_of_parent() {
        let home = std::env::temp_dir();
        let missing = home.join("incodex-canonical-missing.app");
        let got = canonical_path(&missing);
        assert_eq!(got.file_name().unwrap(), missing.file_name().unwrap());
        assert!(got.parent().unwrap().is_absolute());
    }
}
