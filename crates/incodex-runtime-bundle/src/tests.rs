use super::*;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Barrier};

fn scratch(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "incodex-runtime-{label}-{}-{}",
        std::process::id(),
        unique_suffix()
    ))
}

fn runtime_root(user_root: &Path) -> PathBuf {
    user_root.join("runtime")
}

fn manifest_hash() -> String {
    sha256_hex(MANIFEST.as_bytes())
}

fn expected_new_release() -> String {
    format!("{}-{}", runtime_version(), manifest_hash())
}

fn read_current(user_root: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(runtime_root(user_root).join("current.json")).unwrap())
        .unwrap()
}

fn write_json(path: &Path, value: &serde_json::Value) {
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(value).unwrap()),
    )
    .unwrap();
}

fn hash_map_for_bodies(body: &[u8]) -> serde_json::Map<String, serde_json::Value> {
    required_runtime_files()
        .map(|name| {
            (
                name.to_string(),
                serde_json::Value::String(sha256_hex(body)),
            )
        })
        .collect()
}

fn write_old_release(user_root: &Path, release: &str, body: &[u8]) -> serde_json::Value {
    let root = runtime_root(user_root);
    let release_dir = root.join("releases").join(release);
    fs::create_dir_all(&release_dir).unwrap();
    for name in required_runtime_files() {
        let path = release_dir.join(name);
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(FILE_MODE)).unwrap();
    }
    let files = hash_map_for_bodies(body);
    let current = serde_json::json!({
        "schemaVersion": 1,
        "version": runtime_version(),
        "release": format!("releases/{release}"),
        "files": files,
    });
    write_json(&root.join("current.json"), &current);
    current
}

fn verify_current_complete(user_root: &Path) -> serde_json::Value {
    let root = runtime_root(user_root);
    let current = read_current(user_root);
    let release = current["release"].as_str().unwrap();
    let release_dir = root.join(release);
    assert!(release_dir.is_dir(), "release must be a directory");
    for name in required_runtime_files() {
        let expected = current["files"][name].as_str().unwrap();
        let path = release_dir.join(name);
        assert!(path.is_file(), "missing {name}");
        assert_eq!(
            sha256_hex(&fs::read(path).unwrap()),
            expected,
            "hash {name}"
        );
    }
    let has_manifest_hash = current.get("manifestSha256").is_some();
    assert_eq!(
        has_manifest_hash,
        current.get("sourceCommit").is_some(),
        "new pointer fields must be present together"
    );
    if has_manifest_hash {
        let manifest_hash = current["manifestSha256"].as_str().unwrap();
        let manifest_path = release_dir.join("runtime-manifest.json");
        let manifest_bytes = fs::read(&manifest_path).unwrap();
        assert_eq!(sha256_hex(&manifest_bytes), manifest_hash);
        let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes).unwrap();
        assert_eq!(manifest["runtimeVersion"], current["version"]);
        assert_eq!(manifest["sourceCommit"], current["sourceCommit"]);
        for name in required_runtime_files() {
            assert_eq!(manifest["files"][name], current["files"][name]);
        }
        let expected_dir = format!("{}-{}", current["version"].as_str().unwrap(), manifest_hash);
        assert_eq!(release_dir.file_name().unwrap(), expected_dir.as_str());
    }
    current
}

#[test]
fn loader_is_embedded() {
    assert!(loader_source().contains("incodex") || loader_source().len() > 10);
    assert!(!runtime_version().is_empty());
}

