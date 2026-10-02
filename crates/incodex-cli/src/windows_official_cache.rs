// 官方资源准备发生在政策 RPC 的计时内；在精确挂起进程恢复前准备同一内容寻址缓存。
// 仅新建本代缓存，不删除/覆盖官方已有目录，不修改 Store 包或组织安全校验。
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_READ,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath};

use crate::windows_file::{ensure_regular_file, sha256_file};

const KEY_FILES: [&str; 3] = ["manifest.json", "bin/node.exe", "bin/node_repl.exe"];
const CACHE_KEY_LENGTH: usize = 16; // 官方内容寻址协议的十六进制摘要前缀，不是版本号。
const COPY_BUFFER_BYTES: usize = 64 * 1024;

fn with_pinned_directory<T>(
    path: &Path,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let _pin = pin_path(path, true)?;
    operation()
}

fn pin_path(path: &Path, directory: bool) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT
                | if directory {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
        )
        .open(path)
        .map_err(|error| format!("cannot pin runtime cache path {}: {error}", path.display()))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || if directory {
            !metadata.is_dir()
        } else {
            !metadata.is_file()
        }
    {
        return Err("runtime cache path is not a normal pinned entry".into());
    }
    Ok(file)
}

fn pin_ancestry(path: &Path) -> Result<Vec<File>, String> {
    incodex_core::windows_path::require_local_disk_absolute(path, "runtime cache ancestry")?;
    // 根到叶持有不共享写入/删除的目录句柄，读写期间不能替换为 junction。
    path.ancestors()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|ancestor| pin_path(ancestor, true))
        .collect()
}

pub(crate) fn prepare_then_resume(
    full_name: &str,
    user_root: &Path,
    resume: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let began = std::time::Instant::now();
    prepare_then_resume_with(
        || prepare_before_resume(full_name),
        |result| {
            let (phase, detail) = match result {
                Ok(path) => (
                    "official-cache-ready",
                    format!(
                        "key={}, elapsedMs={}",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        began.elapsed().as_millis()
                    ),
                ),
                Err(error) => ("official-cache-deferred", error.to_string()),
            };
            // 复用单个 4 KiB/8 事件 UI 日志；准备失败不能终结官方进程。
            let _ =
                crate::windows_update_observer_log::installed_ui_status(user_root, phase, &detail);
        },
        resume,
    )
}

fn prepare_then_resume_with(
    prepare: impl FnOnce() -> Result<PathBuf, String>,
    report: impl FnOnce(&Result<PathBuf, String>),
    resume: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    report(&prepare());
    resume()
}

pub(crate) fn prepare_before_resume(full_name: &str) -> Result<PathBuf, String> {
    let _apartment = crate::windows_update_repair::WindowsRuntimeApartment::initialize()?;
    let app = crate::windows_package_native::registered_codex_package(full_name)?;
    let source = app
        .executable
        .parent()
        .ok_or("official Codex executable has no parent")?
        .join("resources/cua_node");
    let cache = local_app_data()?.join("OpenAI/Codex/runtimes/cua_node");
    prepare_cache(&source, &cache)
}

fn local_app_data() -> Result<PathBuf, String> {
    let mut raw = std::ptr::null_mut();
    let result =
        unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, 0, std::ptr::null_mut(), &mut raw) };
    if result < 0 || raw.is_null() {
        if !raw.is_null() {
            unsafe { CoTaskMemFree(raw.cast()) };
        }
        return Err(format!(
            "cannot resolve current-user LocalAppData: 0x{:08x}",
            result as u32
        ));
    }
    let mut length = 0;
    unsafe {
        while *raw.add(length) != 0 {
            length += 1;
        }
    }
    let path = PathBuf::from(std::ffi::OsString::from_wide(unsafe {
        std::slice::from_raw_parts(raw, length)
    }));
    unsafe { CoTaskMemFree(raw.cast()) };
    incodex_core::windows_path::require_local_disk_absolute(&path, "official runtime cache root")?;
    Ok(path)
}

fn prepare_cache(source: &Path, cache_root: &Path) -> Result<PathBuf, String> {
    prepare_cache_with(source, cache_root, copy_tree)
}

