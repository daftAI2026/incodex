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

#[derive(Debug, Clone, Serialize)]
struct StableWindowGeometry {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

/// 投影稳定几何并恢复官方空 Home 初始化哨兵，不复制用户全局状态。
pub fn seed_window_state(home: &Path, source_home: &Path) -> Result<bool, String> {
    let home_stat = assert_not_symlink(home, "session home")?;
    if !home_stat.map(|s| s.is_dir()).unwrap_or(false) {
        return Err(format!("session home missing: {}", home.display()));
    }
    let source = source_home.join(GLOBAL_STATE_NAME);
    let Some(source_stat) = assert_not_symlink(&source, "source global state")? else {
        return Ok(false);
    };
    if !source_stat.is_file() {
        return Err(format!(
            "source global state is not a file: {}",
            source.display()
        ));
    }
    let Ok(raw) = fs::read_to_string(&source) else {
        return Ok(false);
    };
    let Ok(source_state) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Ok(false);
    };
    let Some(raw_bounds) = source_state.get(MAIN_WINDOW_BOUNDS_KEY).cloned() else {
        return Ok(false);
    };
    let Ok(bounds) = serde_json::from_value::<PersistedWindowBounds>(raw_bounds) else {
        return Ok(false);
    };
    if bounds.width < MAIN_WINDOW_MIN_WIDTH || bounds.height < MAIN_WINDOW_MIN_HEIGHT {
        return Ok(false);
    }
    let geometry = StableWindowGeometry {
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: bounds.height,
    };
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
    Ok(true)
}
