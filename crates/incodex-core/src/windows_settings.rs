use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

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
const MAIN_WINDOW_BOUNDS_KEY: &str = "electron-main-window-bounds";
const FIRST_SEEN_KEY: &str = "desktop-first-seen-at-ms";
// Chrome Aura offsets a new window from its source by ten physical pixels.
const CHROME_WINDOW_TILE_PIXELS: i64 = 10;
const MAIN_WINDOW_MIN_WIDTH: i64 = 480;
const MAIN_WINDOW_MIN_HEIGHT: i64 = 600;

const SETTINGS_FILES: &[(&str, u64)] = &[
    (AUTH_SETTING_FILE, MAX_WINDOWS_AUTH_BYTES),
    (CONFIG_SETTING_FILE, MAX_WINDOWS_CONFIG_BYTES),
];

pub fn copy_windows_settings(
    session: &WindowsSessionHome,
    source_home: &Path,
) -> Result<usize, String> {
    copy_windows_settings_with_bounds(session, source_home, None)
}

pub fn copy_windows_settings_with_bounds(
    session: &WindowsSessionHome,
    source_home: &Path,
    live_source_bounds: Option<&str>,
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
    project_window_layout(session, source_home, live_source_bounds)?;
    Ok(copied)
}

// 只投影原厂窗口布局数值；全局状态还含聊天与账户状态，绝不能整份复制。
fn project_window_layout(
    session: &WindowsSessionHome,
    source_home: &Path,
    live_source_bounds: Option<&str>,
) -> Result<(), String> {
    let live_bounds = live_source_bounds.map(tiled_live_bounds).transpose()?;
    let source = source_home.join(GLOBAL_STATE_FILE);
    let state = read_source_global_state(&source)?;
    let state = state.unwrap_or(serde_json::Value::Null);
    let mut projected_atoms = serde_json::Map::new();
    for key in [WINDOW_ZOOM_KEY, SIDEBAR_WIDTH_KEY] {
        if let Some(value) = state
            .get(PERSISTED_ATOM_STATE_KEY)
            .and_then(|atoms| atoms.get(key))
            .filter(|value| {
                value
                    .as_f64()
                    .is_some_and(|number| number.is_finite() && number > 0.0)
            })
        {
            projected_atoms.insert(key.to_string(), value.clone());
        }
    }
    let bounds = live_bounds.or_else(|| {
        state
            .get(MAIN_WINDOW_BOUNDS_KEY)
            .and_then(tiled_source_bounds)
    });
    if projected_atoms.is_empty() && bounds.is_none() {
        return Ok(());
    }
    let mut projected = serde_json::Map::new();
    if !projected_atoms.is_empty() {
        projected.insert(PERSISTED_ATOM_STATE_KEY.to_string(), projected_atoms.into());
    }
    if let Some(bounds) = bounds {
        projected.insert(MAIN_WINDOW_BOUNDS_KEY.to_string(), bounds);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("cannot read Windows clock: {error}"))?
            .as_millis();
        projected.insert(FIRST_SEEN_KEY.to_string(), serde_json::json!(now));
    }
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

fn read_source_global_state(source: &Path) -> Result<Option<serde_json::Value>, String> {
    match fs::symlink_metadata(source) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot inspect {}: {error}", source.display())),
        Ok(_) => {}
    }
    reject_reparse_ancestors(source)?;
    let source_file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(source)
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
    Ok(serde_json::from_slice::<serde_json::Value>(&raw).ok())
}

fn tiled_source_bounds(value: &serde_json::Value) -> Option<serde_json::Value> {
    let object = value.as_object()?;
    object.get("isMaximized")?.as_bool()?;
    let x = object.get("x")?.as_i64()?;
    let y = object.get("y")?.as_i64()?;
    let width = object.get("width")?.as_i64()?;
    let height = object.get("height")?.as_i64()?;
    tile_bounds(x, y, width, height)
}

pub fn tiled_live_bounds(raw: &str) -> Result<serde_json::Value, String> {
    let parts = raw
        .split(',')
        .map(str::parse::<i64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Windows source window bounds are invalid".to_string())?;
    if parts.len() != 4 {
        return Err("Windows source window bounds are invalid".to_string());
    }
    tile_bounds(parts[0], parts[1], parts[2], parts[3])
        .ok_or_else(|| "Windows source window bounds are invalid".to_string())
}

fn tile_bounds(x: i64, y: i64, width: i64, height: i64) -> Option<serde_json::Value> {
    if width < MAIN_WINDOW_MIN_WIDTH
        || height < MAIN_WINDOW_MIN_HEIGHT
        || width > i32::MAX as i64
        || height > i32::MAX as i64
    {
        return None;
    }
    let x = i32::try_from(x.checked_add(CHROME_WINDOW_TILE_PIXELS)?).ok()?;
    let y = i32::try_from(y.checked_add(CHROME_WINDOW_TILE_PIXELS)?).ok()?;
    Some(
        serde_json::json!({"x": x, "y": y, "width": width, "height": height, "isMaximized": false}),
    )
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
