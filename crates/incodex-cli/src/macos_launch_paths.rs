use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

pub(crate) const BUNDLE_NAME: &str = "ChatGPT - Inc.app";
pub(crate) const LEGACY_BUNDLE_NAME: &str = "Incodex Launcher.app";

#[derive(Clone, Debug)]
pub(crate) struct LauncherDirectories {
    primary: PathBuf,
    legacy: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FileIdentity {
    pub(crate) device: u64,
    pub(crate) inode: u64,
    pub(crate) owner: u32,
}

impl FileIdentity {
    pub(crate) fn read(path: &Path) -> io::Result<Self> {
        let metadata = std::fs::symlink_metadata(path)?;
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            owner: metadata.uid(),
        })
    }
}

impl LauncherDirectories {
    pub(crate) fn system(home: &Path) -> Self {
        let legacy_home = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
        Self {
            primary: PathBuf::from("/Applications"),
            legacy: legacy_home.join("Applications"),
        }
    }

    #[cfg(test)]
    pub(crate) fn isolated(primary: &Path, legacy: &Path) -> Self {
        Self {
            primary: primary.to_path_buf(),
            legacy: legacy.to_path_buf(),
        }
    }

    pub(crate) fn primary_directory(&self) -> &Path {
        &self.primary
    }

    pub(crate) fn primary_bundle(&self) -> PathBuf {
        self.primary.join(BUNDLE_NAME)
    }

    pub(crate) fn previous_user_bundle(&self) -> PathBuf {
        self.legacy.join(BUNDLE_NAME)
    }

    pub(crate) fn legacy_user_bundle(&self) -> PathBuf {
        self.legacy.join(LEGACY_BUNDLE_NAME)
    }

    pub(crate) fn all_paths(&self) -> [PathBuf; 3] {
        [
            self.primary_bundle(),
            self.previous_user_bundle(),
            self.legacy_user_bundle(),
        ]
    }
}

pub(crate) fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    let from = CString::new(from.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source path contains NUL"))?;
    let to = CString::new(to.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "target path contains NUL"))?;
    let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(crate) fn is_current_user(owner: u32, effective_uid: u32) -> bool {
    owner == effective_uid
}

pub(crate) fn is_trusted_shared_directory_owner(owner: u32, effective_uid: u32) -> bool {
    owner == 0 || is_current_user(owner, effective_uid)
}

#[cfg(test)]
mod tests {
    use super::{is_current_user, is_trusted_shared_directory_owner, rename_noreplace};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn only_current_user_owns_launcher_bundles() {
        assert!(is_current_user(501, 501));
        assert!(!is_current_user(0, 501));
        assert!(!is_current_user(502, 501));
    }

    #[test]
    fn shared_applications_directory_accepts_root_or_current_user() {
        assert!(is_trusted_shared_directory_owner(0, 501));
        assert!(is_trusted_shared_directory_owner(501, 501));
        assert!(!is_trusted_shared_directory_owner(502, 501));
    }

    #[test]
    fn exclusive_rename_does_not_replace_a_racing_destination() {
        let root = std::env::temp_dir().join(format!(
            "incodex-launcher-rename-excl-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let source = root.join("staging");
        let destination = root.join("ChatGPT - Inc.app");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("foreign"), b"keep").unwrap();

        assert!(rename_noreplace(&source, &destination).is_err());
        assert!(source.exists());
        assert_eq!(fs::read(destination.join("foreign")).unwrap(), b"keep");

        fs::remove_dir_all(root).unwrap();
    }
}
