use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

use crate::windows_path::{reject_reparse_ancestors, require_local_disk_absolute};

use super::{
    apply_private_acl, validate_session_identity, verify_private_acl, WindowsSessionHome,
    FILE_ATTRIBUTE_REPARSE_POINT,
};

pub const MAX_WINDOWS_AUTH_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_WINDOWS_CONFIG_BYTES: u64 = 1024 * 1024;

const SETTINGS_FILES: &[(&str, u64)] = &[
    ("auth.json", MAX_WINDOWS_AUTH_BYTES),
    ("config.toml", MAX_WINDOWS_CONFIG_BYTES),
];

pub fn copy_windows_settings(
    session: &WindowsSessionHome,
    source_home: &Path,
) -> Result<usize, String> {
    validate_session_identity(session)?;
    require_local_disk_absolute(source_home, "Windows Codex source home")?;
    match fs::symlink_metadata(source_home) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => {
            return Err(format!(
                "cannot inspect Windows Codex source home {}: {error}",
                source_home.display()
            ))
        }
        Ok(_) => {}
    }
    reject_reparse_ancestors(source_home)?;
    if !source_home.is_dir() {
        return Err(format!(
            "Windows Codex source home is not a directory: {}",
            source_home.display()
        ));
    }

    let mut copied = 0;
    for &(name, limit) in SETTINGS_FILES {
        let source = source_home.join(name);
        match fs::symlink_metadata(&source) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("cannot inspect {}: {error}", source.display())),
            Ok(metadata) if metadata.len() > limit => {
                return Err(setting_size_error(&source, limit));
            }
            Ok(_) => {}
        }
        reject_reparse_ancestors(&source)?;
        copy_private_file(&source, &session.home.join(name), limit)?;
        copied += 1;
    }
    Ok(copied)
}

fn copy_private_file(source: &Path, destination: &Path, limit: u64) -> Result<(), String> {
    if fs::symlink_metadata(destination).is_ok() {
        return Err(format!(
            "refuse to overwrite Windows session setting: {}",
            destination.display()
        ));
    }
    let source_file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(source)
        .map_err(|error| format!("cannot open source setting {}: {error}", source.display()))?;
    let metadata = source_file.metadata().map_err(|error| {
        format!(
            "cannot inspect source setting {}: {error}",
            source.display()
        )
    })?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!(
            "source setting is not a plain file: {}",
            source.display()
        ));
    }
    if metadata.len() > limit {
        return Err(setting_size_error(source, limit));
    }

    let mut destination_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| {
            format!(
                "cannot create session setting {}: {error}",
                destination.display()
            )
        })?;
    let copy_result =
        io::copy(&mut source_file.take(limit + 1), &mut destination_file).and_then(|copied| {
            if copied > limit {
                Err(io::Error::other(format!(
                    "source grew beyond the {limit}-byte size limit"
                )))
            } else {
                destination_file.sync_all()?;
                Ok(copied)
            }
        });
    if let Err(error) = copy_result {
        drop(destination_file);
        let _ = fs::remove_file(destination);
        return Err(format!(
            "cannot copy session setting {}: {error}",
            destination.display()
        ));
    }
    drop(destination_file);
    if let Err(error) = apply_private_acl(destination).and_then(|_| verify_private_acl(destination))
    {
        let _ = fs::remove_file(destination);
        return Err(error);
    }
    Ok(())
}

fn setting_size_error(source: &Path, limit: u64) -> String {
    format!(
        "Windows setting exceeds its {limit}-byte size limit: {}",
        source.display()
    )
}
