use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{assert_not_symlink, write_private_file};

const GLOBAL_STATE_NAME: &str = ".codex-global-state.json";
const MAIN_WINDOW_BOUNDS_KEY: &str = "electron-main-window-bounds";
const MAIN_WINDOW_MIN_WIDTH: i32 = 480;
const MAIN_WINDOW_MIN_HEIGHT: i32 = 600;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedWindowBounds {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    is_maximized: bool,
}

/// 将官方持久化的主窗口尺寸投影到隔离 home，不复制其他全局状态。
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
    let state = serde_json::json!({ MAIN_WINDOW_BOUNDS_KEY: bounds });
    write_private_file(
        &home.join(GLOBAL_STATE_NAME),
        format!("{state}\n").as_bytes(),
        true,
    )?;
    Ok(true)
}