fn prepare_cache_with(
    source: &Path,
    cache_root: &Path,
    copy: impl FnOnce(&Path, &Path) -> Result<(), String>,
) -> Result<PathBuf, String> {
    incodex_core::windows_path::reject_reparse_ancestors(source)?;
    require_directory(source)?;
    validate_manifest(source)?;
    let fingerprints = key_fingerprints(source)?;
    let mut hash = Sha256::new();
    for (name, fingerprint) in KEY_FILES.iter().zip(&fingerprints) {
        hash.update(name.as_bytes());
        hash.update(b"\0");
        hash.update(fingerprint.as_bytes());
        hash.update(b"\0");
    }
    let digest = hex_digest(hash);
    let key = &digest[..CACHE_KEY_LENGTH];
    create_cache_ancestry(cache_root)?;
    let _cache_ancestry = pin_ancestry(cache_root)?;
    let destination = cache_root.join(key);
    match fs::symlink_metadata(&destination) {
        Ok(_) => {
            validate_cached_runtime(&destination, &fingerprints)?;
            return Ok(destination);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect official runtime cache: {error}")),
    }
    let staging = cache_root.join(format!(
        ".staging-{key}-incodex-{}",
        crate::windows_install_state::random_registration_id()?
    ));
    fs::create_dir(&staging)
        .map_err(|error| format!("cannot create runtime cache staging: {error}"))?;
    incodex_core::windows_session::ensure_private_windows_dir(&staging)?;
    let result = (|| {
        // 拷贝用读写流，不继承 WindowsApps EFS 元数据，也不反复尝试慢速 CopyFile。
        copy(source, &staging)?;
        validate_tree_types(&staging)?;
        validate_cached_runtime(&staging, &fingerprints)?;
        if key_fingerprints(source)? != fingerprints {
            return Err("official runtime source changed during preparation".into());
        }
        // 复制期间锁住命名空间；Windows 原子改名需释放祖先读锁。
        // 暂存目录只授权当前用户，改名前再次复核，不用扩大分享模式来放开复制边界。
        drop(_cache_ancestry);
        incodex_core::windows_path::reject_reparse_ancestors(&staging)?;
        validate_tree_types(&staging)?;
        let published = fs::rename(&staging, &destination);
        #[cfg(test)]
        tests::after_publish_attempt(&published);
        match published {
            Ok(()) => Ok(destination.clone()),
            Err(error) => {
                // 并发官方/另一个 helper 可能已发布同代；只复核，不覆盖任何已有目录。
                validate_cached_runtime(&destination, &fingerprints)
                    .map_err(|probe| format!("cannot publish runtime cache: {error}; {probe}"))?;
                Ok(destination.clone())
            }
        }
    })();
    if staging.exists() {
        // 只清理本次创建的随机目录；祖先与整棵树复核后才能递归移除。
        if incodex_core::windows_path::reject_reparse_ancestors(&staging).is_ok()
            && validate_tree_types(&staging).is_ok()
        {
            fs::remove_dir_all(&staging)
                .map_err(|error| format!("runtime cache staging cleanup failed: {error}"))?;
        } else {
            return Err("runtime cache staging changed; retained for inspection".into());
        }
    }
    result
}

fn create_cache_ancestry(path: &Path) -> Result<(), String> {
    incodex_core::windows_path::require_local_disk_absolute(path, "official runtime cache")?;
    if path.exists() {
        incodex_core::windows_path::reject_reparse_ancestors(path)?;
        return require_directory(path);
    }
    create_cache_ancestry(path.parent().ok_or("official cache has no parent")?)?;
    let _parent = pin_ancestry(path.parent().ok_or("official cache has no parent")?)?;
    fs::create_dir(path)
        .or_else(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                Ok(())
            } else {
                Err(error)
            }
        })
        .map_err(|error| format!("cannot create official runtime cache: {error}"))?;
    incodex_core::windows_path::reject_reparse_ancestors(path)?;
    require_directory(path)
}

fn validate_manifest(source: &Path) -> Result<(), String> {
    let path = source.join("manifest.json");
    ensure_regular_file(&path, "official runtime manifest")?;
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(path)
            .map_err(|error| format!("cannot read official runtime manifest: {error}"))?,
    )
    .map_err(|error| format!("invalid official runtime manifest: {error}"))?;
    for (name, value) in [
        ("platform", "windows"),
        ("arch", "x64"),
        ("node_path", KEY_FILES[1]),
        ("node_repl_path", KEY_FILES[2]),
        ("node_modules", "bin/node_modules"),
    ] {
        if manifest[name].as_str() != Some(value) {
            return Err(format!(
                "unrecognized official runtime manifest field: {name}"
            ));
        }
    }
    require_directory(&source.join("bin/node_modules"))
}

