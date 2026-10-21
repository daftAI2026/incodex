/*
 * [INPUT]: 依赖既有目标锁、私有原子文件与NormalHost进程/端口证据。
 * [OUTPUT]: 串行化普通宿主启动；重复入口仅重激活已捕获的同一宿主。
 * [POS]: launch的App作用域owner记录；不持有会话或增加IPC服务。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

use crate::macos_launch_process::{NormalHost, ProcessOwner};
use crate::macos_update_assets::{ensure_private_dir, read_regular_file, write_private_atomic};
use incodex_transaction::{acquire_target_lock, lock_path_for, TargetLock};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
    _root: &Path,
    _profile: &Path,
) -> Result<Option<(PathBuf, NormalHost)>, String> {
    Ok(None)
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
