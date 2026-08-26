use std::fs;
use std::path::{Path, PathBuf};

use incodex_core::windows_session::{
    apply_private_windows_acl, ensure_private_windows_dir, verify_private_acl,
};

use crate::windows_file::{canonical_regular_file, ensure_regular_file, sha256_file};

const HELPER_NAME: &str = "incodex-helper.exe";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedWindowsHelper {
    pub executable: PathBuf,
    pub sha256: String,
}

pub fn publish_windows_helper(
    user_root: &Path,
    source: &Path,
) -> Result<PublishedWindowsHelper, String> {
    let source = canonical_regular_file(source, "Windows helper source")?;
    let sha256 = sha256_file(&source)?;
    let user_root = ensure_private_windows_dir(user_root)?;
    let windows_root = ensure_private_windows_dir(&user_root.join("windows"))?;
    let helpers_root = ensure_private_windows_dir(&windows_root.join("helpers"))?;
    let release = ensure_private_windows_dir(&helpers_root.join(&sha256))?;
    let executable = release.join(HELPER_NAME);

    match fs::symlink_metadata(&executable) {
        Ok(_) => verify_helper(&executable, &sha256)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            publish_helper_file(&release, &source, &executable, &sha256)?;
        }
        Err(error) => return Err(format!("cannot inspect Windows helper release: {error}")),
    }
    Ok(PublishedWindowsHelper { executable, sha256 })
}

fn publish_helper_file(
    release: &Path,
    source: &Path,
    executable: &Path,
    expected_hash: &str,
) -> Result<(), String> {
    let temporary = release.join(format!(".{HELPER_NAME}.tmp-{}", std::process::id()));
    let result = (|| {
        fs::copy(source, &temporary)
            .map_err(|error| format!("cannot stage Windows helper: {error}"))?;
        fs::OpenOptions::new()
            .write(true)
            .open(&temporary)
            .and_then(|file| file.sync_all())
            .map_err(|error| format!("cannot flush Windows helper: {error}"))?;
        apply_private_windows_acl(&temporary)?;
        verify_helper(&temporary, expected_hash)?;
        match fs::rename(&temporary, executable) {
            Ok(()) => Ok(()),
            Err(_) if executable.exists() => verify_helper(executable, expected_hash),
            Err(error) => Err(format!("cannot commit Windows helper: {error}")),
        }
    })();
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    apply_private_windows_acl(executable)?;
    verify_helper(executable, expected_hash)
}

fn verify_helper(path: &Path, expected_hash: &str) -> Result<(), String> {
    ensure_regular_file(path, "Windows helper")?;
    verify_private_acl(path)?;
    if sha256_file(path)? != expected_hash {
        return Err("Windows helper does not match its content address".to_string());
    }
    Ok(())
}
