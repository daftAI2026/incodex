use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path, PathBuf, Prefix};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

pub fn validate_existing_session_dir(
    trusted_root: &Path,
    candidate: &Path,
) -> Result<PathBuf, String> {
    require_absolute(trusted_root, "trusted root")?;
    require_absolute(candidate, "session directory")?;
    reject_reparse_ancestors(trusted_root)?;
    reject_reparse_ancestors(candidate)?;

    let trusted_real = fs::canonicalize(trusted_root).map_err(|error| {
        format!(
            "cannot resolve trusted session root {}: {error}",
            trusted_root.display()
        )
    })?;
    let candidate_real = fs::canonicalize(candidate).map_err(|error| {
        format!(
            "cannot resolve session directory {}: {error}",
            candidate.display()
        )
    })?;
    if !candidate_real.starts_with(&trusted_real) {
        return Err(format!(
            "session path is outside trusted root: {}",
            candidate.display()
        ));
    }
    if !candidate_real.is_dir() {
        return Err(format!(
            "session path is not a directory: {}",
            candidate.display()
        ));
    }
    Ok(candidate_real)
}

pub(crate) fn require_absolute(path: &Path, label: &str) -> Result<(), String> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(format!("{label} must be absolute: {}", path.display()))
    }
}

pub fn require_local_disk_absolute(path: &Path, label: &str) -> Result<(), String> {
    require_absolute(path, label)?;
    let local_disk = matches!(
        path.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
    );
    if local_disk {
        Ok(())
    } else {
        Err(format!(
            "{label} must be on a local disk: {}",
            path.display()
        ))
    }
}

pub fn reject_reparse_ancestors(path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        if component == Component::ParentDir {
            return Err(format!(
                "session path contains a parent traversal: {}",
                path.display()
            ));
        }
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            format!("cannot inspect session path {}: {error}", current.display())
        })?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(format!(
                "session path contains a reparse point: {}",
                current.display()
            ));
        }
    }
    Ok(())
}