#[test]
fn concurrent_publishers_share_one_complete_runtime() {
    let root = scratch("concurrent");
    let barrier = Arc::new(Barrier::new(20));
    let handles: Vec<_> = (0..20)
        .map(|_| {
            let root = root.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                publish(&root)
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap().unwrap();
    }
    let current = verify_current_complete(&root);
    assert_eq!(current["schemaVersion"], 1);
    assert_eq!(
        current["release"],
        format!("releases/{}", expected_new_release())
    );
    let final_release = runtime_root(&root)
        .join("releases")
        .join(expected_new_release());
    assert_eq!(
        fs::read(final_release.join("runtime-manifest.json")).unwrap(),
        MANIFEST.as_bytes()
    );
    assert_eq!(
        fs::metadata(final_release.join("runtime-manifest.json"))
            .unwrap()
            .mode()
            & 0o777,
        FILE_MODE
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn same_version_old_directory_is_never_overwritten() {
    let root = scratch("old-directory");
    let old_release = runtime_root(&root).join("releases").join(runtime_version());
    fs::create_dir_all(&old_release).unwrap();
    let sentinel = old_release.join("incodex-main.cjs");
    fs::write(&sentinel, b"legacy release must survive").unwrap();

    publish(&root).unwrap();

    assert_eq!(fs::read(sentinel).unwrap(), b"legacy release must survive");
    assert!(runtime_root(&root)
        .join("releases")
        .join(expected_new_release())
        .is_dir());
    assert_ne!(
        read_current(&root)["release"],
        format!("releases/{}", runtime_version())
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn identical_content_address_is_reused_without_rewriting() {
    let root = scratch("reuse");
    publish(&root).unwrap();
    let release = runtime_root(&root)
        .join("releases")
        .join(expected_new_release());
    let before = fs::metadata(release.join("incodex-main.cjs")).unwrap();
    let before_manifest = fs::metadata(release.join("runtime-manifest.json")).unwrap();

    publish(&root).unwrap();

    let after = fs::metadata(release.join("incodex-main.cjs")).unwrap();
    let after_manifest = fs::metadata(release.join("runtime-manifest.json")).unwrap();
    assert_eq!(
        before.ino(),
        after.ino(),
        "content-addressed file was rewritten"
    );
    assert_eq!(before_manifest.ino(), after_manifest.ino());
    assert_eq!(
        fs::read_dir(runtime_root(&root).join("releases"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".staging-"))
            .count(),
        0
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn corrupt_content_address_is_rejected_without_repair_or_deletion() {
    let root = scratch("corrupt");
    publish(&root).unwrap();
    let release = runtime_root(&root)
        .join("releases")
        .join(expected_new_release());
    let current_before = fs::read(runtime_root(&root).join("current.json")).unwrap();
    let corrupt = b"corrupt content-addressed release";
    fs::write(release.join("incodex-main.cjs"), corrupt).unwrap();

    let error = publish(&root).expect_err("corrupt content address must not be replaced");

    assert!(error.contains("release") || error.contains("hash"));
    assert_eq!(fs::read(release.join("incodex-main.cjs")).unwrap(), corrupt);
    assert_eq!(
        fs::read(runtime_root(&root).join("current.json")).unwrap(),
        current_before
    );
    fs::remove_dir_all(root).unwrap();
}

#[derive(Clone, Copy)]
enum CrashPhase {
    StagingWrite,
    StagingDirSync,
    FinalRename,
    CurrentRenameBefore,
    CurrentRenameAfter,
}

impl CrashPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::StagingWrite => "staging-write",
            Self::StagingDirSync => "staging-dir-sync",
            Self::FinalRename => "final-rename",
            Self::CurrentRenameBefore => "current-rename-before",
            Self::CurrentRenameAfter => "current-rename-after",
        }
    }
}

fn crash_phase(raw: &str) -> CrashPhase {
    match raw {
        "staging-write" => CrashPhase::StagingWrite,
        "staging-dir-sync" => CrashPhase::StagingDirSync,
        "final-rename" => CrashPhase::FinalRename,
        "current-rename-before" => CrashPhase::CurrentRenameBefore,
        "current-rename-after" => CrashPhase::CurrentRenameAfter,
        other => panic!("unknown crash phase {other}"),
    }
}

#[test]
fn crash_child() {
    let Some(root) = std::env::var_os("INCODEX_RUNTIME_CRASH_ROOT") else {
        return;
    };
    let phase = crash_phase(&std::env::var("INCODEX_RUNTIME_CRASH_PHASE").unwrap());
    let result = publish_with_test_hook(Path::new(&root), move |point| {
        if point == phase.as_str() {
            unsafe {
                libc::raise(libc::SIGKILL);
            }
        }
    });
    panic!("crash hook did not terminate publisher: {result:?}");
}

#[test]
fn sigkill_matrix_exposes_only_complete_old_or_new_pointer() {
    for phase in [
        CrashPhase::StagingWrite,
        CrashPhase::StagingDirSync,
        CrashPhase::FinalRename,
        CrashPhase::CurrentRenameBefore,
        CrashPhase::CurrentRenameAfter,
    ] {
        let root = scratch(phase.as_str());
        let old_release = write_old_release(&root, "old-release", b"old runtime");
        let orphan = runtime_root(&root).join("releases/.staging-orphan");
        fs::create_dir_all(&orphan).unwrap();
        fs::write(orphan.join("left-by-an-interrupted-publish"), b"keep me").unwrap();
        let current_before = fs::read(runtime_root(&root).join("current.json")).unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::crash_child", "--nocapture"])
            .env("INCODEX_RUNTIME_CRASH_ROOT", &root)
            .env("INCODEX_RUNTIME_CRASH_PHASE", phase.as_str())
            .status()
            .unwrap();
        assert_eq!(
            status.signal(),
            Some(libc::SIGKILL),
            "publisher did not receive SIGKILL at {}",
            phase.as_str()
        );

        let current = verify_current_complete(&root);
        let new_release = runtime_root(&root)
            .join("releases")
            .join(expected_new_release());
        let current_is_old =
            fs::read(runtime_root(&root).join("current.json")).unwrap() == current_before;
        if current_is_old {
            assert_eq!(current, old_release);
        } else {
            assert_eq!(current["schemaVersion"], 1);
            assert_eq!(
                current["release"],
                format!("releases/{}", expected_new_release())
            );
            assert!(new_release.is_dir());
        }
        assert!(runtime_root(&root).join("releases/old-release").is_dir());
        assert!(orphan.join("left-by-an-interrupted-publish").is_file());
        if matches!(phase, CrashPhase::StagingWrite | CrashPhase::StagingDirSync) {
            assert!(
                !new_release.exists(),
                "final release appeared before final rename"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}

fn write_loader_fixture(
    user_root: &Path,
    new_contract: bool,
    invalid_manifest: bool,
) -> (PathBuf, PathBuf, PathBuf) {
    let home = user_root.parent().unwrap().to_path_buf();
    let app = home.join("App");
    let runtime = runtime_root(user_root);
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("incodex-loader.cjs"), loader_source()).unwrap();
    fs::write(
        app.join("package.json"),
        r#"{"__incodex":{"originalMain":"official.cjs"}}"#,
    )
    .unwrap();
    fs::write(
        app.join("official.cjs"),
        "require('node:fs').writeFileSync(process.env.INCODEX_OFFICIAL_MARKER, 'official');",
    )
    .unwrap();
    let runtime_body = b"require('node:fs').writeFileSync(process.env.INCODEX_RUNTIME_MARKER, 'runtime'); module.exports = {};";
    let mut files = serde_json::Map::new();
    for name in required_runtime_files() {
        files.insert(
            name.to_string(),
            serde_json::Value::String(sha256_hex(runtime_body)),
        );
    }
    let manifest = serde_json::json!({
        "runtimeVersion": runtime_version(),
        "sourceCommit": "",
        "files": files,
    });
    let manifest_bytes = format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap());
    let release_name = if new_contract {
        format!("{}-{}", runtime_version(), sha256_hex(manifest_bytes.as_bytes()))
    } else {
        "legacy-release".to_string()
    };
    let release = runtime.join("releases").join(&release_name);
    fs::create_dir_all(&release).unwrap();
    for name in required_runtime_files() {
        fs::write(release.join(name), runtime_body).unwrap();
    }
    if new_contract {
        fs::write(release.join("runtime-manifest.json"), &manifest_bytes).unwrap();
    }
    let mut current = serde_json::json!({
        "schemaVersion": 1,
        "version": runtime_version(),
        "release": format!("releases/{release_name}"),
        "files": files,
    });
    if new_contract {
        current["manifestSha256"] = serde_json::Value::String(if invalid_manifest {
            "0".repeat(64)
        } else {
            sha256_hex(manifest_bytes.as_bytes())
        });
        current["sourceCommit"] = serde_json::Value::String(String::new());
    }
    write_json(&runtime.join("current.json"), &current);
    (home, app.join("incodex-loader.cjs"), release)
}

fn run_loader(home: &Path, loader: &Path, runtime_marker: &Path, official_marker: &Path) {
    let output = Command::new("node")
        .arg(loader)
        .env("HOME", home)
        .env("INCODEX_RUNTIME_MARKER", runtime_marker)
        .env("INCODEX_OFFICIAL_MARKER", official_marker)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "loader stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn loader_uses_real_filesystem_and_accepts_old_pointer_without_manifest_hash() {
    let home_root = scratch("loader-old");
    let root = home_root.join(".incodex");
    let (home, loader, _) = write_loader_fixture(&root, false, false);
    let runtime_marker = home.join("runtime-marker");
    let official_marker = home.join("official-marker");
    run_loader(&home, &loader, &runtime_marker, &official_marker);
    assert_eq!(fs::read_to_string(runtime_marker).unwrap(), "runtime");
    assert_eq!(fs::read_to_string(official_marker).unwrap(), "official");
    fs::remove_dir_all(home_root).unwrap();
}

#[test]
fn loader_verifies_new_manifest_hash_and_fails_open_on_mismatch() {
    let home_root = scratch("loader-new");
    let root = home_root.join(".incodex");
    let (home, loader, _) = write_loader_fixture(&root, true, true);
    let runtime_marker = home.join("runtime-marker");
    let official_marker = home.join("official-marker");
    run_loader(&home, &loader, &runtime_marker, &official_marker);
    assert!(!runtime_marker.exists(), "invalid manifest was loaded");
    assert_eq!(fs::read_to_string(official_marker).unwrap(), "official");
    fs::remove_dir_all(home_root).unwrap();
}

#[test]
fn loader_fails_open_on_new_release_path_mismatch() {
    let home_root = scratch("loader-path");
    let root = home_root.join(".incodex");
    let (home, loader, _) = write_loader_fixture(&root, true, false);
    let current_path = runtime_root(&root).join("current.json");
    let mut current = read_current(&root);
    current["release"] = serde_json::Value::String("releases/not-the-address".into());
    write_json(&current_path, &current);
    let runtime_marker = home.join("runtime-marker");
    let official_marker = home.join("official-marker");
    run_loader(&home, &loader, &runtime_marker, &official_marker);
    assert!(!runtime_marker.exists(), "invalid release path was loaded");
    assert_eq!(fs::read_to_string(official_marker).unwrap(), "official");
    fs::remove_dir_all(home_root).unwrap();
}

#[test]
fn loader_rejects_partial_new_pointer_fields() {
    let home_root = scratch("loader-partial");
    let root = home_root.join(".incodex");
    let (home, loader, _) = write_loader_fixture(&root, false, false);
    let current_path = runtime_root(&root).join("current.json");
    let mut current = read_current(&root);
    current["manifestSha256"] = serde_json::Value::String("0".repeat(64));
    write_json(&current_path, &current);
    let runtime_marker = home.join("runtime-marker");
    let official_marker = home.join("official-marker");
    run_loader(&home, &loader, &runtime_marker, &official_marker);
    assert!(!runtime_marker.exists(), "partial pointer was loaded");
    assert_eq!(fs::read_to_string(official_marker).unwrap(), "official");
    fs::remove_dir_all(home_root).unwrap();
}
