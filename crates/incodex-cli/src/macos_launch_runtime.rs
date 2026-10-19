/*
 * [INPUT]: 依赖经 Runtime Bundle 验证的 release 与 CDP renderer/action ACK。
 * [OUTPUT]: 提供普通宿主代际读取、kqueue 通知、事务刷新与请求 release 校验。
 * [POS]: macOS launch 的 Runtime 适配器；代际提交与回滚由 UiUpdate 独占。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::runtime_ui_update::{UiGeneration, UiUpdate};

const MAX_RUNTIME_SOURCE_BYTES: u64 = 2 * 1024 * 1024;
const INJECTOR: &str = "incodex-inject.js";
const MAIN_ACTIONS: &str = "incodex-main-actions.cjs";

/// macOS Runtime 选择器与文件事件适配器。
///
/// 代际兼容、ACK 顺序和回滚始终由共享 `UiUpdate` 执行。
pub(crate) struct LaunchRuntime {
    root: PathBuf,
    updates: UiUpdate,
    watcher: RuntimeWatcher,
}

impl LaunchRuntime {
    /// 先订阅 Runtime 目录，再读取初始选择，避免漏掉启动期发布。
    pub(crate) fn new(root: &Path) -> Result<Self, String> {
        let runtime_dir = root.join("runtime");
        let watcher = RuntimeWatcher::new(&runtime_dir)?;
        let active = read_selected_generation(root)?;
        Ok(Self {
            root: root.to_path_buf(),
            updates: UiUpdate::new(active),
            watcher,
        })
    }

    pub(crate) fn status(&self) -> serde_json::Value {
        self.updates.snapshot()
    }

    pub(crate) fn active(&self) -> &UiGeneration {
        self.updates.active()
    }

    /// 仅在 kqueue 报告目录变化时由调用方触发刷新。
    pub(crate) fn refresh(
        &mut self,
        apply: impl FnMut(&UiGeneration) -> Result<bool, String>,
        commit: impl FnMut(&UiGeneration) -> Result<bool, String>,
    ) -> Result<(), String> {
        let candidate = match read_selected_generation(&self.root) {
            Ok(candidate) => candidate,
            Err(error) => {
                self.updates.preparation_failed();
                return Err(error);
            }
        };

        // 同代且已有 ACK 时，UiUpdate 会短路。先复验选择，避免短路掩盖换代。
        match read_selected_generation(&self.root) {
            Ok(selected) if selected == candidate => {}
            Ok(_) => {
                self.updates.preparation_failed();
                return Err("Runtime selection changed before activation".into());
            }
            Err(error) => {
                self.updates.preparation_failed();
                return Err(error);
            }
        }

        let root = &self.root;
        self.updates.activate_with_commit(
            candidate,
            |expected| read_selected_generation(root).map(|selected| selected == *expected),
            apply,
            commit,
        )?;

        // UiUpdate 保留不兼容代际并标记 restart-required，不会调用宿主回调。
        if self.updates.phase() == "restart-required"
            || self.updates.snapshot()["restartRequired"] == true
        {
            return Err("Runtime changed non-hot-swappable assets; restart required".into());
        }
        Ok(())
    }

    /// 新 document 必须重新获得 renderer 与 action 的真实 ACK。
    pub(crate) fn renderer_invalidated(&mut self) {
        self.updates.renderer_invalidated();
    }

    pub(crate) fn validate_request_release(&self, requested: Option<&str>) -> Result<(), String> {
        self.updates.request_release(requested).map(|_| ())
    }

    /// 阻塞等待目录事件；超时只服务宿主生命周期，不扫描 Runtime。
    pub(crate) fn wait_for_change(&mut self, timeout: Duration) -> Result<bool, String> {
        match self.watcher.wait(timeout) {
            Ok(changed) => Ok(changed),
            Err(error) => {
                self.updates.notification_failed();
                Err(error)
            }
        }
    }
}

fn read_selected_generation(user_root: &Path) -> Result<UiGeneration, String> {
    let deployed =
        incodex_runtime_bundle::inspect_deployed(user_root)?.ok_or("Runtime is not published")?;
    let release_dir = user_root.join("runtime").join(&deployed.release);
    let source = read_verified_source(&release_dir, INJECTOR, &deployed.files)?;
    let action_source = read_verified_source(&release_dir, MAIN_ACTIONS, &deployed.files)?;

    // inspect_deployed 验证完整清单；复读当前选择，拒绝读取期间被替换的 release。
    let selected_again = incodex_runtime_bundle::inspect_deployed(user_root)?
        .ok_or("Runtime selection disappeared")?;
    if selected_again != deployed {
        return Err("Runtime selection changed while reading artifacts".into());
    }

    Ok(UiGeneration {
        release: deployed.release,
        source,
        action_source,
        files: deployed.files,
    })
}

fn read_verified_source(
    release_dir: &Path,
    name: &str,
    files: &std::collections::BTreeMap<String, String>,
) -> Result<String, String> {
    let expected = files
        .get(name)
        .ok_or_else(|| format!("Runtime manifest is missing {name}"))?;
    let path = release_dir.join(name);
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(&path)
        .map_err(|error| format!("cannot open Runtime {name}: {error}"))?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err(format!("Runtime {name} is not a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_RUNTIME_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read Runtime {name}: {error}"))?;
    if bytes.len() as u64 > MAX_RUNTIME_SOURCE_BYTES {
        return Err(format!("Runtime {name} exceeds the size limit"));
    }
    if crate::macos_update_assets::sha256_hex(&bytes) != expected.as_str() {
        return Err(format!("Runtime {name} hash mismatch"));
    }
    String::from_utf8(bytes).map_err(|_| format!("Runtime {name} is not UTF-8"))
}

struct RuntimeWatcher {
    queue: File,
    directory: File,
}

impl RuntimeWatcher {
    fn new(runtime_dir: &Path) -> Result<Self, String> {
        let queue_fd = unsafe { libc::kqueue() };
        if queue_fd < 0 {
            return Err(format!(
                "cannot create Runtime kqueue: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: kqueue returned a new owned descriptor.
        let queue = unsafe { File::from_raw_fd(queue_fd) };
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_EVTONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_DIRECTORY)
            .open(runtime_dir)
            .map_err(|error| format!("cannot watch Runtime directory: {error}"))?;
        let event = libc::kevent {
            ident: directory.as_raw_fd() as libc::uintptr_t,
            filter: libc::EVFILT_VNODE,
            flags: libc::EV_ADD | libc::EV_CLEAR,
            fflags: libc::NOTE_WRITE
                | libc::NOTE_EXTEND
                | libc::NOTE_ATTRIB
                | libc::NOTE_LINK
                | libc::NOTE_RENAME
                | libc::NOTE_DELETE
                | libc::NOTE_REVOKE,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        let registered = unsafe {
            libc::kevent(
                queue.as_raw_fd(),
                &event,
                1,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        };
        if registered < 0 {
            return Err(format!(
                "cannot watch Runtime directory: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self { queue, directory })
    }

    fn wait(&self, timeout: Duration) -> Result<bool, String> {
        let seconds = timeout.as_secs().min(libc::time_t::MAX as u64);
        let limit = libc::timespec {
            tv_sec: seconds as libc::time_t,
            tv_nsec: timeout.subsec_nanos() as libc::c_long,
        };
        let mut event = std::mem::MaybeUninit::<libc::kevent>::uninit();
        let count = loop {
            let count = unsafe {
                libc::kevent(
                    self.queue.as_raw_fd(),
                    std::ptr::null(),
                    0,
                    event.as_mut_ptr(),
                    1,
                    &limit,
                )
            };
            if count >= 0 {
                break count;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(format!("Runtime kqueue wait failed: {error}"));
            }
        };
        if count == 0 {
            return Ok(false);
        }
        // SAFETY: kevent returned one initialized event.
        let event = unsafe { event.assume_init() };
        if event.flags & libc::EV_ERROR != 0 {
            let error_code = event.data;
            return Err(format!("Runtime kqueue event failed: {error_code}"));
        }
        if event.flags & libc::EV_EOF != 0
            || event.fflags & (libc::NOTE_RENAME | libc::NOTE_DELETE | libc::NOTE_REVOKE) != 0
        {
            return Err("Runtime directory identity changed; restart required".into());
        }
        // The directory is intentionally held open for the lifetime of the watch.
        let _directory_fd = self.directory.as_raw_fd();
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "incodex-launch-runtime-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            incodex_runtime_bundle::ensure_current(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn publish_variant(root: &Path, changed: &[&str]) -> String {
        let deployed = incodex_runtime_bundle::inspect_deployed(root)
            .unwrap()
            .unwrap();
        let runtime_dir = root.join("runtime");
        let old_release_dir = runtime_dir.join(&deployed.release);
        let mut artifacts = BTreeMap::new();
        for name in deployed.files.keys() {
            artifacts.insert(name.clone(), fs::read(old_release_dir.join(name)).unwrap());
        }
        for name in changed {
            artifacts.get_mut(*name).unwrap().extend_from_slice(b"\n");
        }

        let mut hashes = BTreeMap::new();
        for (name, bytes) in &artifacts {
            hashes.insert(name.clone(), crate::macos_update_assets::sha256_hex(bytes));
        }
        let manifest_path = old_release_dir.join("runtime-manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(manifest_path).unwrap()).unwrap();
        for (name, hash) in &hashes {
            manifest["files"][name] = serde_json::Value::String(hash.clone());
        }
        let manifest_bytes = serde_json::to_vec_pretty(&manifest).unwrap();
        let manifest_hash = crate::macos_update_assets::sha256_hex(&manifest_bytes);
        let release_name = format!("{}-{manifest_hash}", deployed.version);
        let release_dir = runtime_dir.join("releases").join(&release_name);
        fs::create_dir(&release_dir).unwrap();
        fs::set_permissions(&release_dir, fs::Permissions::from_mode(0o700)).unwrap();
        for (name, bytes) in artifacts {
            let destination = release_dir.join(&name);
            fs::write(&destination, bytes).unwrap();
            let mode = fs::metadata(old_release_dir.join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            fs::set_permissions(&destination, fs::Permissions::from_mode(mode)).unwrap();
        }
        let manifest_path = release_dir.join("runtime-manifest.json");
        fs::write(&manifest_path, manifest_bytes).unwrap();
        fs::set_permissions(&manifest_path, fs::Permissions::from_mode(0o600)).unwrap();

        let current_path = runtime_dir.join("current.json");
        let mut pointer: serde_json::Value =
            serde_json::from_slice(&fs::read(&current_path).unwrap()).unwrap();
        pointer["release"] = serde_json::Value::String(format!("releases/{release_name}"));
        pointer["manifestSha256"] = serde_json::Value::String(manifest_hash);
        pointer["files"] = serde_json::to_value(hashes).unwrap();
        let temporary = runtime_dir.join(format!(
            ".current-test-{}-{}.json",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&temporary, serde_json::to_vec_pretty(&pointer).unwrap()).unwrap();
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600)).unwrap();
        fs::rename(temporary, current_path).unwrap();
        incodex_runtime_bundle::inspect_deployed(root)
            .unwrap()
            .unwrap()
            .release
    }

    #[test]
    fn initial_and_new_document_require_real_acknowledgements() {
        let root = TempRoot::new();
        let mut runtime = LaunchRuntime::new(&root.0).unwrap();
        let release = runtime.active().release.clone();
        assert!(runtime.validate_request_release(None).is_ok());
        assert!(runtime
            .validate_request_release(Some("unobserved"))
            .is_err());

        let mut applied = 0;
        let mut committed = 0;
        runtime
            .refresh(
                |_| {
                    applied += 1;
                    Ok(true)
                },
                |_| {
                    committed += 1;
                    Ok(true)
                },
            )
            .unwrap();
        runtime.renderer_invalidated();
        runtime
            .refresh(
                |_| {
                    applied += 1;
                    Ok(true)
                },
                |_| {
                    committed += 1;
                    Ok(true)
                },
            )
            .unwrap();
        assert_eq!(runtime.active().release, release);
        assert_eq!((applied, committed), (2, 2));
    }

    #[test]
    fn selected_ui_and_action_generation_activates_through_shared_update() {
        let root = TempRoot::new();
        let mut runtime = LaunchRuntime::new(&root.0).unwrap();
        let old_release = runtime.active().release.clone();
        let deployed = incodex_runtime_bundle::inspect_deployed(&root.0)
            .unwrap()
            .unwrap();
        let new_release = publish_variant(&root.0, &[INJECTOR, MAIN_ACTIONS]);
        runtime.wait_for_change(Duration::from_secs(1)).unwrap();
        let mut applied = Vec::new();
        let mut committed = Vec::new();
        runtime
            .refresh(
                |generation| {
                    applied.push(generation.release.clone());
                    Ok(true)
                },
                |generation| {
                    committed.push(generation.release.clone());
                    Ok(true)
                },
            )
            .unwrap();
        assert_eq!(runtime.active().release, new_release);
        assert_eq!(applied.as_slice(), std::slice::from_ref(&new_release));
        assert_eq!(committed.as_slice(), std::slice::from_ref(&new_release));
        assert!(runtime.validate_request_release(Some(&old_release)).is_ok());
        assert!(runtime
            .validate_request_release(Some(&deployed.release))
            .is_ok());
    }

    #[test]
    fn non_ui_runtime_change_requires_restart_without_host_callbacks() {
        let root = TempRoot::new();
        let mut runtime = LaunchRuntime::new(&root.0).unwrap();
        let old_release = runtime.active().release.clone();
        let deployed = incodex_runtime_bundle::inspect_deployed(&root.0)
            .unwrap()
            .unwrap();
        let other = deployed
            .files
            .keys()
            .find(|name| name.as_str() != INJECTOR && name.as_str() != MAIN_ACTIONS)
            .unwrap()
            .clone();
        let new_release = publish_variant(&root.0, &[other.as_str()]);
        let callbacks = Cell::new(0);
        let error = runtime
            .refresh(
                |_| {
                    callbacks.set(callbacks.get() + 1);
                    Ok(true)
                },
                |_| {
                    callbacks.set(callbacks.get() + 1);
                    Ok(true)
                },
            )
            .unwrap_err();
        assert!(error.contains("restart required"), "{error}");
        assert_eq!(callbacks.get(), 0);
        assert_eq!(runtime.active().release, old_release);
        assert!(runtime
            .validate_request_release(Some(&new_release))
            .is_err());
    }

    #[test]
    fn non_ui_change_after_failed_rollback_still_requires_restart() {
        let root = TempRoot::new();
        let mut runtime = LaunchRuntime::new(&root.0).unwrap();
        let active_release = runtime.active().release.clone();
        publish_variant(&root.0, &[INJECTOR, MAIN_ACTIONS]);

        assert!(runtime.refresh(|_| Ok(true), |_| Ok(false)).is_err());
        assert_eq!(runtime.status()["phase"], "rollback-failed");
        assert_eq!(runtime.status()["restartRequired"], false);

        let deployed = incodex_runtime_bundle::inspect_deployed(&root.0)
            .unwrap()
            .unwrap();
        let other = deployed
            .files
            .keys()
            .find(|name| name.as_str() != INJECTOR && name.as_str() != MAIN_ACTIONS)
            .unwrap()
            .clone();
        let candidate_release = publish_variant(&root.0, &[other.as_str()]);
        let callbacks = Cell::new(0);

        let error = runtime
            .refresh(
                |_| {
                    callbacks.set(callbacks.get() + 1);
                    Ok(true)
                },
                |_| {
                    callbacks.set(callbacks.get() + 1);
                    Ok(true)
                },
            )
            .unwrap_err();

        assert!(error.contains("restart required"), "{error}");
        assert_eq!(callbacks.get(), 0);
        assert_eq!(runtime.active().release, active_release);
        assert!(runtime
            .validate_request_release(Some(&candidate_release))
            .is_err());
        assert_eq!(runtime.status()["phase"], "rollback-failed");
        assert_eq!(runtime.status()["restartRequired"], true);
    }

    #[test]
    fn lost_action_ack_rolls_both_layers_back_to_active_generation() {
        let root = TempRoot::new();
        let mut runtime = LaunchRuntime::new(&root.0).unwrap();
        let old_release = runtime.active().release.clone();
        let new_release = publish_variant(&root.0, &[INJECTOR, MAIN_ACTIONS]);
        let mut applied = Vec::new();
        let mut committed = Vec::new();
        let result = runtime.refresh(
            |generation| {
                applied.push(generation.release.clone());
                Ok(true)
            },
            |generation| {
                committed.push(generation.release.clone());
                Ok(generation.release == old_release)
            },
        );
        assert!(result.is_err());
        assert_eq!(runtime.active().release, old_release);
        assert_eq!(applied, [new_release.clone(), old_release.clone()]);
        assert_eq!(committed, [new_release, old_release.clone()]);
        assert_eq!(runtime.updates.snapshot()["rollback"], "succeeded");
        assert!(runtime.validate_request_release(Some(&old_release)).is_ok());
    }
}
