use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

#[cfg(target_os = "macos")]
use crate::live_window_macos::system_window_candidates;
use crate::{ProcessProbe, SystemProcessProbe};

const MAIN_WINDOW_MIN_WIDTH: i32 = 480;
const MAIN_WINDOW_MIN_HEIGHT: i32 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WindowCandidate {
    pub pid: i32,
    pub layer: i32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn live_main_window_bounds(executable: &Path) -> Result<Option<WindowBounds>, String> {
    let expected = fs::canonicalize(executable).map_err(|error| {
        format!(
            "cannot resolve source executable {}: {error}",
            executable.display()
        )
    })?;
    let probe = SystemProcessProbe;
    let mut official_pids = Vec::new();
    for (pid, path) in probe.process_paths()? {
        if path != expected {
            continue;
        }
        let command = process_command(pid)?;
        if !is_isolated_launch_command(&command) {
            official_pids.push(pid);
        }
    }
    if official_pids.is_empty() {
        return Ok(None);
    }
    let windows = system_window_candidates()?;
    Ok(select_live_main_window_bounds(&official_pids, &windows))
}

fn process_command(pid: i32) -> Result<String, String> {
    let output = Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .map_err(|error| format!("cannot inspect source process {pid}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "/bin/ps could not inspect source process {pid}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(crate) fn is_isolated_launch_command(command: &str) -> bool {
    command
        .split_whitespace()
        .any(|arg| arg == "--user-data-dir" || arg.starts_with("--user-data-dir="))
}

pub(crate) fn select_live_main_window_bounds(
    official_pids: &[i32],
    windows: &[WindowCandidate],
) -> Option<WindowBounds> {
    let official_pids: HashSet<i32> = official_pids.iter().copied().collect();
    windows
        .iter()
        .filter(|window| {
            official_pids.contains(&window.pid)
                && window.layer == 0
                && window.width >= MAIN_WINDOW_MIN_WIDTH
                && window.height >= MAIN_WINDOW_MIN_HEIGHT
        })
        .max_by_key(|window| i64::from(window.width) * i64::from(window.height))
        .map(|window| WindowBounds {
            x: window.x,
            y: window.y,
            width: window.width,
            height: window.height,
        })
}

#[cfg(not(target_os = "macos"))]
fn system_window_candidates() -> Result<Vec<WindowCandidate>, String> {
    Ok(Vec::new())
}
