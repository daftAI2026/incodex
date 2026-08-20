use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_core::canonical_path;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::durable::ensure_private_dir;

static LOCK_TEMP_SEQ: AtomicU64 = AtomicU64::new(0);
static LOCK_OWNER_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockRecord {
    schema_version: u32,
    pid: u32,
    process_start: String,
    command: String,
    install_id: Option<String>,
    #[serde(default)]
    owner_token: Option<String>,
    requested_path: String,
    real_path: String,
    created_at: String,
}

pub struct TargetLock {
    path: PathBuf,
    pid: u32,
    owner_token: String,
}

impl Drop for TargetLock {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

impl TargetLock {
    fn release(&mut self) -> Result<(), String> {
        let Ok(body) = fs::read_to_string(&self.path) else {
            return Ok(());
        };
        let Ok(record) = serde_json::from_str::<LockRecord>(&body) else {
            return Ok(());
        };
        if record.pid != self.pid
            || record.owner_token.as_deref() != Some(self.owner_token.as_str())
        {
            return Ok(());
        }
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    }
}

pub fn lock_path_for(root: &Path, target_path: &Path) -> PathBuf {
    let digest = Sha256::digest(canonical_path(target_path).to_string_lossy().as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    root.join("locks").join(format!("{hex}.lock"))
}

pub fn acquire_target_lock(
    root: &Path,
    target_path: &Path,
    command: &str,
    install_id: Option<&str>,
) -> Result<TargetLock, String> {
    let real_path = canonical_path(target_path);
    let path = lock_path_for(root, target_path);
    ensure_private_dir(path.parent().unwrap())?;
    let owner_token = new_owner_token();
    let record = LockRecord {
        schema_version: 1,
        pid: std::process::id(),
        process_start: process_start(std::process::id() as i32).unwrap_or_default(),
        command: command.to_string(),
        install_id: install_id.map(str::to_string),
        owner_token: Some(owner_token.clone()),
        requested_path: target_path.display().to_string(),
        real_path: real_path.display().to_string(),
        created_at: unix_now(),
    };
    match write_exclusive(&path, &record) {
        Ok(()) => Ok(TargetLock {
            path,
            pid: std::process::id(),
            owner_token: owner_token.clone(),
        }),
        Err(_) => {
            if steal_if_stale(&path) {
                write_exclusive(&path, &record)?;
                return Ok(TargetLock {
                    path,
                    pid: std::process::id(),
                    owner_token,
                });
            }
            let who = fs::read_to_string(&path)
                .ok()
                .and_then(|body| serde_json::from_str::<LockRecord>(&body).ok())
                .map(|holder| format!("{} pid {}", holder.command, holder.pid))
                .unwrap_or_else(|| "another process".into());
            Err(format!(
                "another incodex command is modifying this app ({who})"
            ))
        }
    }
}

fn new_owner_token() -> String {
    let sequence = LOCK_OWNER_SEQ.fetch_add(1, Ordering::Relaxed);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{}-{timestamp}-{sequence}", std::process::id())
}

fn write_exclusive(path: &Path, record: &LockRecord) -> Result<(), String> {
    let body = format!(
        "{}\n",
        serde_json::to_string(record).map_err(|err| err.to_string())?
    );
    let parent = path.parent().ok_or("mutation lock has no parent")?;
    let seq = LOCK_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".lock-{}-{}-{seq}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    let mut opts = OpenOptions::new();
    opts.write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW);
    let mut file = opts.open(&temporary).map_err(|err| err.to_string())?;
    file.write_all(body.as_bytes())
        .map_err(|err| err.to_string())?;
    file.sync_data().map_err(|err| err.to_string())?;
    drop(file);
    let linked = fs::hard_link(&temporary, path).map_err(|err| err.to_string());
    let _ = fs::remove_file(&temporary);
    linked
}

fn steal_if_stale(path: &Path) -> bool {
    let Ok(body) = fs::read_to_string(path) else {
        let _ = fs::remove_file(path);
        return true;
    };
    let Ok(holder) = serde_json::from_str::<LockRecord>(&body) else {
        let _ = fs::remove_file(path);
        return true;
    };
    if lock_is_live(&holder) {
        return false;
    }
    fs::remove_file(path).is_ok()
}

fn lock_is_live(holder: &LockRecord) -> bool {
    if !pid_alive(holder.pid as i32) {
        return false;
    }
    if holder.process_start.is_empty() {
        return true;
    }
    match process_start(holder.pid as i32) {
        None => true,
        Some(current) => current == holder.process_start,
    }
}

fn pid_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    unsafe { libc::kill(pid, 0) == 0 }
}

fn process_start(pid: i32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let start = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if start.is_empty() {
        None
    } else {
        Some(start)
    }
}

fn unix_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}
