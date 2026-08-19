use std::path::{Path, PathBuf};

use crate::paths::DEFAULT_APP;

pub fn canonical_path(input: impl AsRef<Path>) -> PathBuf {
    let requested = absolute(input.as_ref());
    if let Ok(real) = std::fs::canonicalize(&requested) {
        return real;
    }
    if let Some(parent) = requested.parent() {
        if let Ok(real_parent) = std::fs::canonicalize(parent) {
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