fn key_fingerprints(root: &Path) -> Result<Vec<String>, String> {
    // Store 父目录允许读取包内文件，但不授予 WindowsApps 列目录权限。
    // 祖先只检查重解析点；持有运行时根和内部目录，缓存外层由发布事务固定。
    incodex_core::windows_path::reject_reparse_ancestors(root)?;
    let _root = pin_path(root, true)?;
    KEY_FILES
        .iter()
        .map(|name| {
            let path = root.join(name);
            let parent = path.parent().ok_or("runtime key file has no parent")?;
            let _parent = pin_path(parent, true)?;
            let _key_file = pin_path(&path, false)?;
            ensure_regular_file(&path, "official runtime key file")?;
            sha256_file(&path)
        })
        .collect()
}

fn validate_cached_runtime(root: &Path, fingerprints: &[String]) -> Result<(), String> {
    incodex_core::windows_path::reject_reparse_ancestors(root)?;
    require_directory(root)?;
    if key_fingerprints(root)? != fingerprints {
        return Err(
            "existing official runtime cache has different content; not overwritten".into(),
        );
    }
    let modules = root.join("bin/node_modules");
    incodex_core::windows_path::reject_reparse_ancestors(&modules)?;
    require_directory(&modules)
}

fn require_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect runtime directory: {error}"))?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err("official runtime directory is not a normal directory".into());
    }
    Ok(())
}

fn validate_tree_types(root: &Path) -> Result<(), String> {
    require_directory(root)?;
    for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if fs::symlink_metadata(&path)
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            validate_tree_types(&path)?;
        } else {
            ensure_regular_file(&path, "runtime cache file")?;
        }
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    require_directory(source)?;
    with_pinned_directory(destination, || {
        for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let input = entry.path();
            let output = destination.join(entry.file_name());
            let metadata = fs::symlink_metadata(&input).map_err(|error| error.to_string())?;
            if metadata.is_dir() {
                require_directory(&input)?;
                fs::create_dir(&output).map_err(|error| error.to_string())?;
                copy_tree(&input, &output)?;
            } else {
                ensure_regular_file(&input, "official runtime source file")?;
                let mut read = File::open(&input).map_err(|error| error.to_string())?;
                let mut write = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .share_mode(FILE_SHARE_READ)
                    .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                    .open(&output)
                    .map_err(|error| error.to_string())?;
                let mut hash = Sha256::new();
                let mut buffer = [0u8; COPY_BUFFER_BYTES];
                loop {
                    let count = read.read(&mut buffer).map_err(|error| error.to_string())?;
                    if count == 0 {
                        break;
                    }
                    write
                        .write_all(&buffer[..count])
                        .map_err(|error| error.to_string())?;
                    hash.update(&buffer[..count]);
                }
                write.flush().map_err(|error| error.to_string())?;
                drop(write);
                if sha256_file(&output)? != hex_digest(hash) {
                    return Err("copied official runtime file hash mismatch".into());
                }
            }
        }
        Ok(())
    })
}

