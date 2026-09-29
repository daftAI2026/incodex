use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

use crate::session_layout::{AUTH_SETTING_FILE, CONFIG_SETTING_FILE};
use crate::windows_path::{reject_reparse_ancestors, require_local_disk_absolute};

use super::{
    apply_private_acl, validate_session_identity, verify_private_acl, WindowsSessionHome,
    FILE_ATTRIBUTE_REPARSE_POINT,
};

pub const MAX_WINDOWS_AUTH_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_WINDOWS_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_WINDOWS_GLOBAL_STATE_BYTES: u64 = 16 * 1024 * 1024;
const GLOBAL_STATE_FILE: &str = ".codex-global-state.json";
const PERSISTED_ATOM_STATE_KEY: &str = "electron-persisted-atom-state";
const WINDOW_ZOOM_KEY: &str = "electron:window-zoom";
const SIDEBAR_WIDTH_KEY: &str = "sidebar-width";

const SETTINGS_FILES: &[(&str, u64)] = &[
    (AUTH_SETTING_FILE, MAX_WINDOWS_AUTH_BYTES),
    (CONFIG_SETTING_FILE, MAX_WINDOWS_CONFIG_BYTES),
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
    project_window_layout(session, source_home)?;
    Ok(copied)
}

// 只投影原厂窗口布局数值；全局状态还含聊天与账户状态，绝不能整份复制。
fn project_window_layout(session: &WindowsSessionHome, source_home: &Path) -> Result<(), String> {
    let source = source_home.join(GLOBAL_STATE_FILE);
    match fs::symlink_metadata(&source) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("cannot inspect {}: {error}", source.display())),
        Ok(_) => {}
    }
    reject_reparse_ancestors(&source)?;
    let source_file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&source)
        .map_err(|error| format!("cannot open source state {}: {error}", source.display()))?;
    let metadata = source_file
        .metadata()
        .map_err(|error| format!("cannot inspect source state {}: {error}", source.display()))?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!(
            "source state is not a plain file: {}",
            source.display()
        ));
    }
    if metadata.len() > MAX_WINDOWS_GLOBAL_STATE_BYTES {
        return Err(format!(
            "Windows source state exceeds its size limit: {}",
            source.display()
        ));
    }
    let mut raw = Vec::new();
    source_file
        .take(MAX_WINDOWS_GLOBAL_STATE_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|error| format!("cannot read source state {}: {error}", source.display()))?;
    if raw.len() as u64 > MAX_WINDOWS_GLOBAL_STATE_BYTES {
        return Err(format!(
            "Windows source state exceeds its size limit: {}",
            source.display()
        ));
    }
    let Ok(state) = serde_json::from_slice::<serde_json::Value>(&raw) else {
        return Ok(());
    };
    let Some(source_atoms) = state.get(PERSISTED_ATOM_STATE_KEY) else {
        return Ok(());
    };
    let mut projected_atoms = serde_json::Map::new();
    for key in [WINDOW_ZOOM_KEY, SIDEBAR_WIDTH_KEY] {
        if let Some(value) = source_atoms.get(key).filter(|value| {
            value
                .as_f64()
                .is_some_and(|number| number.is_finite() && number > 0.0)
        }) {
            projected_atoms.insert(key.to_string(), value.clone());
        }
    }
    if projected_atoms.is_empty() {
        return Ok(());
    }
    let projected = serde_json::json!({PERSISTED_ATOM_STATE_KEY: projected_atoms});
    let mut encoded = serde_json::to_vec(&projected)
        .map_err(|error| format!("cannot encode projected window layout: {error}"))?;
    encoded.push(b'\n');
    let destination = session.home.join(GLOBAL_STATE_FILE);
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .map_err(|error| {
            format!(
                "cannot create projected state {}: {error}",
                destination.display()
            )
        })?;
    if let Err(error) = output.write_all(&encoded).and_then(|_| output.sync_all()) {
        drop(output);
        let _ = fs::remove_file(&destination);
        return Err(format!(
            "cannot write projected state {}: {error}",
            destination.display()
        ));
    }
    drop(output);
    if let Err(error) =
        apply_private_acl(&destination).and_then(|_| verify_private_acl(&destination))
    {
        let _ = fs::remove_file(&destination);
        return Err(error);
    }
    Ok(())
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
