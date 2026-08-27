use std::fs;
use std::path::{Path, PathBuf};

use incodex_core::windows_session::{
    apply_private_windows_acl, ensure_private_windows_dir, verify_private_acl,
};
use sha2::{Digest, Sha256};

use crate::windows_file::{canonical_regular_file, ensure_regular_file, sha256_file};

const HELPER_NAME: &str = "incodex-helper.exe";
const TRANSIENT_HELPER_DIRECTORY: &str = "t";
const TRANSIENT_HELPER_NAME: &str = "i.exe";
const TRANSIENT_CONTENT_ADDRESS_LEN: usize = 16;
const DOS_PE_OFFSET: usize = 0x3c;
const COFF_HEADER_SIZE: usize = 20;
const OPTIONAL_HEADER_SUBSYSTEM_OFFSET: usize = 68;
const PE32_MAGIC: u16 = 0x10b;
const PE32_PLUS_MAGIC: u16 = 0x20b;
const WINDOWS_GUI_SUBSYSTEM: u16 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedWindowsHelper {
    pub executable: PathBuf,
    pub sha256: String,
}

pub fn publish_windows_helper(
    user_root: &Path,
    source: &Path,
) -> Result<PublishedWindowsHelper, String> {
    publish_windows_helper_with_layout(user_root, source, false)
}

pub fn publish_windows_transient_helper(
    user_root: &Path,
    source: &Path,
) -> Result<PublishedWindowsHelper, String> {
    publish_windows_helper_with_layout(user_root, source, true)
}

fn publish_windows_helper_with_layout(
    user_root: &Path,
    source: &Path,
    transient: bool,
) -> Result<PublishedWindowsHelper, String> {
    let source = canonical_regular_file(source, "Windows helper source")?;
    let helper_bytes = windowless_helper_bytes(&source)?;
    let sha256: String = Sha256::digest(&helper_bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let user_root = ensure_private_windows_dir(user_root)?;
    let windows_root = ensure_private_windows_dir(&user_root.join("windows"))?;
    let (collection, release_name, helper_name) = if transient {
        (
            TRANSIENT_HELPER_DIRECTORY,
            &sha256[..TRANSIENT_CONTENT_ADDRESS_LEN],
            TRANSIENT_HELPER_NAME,
        )
    } else {
        ("helpers", sha256.as_str(), HELPER_NAME)
    };
    let helpers_root = ensure_private_windows_dir(&windows_root.join(collection))?;
    let release = ensure_private_windows_dir(&helpers_root.join(release_name))?;
    let executable = release.join(helper_name);

    match fs::symlink_metadata(&executable) {
        Ok(_) => verify_helper(&executable, &sha256)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            publish_helper_file(&release, helper_name, &helper_bytes, &executable, &sha256)?;
        }
        Err(error) => return Err(format!("cannot inspect Windows helper release: {error}")),
    }
    Ok(PublishedWindowsHelper { executable, sha256 })
}

fn publish_helper_file(
    release: &Path,
    helper_name: &str,
    helper_bytes: &[u8],
    executable: &Path,
    expected_hash: &str,
) -> Result<(), String> {
    let temporary = release.join(format!(".{helper_name}.tmp-{}", std::process::id()));
    let result = (|| {
        fs::write(&temporary, helper_bytes)
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
    let bytes = fs::read(path).map_err(|error| format!("cannot read Windows helper: {error}"))?;
    if pe_subsystem_offset(&bytes)
        .and_then(|offset| read_u16(&bytes, offset, "Windows helper subsystem"))?
        != WINDOWS_GUI_SUBSYSTEM
    {
        return Err("Windows helper is not a windowless GUI-subsystem executable".to_string());
    }
    Ok(())
}

fn windowless_helper_bytes(source: &Path) -> Result<Vec<u8>, String> {
    let mut bytes =
        fs::read(source).map_err(|error| format!("cannot read Windows helper source: {error}"))?;
    let subsystem = pe_subsystem_offset(&bytes)?;
    bytes[subsystem..subsystem + 2].copy_from_slice(&WINDOWS_GUI_SUBSYSTEM.to_le_bytes());
    Ok(bytes)
}

fn pe_subsystem_offset(bytes: &[u8]) -> Result<usize, String> {
    if bytes.get(0..2) != Some(b"MZ") {
        return Err("Windows helper source is not a PE executable".to_string());
    }
    let pe_offset = read_u32(bytes, DOS_PE_OFFSET, "Windows helper PE offset")? as usize;
    if bytes.get(pe_offset..pe_offset.saturating_add(4)) != Some(b"PE\0\0") {
        return Err("Windows helper source has an invalid PE signature".to_string());
    }
    let optional_header = pe_offset
        .checked_add(4 + COFF_HEADER_SIZE)
        .ok_or_else(|| "Windows helper PE header overflows".to_string())?;
    let magic = read_u16(
        bytes,
        optional_header,
        "Windows helper optional-header magic",
    )?;
    if !matches!(magic, PE32_MAGIC | PE32_PLUS_MAGIC) {
        return Err("Windows helper source has an unsupported PE optional header".to_string());
    }
    let subsystem = optional_header
        .checked_add(OPTIONAL_HEADER_SUBSYSTEM_OFFSET)
        .ok_or_else(|| "Windows helper subsystem offset overflows".to_string())?;
    let _ = read_u16(bytes, subsystem, "Windows helper subsystem")?;
    Ok(subsystem)
}

fn read_u16(bytes: &[u8], offset: usize, label: &str) -> Result<u16, String> {
    let value = bytes
        .get(offset..offset.saturating_add(2))
        .ok_or_else(|| format!("{label} is truncated"))?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize, label: &str) -> Result<u32, String> {
    let value = bytes
        .get(offset..offset.saturating_add(4))
        .ok_or_else(|| format!("{label} is truncated"))?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_helper_identity_accepts_short_and_legacy_layouts_only() {
        let root = Path::new(r"C:\Users\test\.incodex");
        let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert!(installed_windows_helper_path_matches(
            root,
            hash,
            &root.join(r"windows\i\0123456789abcdef\i.exe")
        ));
        assert!(installed_windows_helper_path_matches(
            root,
            hash,
            &root
                .join("windows")
                .join("helpers")
                .join(hash)
                .join("incodex-helper.exe")
        ));
        assert!(!installed_windows_helper_path_matches(
            root,
            hash,
            &root.join(r"windows\i\fedcba9876543210\i.exe")
        ));
    }
}
