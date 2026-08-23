use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::{assert_not_symlink, write_private_file};

const GLOBAL_STATE_NAME: &str = ".codex-global-state.json";
const MAIN_WINDOW_BOUNDS_KEY: &str = "electron-main-window-bounds";
const DESKTOP_FIRST_SEEN_AT_MS_KEY: &str = "desktop-first-seen-at-ms";
const PERSISTED_ATOM_STATE_KEY: &str = "electron-persisted-atom-state";
const MIGRATION_ANNOUNCEMENT_KEY: &str = "chatgpt-migration-announcement-completed-v1";
const UPDATE_ANNOUNCEMENT_KEY: &str = "chatgpt-update-downloaded-announcement-seen-v1";
const MAIN_WINDOW_MIN_WIDTH: i32 = 480;
const MAIN_WINDOW_MIN_HEIGHT: i32 = 600;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedWindowBounds {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    #[serde(rename = "isMaximized")]
    _is_maximized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// 投影稳定几何并恢复官方空 Home 初始化哨兵，不复制用户全局状态。
pub fn seed_window_state(home: &Path, source_home: &Path) -> Result<bool, String> {
    seed_window_state_with_geometry(home, source_home, None)
}

/// 实时几何存在时优先使用；仅在窗口不可观测时回退到官方落盘状态。
pub fn seed_window_state_with_geometry(
    home: &Path,
    source_home: &Path,
    live_geometry: Option<WindowGeometry>,
) -> Result<bool, String> {
    let home_stat = assert_not_symlink(home, "session home")?;
    if !home_stat.map(|s| s.is_dir()).unwrap_or(false) {
        return Err(format!("session home missing: {}", home.display()));
    }
    let geometry = match live_geometry.filter(valid_geometry) {
        Some(geometry) => geometry,
        None => match persisted_geometry(source_home)? {
            Some(geometry) => geometry,
            None => return Ok(false),
        },
    };
    write_seeded_state(home, geometry)?;
    Ok(true)
}

fn valid_geometry(geometry: &WindowGeometry) -> bool {
    geometry.width >= MAIN_WINDOW_MIN_WIDTH && geometry.height >= MAIN_WINDOW_MIN_HEIGHT
}

fn persisted_geometry(source_home: &Path) -> Result<Option<WindowGeometry>, String> {
    let source = source_home.join(GLOBAL_STATE_NAME);
    let Some(source_stat) = assert_not_symlink(&source, "source global state")? else {
        return Ok(None);
    };
    if !source_stat.is_file() {
        return Err(format!(
            "source global state is not a file: {}",
            source.display()
        ));
    }
    let Ok(raw) = fs::read_to_string(&source) else {
        return Ok(None);
    };
    let Ok(source_state) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Ok(None);
    };
    let Some(raw_bounds) = source_state.get(MAIN_WINDOW_BOUNDS_KEY).cloned() else {
        return Ok(None);
    };
    let Ok(bounds) = serde_json::from_value::<PersistedWindowBounds>(raw_bounds) else {
        return Ok(None);
    };
    let geometry = WindowGeometry {
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: bounds.height,
    };
    if !valid_geometry(&geometry) {
        return Ok(None);
    }
    Ok(Some(geometry))
}

fn write_seeded_state(home: &Path, geometry: WindowGeometry) -> Result<(), String> {
    if !valid_geometry(&geometry) {
        return Err("window geometry is below the supported minimum".into());
    }
    let first_seen = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock precedes Unix epoch: {error}"))?
        .as_millis() as u64;
    let state = serde_json::json!({
        DESKTOP_FIRST_SEEN_AT_MS_KEY: first_seen,
        MAIN_WINDOW_BOUNDS_KEY: geometry,
        PERSISTED_ATOM_STATE_KEY: {
            MIGRATION_ANNOUNCEMENT_KEY: true,
            UPDATE_ANNOUNCEMENT_KEY: true,
        },
    });
    write_private_file(
        &home.join(GLOBAL_STATE_NAME),
        format!("{state}\n").as_bytes(),
        true,
    )?;
    Ok(())
}
