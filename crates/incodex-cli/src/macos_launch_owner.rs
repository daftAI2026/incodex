/*
 * [INPUT]: 依赖既有目标锁、私有原子文件与NormalHost进程/端口证据。
 * [OUTPUT]: 串行化普通宿主启动；标准profile跨包路径复用须命中已验证owner。
 * [POS]: launch的App作用域owner记录；profile锁只选候选，不授权激活。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

use crate::macos_launch_process::{NormalHost, ProcessOwner};
use crate::macos_update_assets::{ensure_private_dir, read_regular_file, write_private_atomic};
use incodex_transaction::{acquire_target_lock, lock_path_for, TargetLock};
use serde::{Deserialize, Serialize};
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) enum LaunchClaim {
    Primary(LaunchOwner),
    Existing(NormalHost),
}

pub(crate) struct LaunchOwner {
    _lock: TargetLock,
    path: PathBuf,
    app: PathBuf,
    controller: Option<ProcessOwner>,
}

#[derive(Deserialize, Serialize)]
struct Record {
    schema: u8,
    app: PathBuf,
    controller: ProcessOwner,
    host: NormalHost,
}

pub(crate) fn claim(root: &Path, app: &Path) -> Result<LaunchClaim, String> {
    incodex_transaction::validate_storage_root(root)?;
    let app = std::fs::canonicalize(app).map_err(|e| e.to_string())?;
    let directory = root.join("launchers");
    let key = crate::macos_update_assets::sha256_hex(app.to_string_lossy().as_bytes());
    let path = directory.join(format!("{key}.json"));
    ensure_private_dir(&directory)?;
    if let Some(host) = live_host(&path, &app)? {
        return Ok(LaunchClaim::Existing(host));
    }
    // 专用receipt路径作为锁目标，不占用install/uninstall的官方App事务锁。
    let lock = match acquire_target_lock(root, &path, "launch", None) {
        Ok(lock) => lock,
        Err(error) => {
            // 首启已持锁但尚未发布端口。复用原启动预算，不创建第二个controller。
            let deadline = std::time::Instant::now() + crate::macos_launch_process::STARTUP_TIMEOUT;
            let lock_path = lock_path_for(root, &path);
            while lock_path.is_file() && std::time::Instant::now() < deadline {
                if let Some(host) = live_host(&path, &app)? {
                    return Ok(LaunchClaim::Existing(host));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            return Err(error);
        }
    };
    if let Some(host) = live_host(&path, &app)? {
        return Ok(LaunchClaim::Existing(host));
    }
    Ok(LaunchClaim::Primary(LaunchOwner {
        _lock: lock,
        path,
        app,
        controller: None,
    }))
}

pub(crate) fn active_profile_host(
    root: &Path,
    profile: &Path,
) -> Result<Option<(PathBuf, NormalHost)>, String> {
    let lock = profile.join("SingletonLock");
    let lock_value = match std::fs::read_link(&lock) {
        Ok(value) => value,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidInput
            ) =>
        {
            return Ok(None)
        }
        Err(error) => {
            return Err(format!(
                "cannot inspect normal Codex profile owner: {error}"
            ))
        }
    };
    let Some(pid) = lock_value
        .to_str()
        .and_then(|value| value.rsplit_once('-'))
        .and_then(|(_, pid)| pid.parse::<i32>().ok())
        .filter(|pid| *pid > 0)
    else {
        return Ok(None);
    };
    let socket_link = profile.join("SingletonSocket");
    let socket_value = match std::fs::read_link(&socket_link) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "cannot inspect normal Codex profile socket: {error}"
            ))
        }
    };
    let socket_path = if socket_value.is_absolute() {
        socket_value.clone()
    } else {
        profile.join(&socket_value)
    };
    if !owns_profile_socket(pid, &socket_path)? {
        return Ok(None);
    }
    incodex_transaction::validate_storage_root(root)?;
    let directory = root.join("launchers");
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot inspect recorded Codex launchers: {error}")),
    };
    let metadata = std::fs::symlink_metadata(&directory).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("recorded Codex launchers path is not a real directory".into());
    }
    let mut selected = None;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        let Some(key) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if path.extension().and_then(|value| value.to_str()) != Some("json")
            || key.len() != 64
            || !key.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            continue;
        }
        let Some(record) = read_record(&path)? else {
            continue;
        };
        if record.host.pid() != pid {
            continue;
        }
        let expected_key =
            crate::macos_update_assets::sha256_hex(record.app.to_string_lossy().as_bytes());
        if record.schema != 1 || key != expected_key {
            return Err("normal profile owner record does not match its published identity".into());
        }
        if !record.controller.is_alive()? || !record.host.validate_app(&record.app)? {
            continue;
        }
        if selected.is_some() {
            return Err("normal Codex profile has ambiguous recorded owners".into());
        }
        selected = Some((record.app, record.host));
    }
    if std::fs::read_link(&lock).ok().as_ref() != Some(&lock_value)
        || std::fs::read_link(&socket_link).ok().as_ref() != Some(&socket_value)
    {
        return Err("normal Codex profile owner changed; retry launch".into());
    }
    Ok(selected)
}

fn owns_profile_socket(pid: i32, path: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => {}
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("cannot inspect profile socket: {error}")),
    }
    let Some(expected) = path.to_str() else {
        return Ok(false);
    };
    let output = Command::new("/usr/sbin/lsof")
        .args(["-a", "-p", &pid.to_string(), "-U", "-Fn"])
        .output()
        .map_err(|error| format!("cannot inspect profile socket owner: {error}"))?;
    if !output.status.success() {
        return Ok(false);
    }
    let mut current_pid = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(value) = line.strip_prefix('p') {
            current_pid = value.parse::<i32>().ok();
        } else if current_pid == Some(pid) && line.strip_prefix('n') == Some(expected) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn read_record(path: &Path) -> Result<Option<Record>, String> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(m) if m.len() > 8192 => return Err("launch owner record exceeds its size limit".into()),
        Ok(_) => {}
    }
    let bytes = read_regular_file(path, "launch owner record")?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| format!("cannot read launch owner record: {e}"))
}

fn live_host(path: &Path, app: &Path) -> Result<Option<NormalHost>, String> {
    let Some(record) = read_record(path)? else {
        return Ok(None);
    };
    if record.schema != 1 || record.app != app {
        return Err("launch owner record does not match the requested app".into());
    }
    if !record.host.validate_app(app)? {
        return Ok(None);
    }
    if !record.controller.is_alive()? {
        return Err(
            "the captured Codex launcher has exited; quit that normal app before launching again"
                .into(),
        );
    }
    Ok(Some(record.host))
}

impl LaunchOwner {
    pub(crate) fn publish(&mut self, host: &NormalHost) -> Result<(), String> {
        if !host.validate_app(&self.app)? {
            return Err("cannot publish an inactive Codex host".into());
        }
        let controller = ProcessOwner::current()?;
        let record = Record {
            schema: 1,
            app: self.app.clone(),
            controller: controller.clone(),
            host: host.clone(),
        };
        write_private_atomic(
            &self.path,
            &serde_json::to_vec(&record).map_err(|e| e.to_string())?,
        )?;
        self.controller = Some(controller);
        Ok(())
    }
}

impl Drop for LaunchOwner {
    fn drop(&mut self) {
        if let Ok(Some(record)) = read_record(&self.path) {
            if self.controller.as_ref() == Some(&record.controller) {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macos_launch_process::NormalHost;
    use std::net::TcpListener;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn repeated_launch_reuses_the_same_live_host_and_releases_its_record() {
        let root = std::env::temp_dir().join(format!(
            "incodex-launch-owner-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let app = root.join("Official.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(app.join("Contents/Info.plist"), "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>Host</string></dict></plist>").unwrap();
        symlink(
            std::env::current_exe().unwrap(),
            app.join("Contents/MacOS/Host"),
        )
        .unwrap();
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let host = NormalHost::current_process_for_test(port).unwrap();
        host.wait_for_test_listener_owner(port).unwrap();
        let LaunchClaim::Primary(mut owner) = claim(&root, &app).unwrap() else {
            panic!("first entry did not own launch");
        };
        owner.publish(&host).unwrap();
        let LaunchClaim::Existing(existing) = claim(&root, &app).unwrap() else {
            panic!("second entry created another owner");
        };
        assert_eq!(existing, host);
        let original = std::fs::read(&owner.path).unwrap();
        let mut changed: serde_json::Value = serde_json::from_slice(&original).unwrap();
        let seconds = changed["controller"]["start_identity"]["seconds"]
            .as_u64()
            .unwrap();
        changed["controller"]["start_identity"]["seconds"] = serde_json::json!(seconds + 1);
        std::fs::write(&owner.path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(
            claim(&root, &app).is_err(),
            "stale CLI identity was accepted"
        );
        assert!(host.is_alive().unwrap());
        std::fs::write(&owner.path, &original).unwrap();
        let install_lock =
            acquire_target_lock(&root, &app, "isolated install-lock proof", None).unwrap();
        drop(install_lock);
        drop(owner);
        assert!(matches!(
            claim(&root, &app).unwrap(),
            LaunchClaim::Primary(_)
        ));
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
#[path = "macos_launch_profile_tests.rs"]
mod profile_tests;
