use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn collect_directory_entries<I>(entries: I) -> Result<Vec<PathBuf>, io::Error>
where
    I: IntoIterator<Item = io::Result<PathBuf>>,
{
    entries.into_iter().collect()
}

pub(crate) fn read_directory(path: &Path) -> Result<Option<Vec<PathBuf>>, String> {
    match fs::read_dir(path) {
        Ok(entries) => collect_directory_entries(
            entries
                .into_iter()
                .map(|entry| entry.map(|entry| entry.path())),
        )
        .map(Some)
        .map_err(|error| error.to_string()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn is_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

pub(crate) fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn file_name_starts(path: &Path, prefix: &str) -> bool {
    file_name(path).starts_with(prefix)
}

pub(crate) fn looks_like_uuid(value: &str) -> bool {
    value.len() == 36
        && value.as_bytes().iter().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

pub(crate) fn basename(value: &str) -> &str {
    value.rsplit('/').next().unwrap_or(value)
}

#[derive(Debug, Clone)]
pub(crate) struct LiveProcess {
    pub start: String,
    pub exec: String,
}

pub(crate) fn live_process_identity(pid: i32) -> Option<LiveProcess> {
    if pid <= 0 {
        return None;
    }
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart=,comm="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let (start, exec) = line.rsplit_once(char::is_whitespace)?;
    if start.trim().is_empty() || exec.trim().is_empty() {
        return None;
    }
    Some(LiveProcess {
        start: start.trim().to_string(),
        exec: exec.trim().to_string(),
    })
}

pub(crate) fn pid_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    unsafe { libc::kill(pid, 0) == 0 }
}