fn hex_digest(hash: Sha256) -> String {
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{prepare_cache, prepare_cache_with, prepare_then_resume_with};

    type PublishObserver = Box<dyn FnOnce(Option<i32>)>;
    thread_local! {
        static PUBLISH_OBSERVER: std::cell::RefCell<Option<PublishObserver>> =
            const { std::cell::RefCell::new(None) };
    }

    pub(super) fn after_publish_attempt(result: &std::io::Result<()>) {
        PUBLISH_OBSERVER.with(|slot| {
            if let Some(observer) = slot.borrow_mut().take() {
                observer(result.as_ref().err().and_then(std::io::Error::raw_os_error));
            }
        });
    }

    struct Fixture {
        root: PathBuf,
        source: PathBuf,
        cache: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "incodex-official-cache-{}",
                crate::windows_install_state::random_registration_id().unwrap()
            ));
            let source = root.join("source");
            let cache = root.join("cache");
            fs::create_dir_all(source.join("bin/node_modules/fixture")).unwrap();
            fs::create_dir(&cache).unwrap();
            fs::write(
                source.join("manifest.json"),
                br#"{"platform":"windows","arch":"x64","node_path":"bin/node.exe","node_repl_path":"bin/node_repl.exe","node_modules":"bin/node_modules"}"#,
            ).unwrap();
            fs::write(source.join("bin/node.exe"), b"node fixture").unwrap();
            fs::write(source.join("bin/node_repl.exe"), b"repl fixture").unwrap();
            fs::write(
                source.join("bin/node_modules/fixture/index.js"),
                b"module fixture",
            )
            .unwrap();
            Self {
                root,
                source,
                cache,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn cold_official_runtime_is_ready_before_the_policy_clock_starts() {
        let fixture = Fixture::new();
        let prepared = prepare_cache(&fixture.source, &fixture.cache)
            .expect("cold runtime preparation must precede official bootstrap");
        assert_eq!(prepared.parent(), Some(fixture.cache.as_path()));
        assert_eq!(prepared.file_name().unwrap().to_string_lossy().len(), 16);
        for name in [
            "manifest.json",
            "bin/node.exe",
            "bin/node_repl.exe",
            "bin/node_modules/fixture/index.js",
        ] {
            assert_eq!(
                fs::read(prepared.join(name)).unwrap(),
                fs::read(fixture.source.join(name)).unwrap()
            );
        }
        assert_eq!(
            fs::read_dir(&fixture.cache).unwrap().count(),
            1,
            "only the committed cache may remain"
        );
    }

    #[test]
    fn warm_official_cache_is_reused_without_overwriting_its_contents() {
        let fixture = Fixture::new();
        let first = prepare_cache(&fixture.source, &fixture.cache).unwrap();
        fs::write(first.join("official-owned-marker"), b"preserve").unwrap();
        let second = prepare_cache(&fixture.source, &fixture.cache).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            fs::read(second.join("official-owned-marker")).unwrap(),
            b"preserve"
        );
    }

    #[test]
    fn content_change_uses_a_new_cache_key_without_removing_the_old_generation() {
        let fixture = Fixture::new();
        let first = prepare_cache(&fixture.source, &fixture.cache).unwrap();
        fs::write(fixture.source.join("bin/node.exe"), b"new node fixture").unwrap();
        let second = prepare_cache(&fixture.source, &fixture.cache).unwrap();
        assert_ne!(first, second);
        assert!(first.exists());
        assert_eq!(
            fs::read(second.join("bin/node.exe")).unwrap(),
            b"new node fixture"
        );
    }

    #[test]
    fn policy_bootstrap_cannot_start_before_the_cache_is_committed() {
        let fixture = Fixture::new();
        let path = std::cell::RefCell::new(None);
        let order = std::cell::RefCell::new(Vec::new());
        prepare_then_resume_with(
            || {
                order.borrow_mut().push("prepare");
                prepare_cache(&fixture.source, &fixture.cache)
            },
            |result| {
                order.borrow_mut().push("report");
                *path.borrow_mut() = Some(result.as_ref().unwrap().clone());
            },
            || {
                order.borrow_mut().push("resume");
                assert!(path
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .join("bin/node.exe")
                    .is_file());
                assert_eq!(fs::read_dir(&fixture.cache).unwrap().count(), 1);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*order.borrow(), ["prepare", "report", "resume"]);
    }

    #[test]
    fn failed_cache_preparation_still_resumes_official_and_reports_failure() {
        let reported = std::cell::Cell::new(false);
        let resumed = std::cell::Cell::new(false);
        prepare_then_resume_with(
            || Err("real read failure".into()),
            |result| {
                assert_eq!(result.as_ref().unwrap_err(), "real read failure");
                reported.set(true);
            },
            || {
                assert!(reported.get());
                resumed.set(true);
                Ok(())
            },
        )
        .unwrap();
        assert!(resumed.get());
    }

    #[test]
    fn interrupted_copy_leaves_no_published_cache_and_retry_succeeds() {
        let fixture = Fixture::new();
        let failed = prepare_cache_with(&fixture.source, &fixture.cache, |_, staging| {
            fs::write(staging.join("partial"), b"partial").unwrap();
            Err("interrupted copy".into())
        });
        assert!(failed.unwrap_err().contains("interrupted copy"));
        assert_eq!(fs::read_dir(&fixture.cache).unwrap().count(), 0);
        assert!(prepare_cache(&fixture.source, &fixture.cache)
            .unwrap()
            .is_dir());
    }

    #[test]
    fn corrupted_existing_cache_is_preserved_not_overwritten() {
        let fixture = Fixture::new();
        let cache = prepare_cache(&fixture.source, &fixture.cache).unwrap();
        fs::write(cache.join("bin/node.exe"), b"preserve corrupted evidence").unwrap();
        assert!(prepare_cache(&fixture.source, &fixture.cache).is_err());
        assert_eq!(
            fs::read(cache.join("bin/node.exe")).unwrap(),
            b"preserve corrupted evidence"
        );
        assert_eq!(fs::read_dir(&fixture.cache).unwrap().count(), 1);
    }

    #[test]
    fn concurrent_publishers_share_one_verified_generation() {
        let fixture = Fixture::new();
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let run = || {
                prepare_cache_with(&fixture.source, &fixture.cache, |source, staging| {
                    super::copy_tree(source, staging)?;
                    barrier.wait();
                    Ok(())
                })
                .unwrap()
            };
            let one = scope.spawn(run);
            let two = scope.spawn(run);
            assert_eq!(one.join().unwrap(), two.join().unwrap());
        });
        assert_eq!(fs::read_dir(&fixture.cache).unwrap().count(), 1);
    }

    #[test]
    fn publication_recovers_after_another_publishers_ancestry_pin_is_released() {
        let fixture = Fixture::new();
        let competing_pin = super::pin_ancestry(&fixture.cache).unwrap();
        let (attempt_tx, attempt_rx) = std::sync::mpsc::channel();
        let (released_tx, released_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let publisher = scope.spawn(|| {
                PUBLISH_OBSERVER.with(|slot| {
                    *slot.borrow_mut() = Some(Box::new(move |code| {
                        attempt_tx.send(code).unwrap();
                        released_rx
                            .recv_timeout(std::time::Duration::from_secs(5))
                            .unwrap();
                    }));
                });
                prepare_cache(&fixture.source, &fixture.cache)
            });
            let first_error = attempt_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            drop(competing_pin);
            released_tx.send(()).unwrap();
            let result = publisher.join().unwrap();
            assert_eq!(first_error, Some(32), "real Windows sharing violation");
            let published = result.expect("publication must recover after the competing pin drops");
            assert_eq!(
                fs::read(published.join("bin/node.exe")).unwrap(),
                fs::read(fixture.source.join("bin/node.exe")).unwrap()
            );
        });
        assert_eq!(fs::read_dir(&fixture.cache).unwrap().count(), 1);
    }

    #[test]
    fn cache_fingerprints_reject_a_junction_ancestor_before_reading_keys() {
        let fixture = Fixture::new();
        let linked = fixture.root.join("linked");
        fs::create_dir(&linked).unwrap();
        fs::copy(
            fixture.source.join("manifest.json"),
            linked.join("manifest.json"),
        )
        .unwrap();
        let junction = linked.join("bin");
        let target = fixture.source.join("bin");
        let status = std::process::Command::new(
            crate::windows_system::system_binary_path("cmd.exe").unwrap(),
        )
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&target)
        .output()
        .unwrap();
        assert!(status.status.success(), "test junction must be created");
        let result = super::key_fingerprints(&linked);
        fs::remove_dir(&junction).unwrap();
        assert!(
            result.is_err(),
            "key hashing must not follow an ancestor junction"
        );
    }

    #[test]
    fn destination_is_pinned_against_redirect_during_copy() {
        let fixture = Fixture::new();
        let relocated = fixture.root.join("relocated");
        super::with_pinned_directory(&fixture.cache, || {
            assert!(
                fs::rename(&fixture.cache, &relocated).is_err(),
                "copy must retain a directory handle that blocks path replacement"
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    #[ignore = "copies the real Store resources only into an isolated test cache"]
    fn prepares_current_official_bundle_into_isolated_cache() {
        let app = crate::windows_app::discover_codex_package().unwrap();
        let source = app.executable.parent().unwrap().join("resources/cua_node");
        let fixture = Fixture::new();
        let began = std::time::Instant::now();
        let cache = prepare_cache(&source, &fixture.cache).unwrap();
        println!(
            "official isolated cache key={} elapsedMs={}",
            cache.file_name().unwrap().to_string_lossy(),
            began.elapsed().as_millis()
        );
        assert_eq!(
            super::key_fingerprints(&cache).unwrap(),
            super::key_fingerprints(&source).unwrap()
        );
        assert!(cache.join("bin/node_modules").is_dir());
        assert_eq!(fs::read_dir(&fixture.cache).unwrap().count(), 1);
    }
}
