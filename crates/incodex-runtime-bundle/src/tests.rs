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
    sha256_hex(manifest_source().as_bytes())
}

fn expected_new_release() -> String {
    format!("{}-{}", runtime_version(), manifest_hash())
}

fn write_legacy_embedded_release(user_root: &Path, release: &str, version: &str) {
    let root = runtime_root(user_root);
    let release_dir = root.join("releases").join(release);
    fs::create_dir_all(&release_dir).unwrap();
    set_runtime_dir_modes(&root, &release_dir);
    let mut files = serde_json::Map::new();
    for (name, body) in external_files() {
        let path = release_dir.join(name);
        fs::write(&path, body.as_bytes()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(FILE_MODE)).unwrap();
        files.insert(
            (*name).to_string(),
            serde_json::Value::String(sha256_hex(body.as_bytes())),
        );
    }
    let current = serde_json::json!({
        "schemaVersion": 1,
        "version": version,
        "release": format!("releases/{release}"),
        "files": files,
    });
    write_json(&root.join("current.json"), &current);
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
    fs::set_permissions(path, fs::Permissions::from_mode(FILE_MODE)).unwrap();
}

fn set_runtime_dir_modes(root: &Path, release: &Path) {
    for path in [
        root.parent().expect("runtime user root"),
        root,
        &root.join("releases"),
        release,
    ] {
        fs::set_permissions(path, fs::Permissions::from_mode(DIR_MODE)).unwrap();
    }
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
    set_runtime_dir_modes(&root, &release_dir);
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
fn codex_mode_readiness_is_a_required_runtime_file() {
    assert!(required_runtime_files().any(|name| name == "incodex-codex-mode.cjs"));
}

#[test]
fn runtime_identity_is_content_addressed_by_the_canonical_manifest() {
    let identity = runtime_identity().unwrap();
    assert_eq!(identity.version, runtime_version());
    assert_eq!(identity.manifest_sha256, manifest_hash());
}

#[test]
fn embedded_identity_rejects_an_incomplete_or_mismatched_manifest_file_set() {
    let mut manifest = embedded_manifest().unwrap();
    manifest.files.insert(LOADER_NAME.into(), "00".repeat(32));
    assert!(runtime_file_hashes(&manifest)
        .unwrap_err()
        .contains(LOADER_NAME));

    let mut manifest = embedded_manifest().unwrap();
    manifest
        .files
        .insert("unexpected.cjs".into(), "00".repeat(32));
    assert_eq!(
        declared_runtime_file_hashes(&manifest).unwrap_err(),
        "runtime manifest files do not match required artifacts"
    );
}

#[test]
fn inspect_deployed_returns_the_verified_pointer_identity() {
    let root = scratch("inspect");
    let published = publish(&root).unwrap();
    let inspected = inspect_deployed(&root).unwrap().unwrap();

    assert_eq!(inspected.version, published.version);
    assert_eq!(inspected.release, published.release);
    assert_eq!(
        inspected.manifest_sha256.as_deref(),
        Some(manifest_hash().as_str())
    );
    assert_eq!(inspected.files.len(), required_runtime_files().count());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn ensure_current_repairs_insecure_runtime_container_modes() {
    for (label, relative) in [
        ("root", "runtime"),
        ("releases", "runtime/releases"),
        ("pointer", "runtime/current.json"),
    ] {
        let root = scratch(&format!("mode-{label}"));
        publish(&root).unwrap();
        fs::set_permissions(
            root.join(relative),
            fs::Permissions::from_mode(if label == "pointer" { 0o666 } else { 0o777 }),
        )
        .unwrap();

        assert!(inspect_deployed(&root).is_err(), "{label}");
        ensure_current(&root).unwrap();
        assert!(deployed_current_matches_embedded(&root).unwrap(), "{label}");
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn inspect_deployed_rejects_a_non_regular_current_pointer() {
    let root = scratch("pointer-directory");
    publish(&root).unwrap();
    let current = runtime_root(&root).join("current.json");
    fs::remove_file(&current).unwrap();
    fs::create_dir(&current).unwrap();

    let error = inspect_deployed(&root).unwrap_err();
    assert!(
        error.contains("current.json is not a regular file"),
        "{error}"
    );
    assert!(ensure_current(&root).is_err());
    assert_eq!(
        fs::read_dir(runtime_root(&root))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".current.json.tmp-"))
            .count(),
        0,
        "a rejected pointer must not leave a temporary file"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn user_root_symlink_is_never_followed_by_inspection_or_publish() {
    let parent = scratch("user-root-symlink");
    let outside = parent.join("outside");
    let user_root = parent.join(".incodex");
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, &user_root).unwrap();

    assert!(inspect_deployed(&user_root)
        .unwrap_err()
        .contains(".incodex is a symlink"));
    assert!(publish(&user_root)
        .unwrap_err()
        .contains(".incodex is a symlink"));
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn ensure_current_repairs_an_insecure_user_root_mode() {
    let parent = scratch("user-root-mode");
    let user_root = parent.join(".incodex");
    fs::create_dir_all(&user_root).unwrap();
    fs::set_permissions(&user_root, fs::Permissions::from_mode(0o777)).unwrap();

    assert!(inspect_deployed(&user_root).is_err());
    ensure_current(&user_root).unwrap();

    assert_eq!(fs::metadata(&user_root).unwrap().mode() & 0o777, DIR_MODE);
    assert!(deployed_current_matches_embedded(&user_root).unwrap());
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn no_replace_rename_preserves_an_existing_destination() {
    let root = scratch("rename-exclusive");
    let source = root.join("source");
    let destination = root.join("destination");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::write(source.join("source-marker"), b"source").unwrap();
    fs::write(destination.join("destination-marker"), b"destination").unwrap();

    assert!(rename_noreplace(&source, &destination).is_err());
    assert!(source.join("source-marker").is_file());
    assert_eq!(
        fs::read(destination.join("destination-marker")).unwrap(),
        b"destination"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_null_manifest_provenance_is_invalid_and_republished() {
    let root = scratch("null-provenance");
    publish(&root).unwrap();
    let mut current = read_current(&root);
    current["manifestSha256"] = serde_json::Value::Null;
    current["sourceCommit"] = serde_json::Value::Null;
    write_json(&runtime_root(&root).join("current.json"), &current);

    assert_eq!(
        inspect_deployed(&root).unwrap_err(),
        "runtime manifest pointer fields must be paired non-null strings"
    );
    ensure_current(&root).unwrap();

    let repaired = verify_current_complete(&root);
    assert!(repaired["manifestSha256"].is_string());
    assert!(repaired["sourceCommit"].is_string());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_pointer_with_matching_embedded_files_is_current_without_manifest_provenance() {
    let root = scratch("legacy-current");
    write_legacy_embedded_release(&root, &runtime_version(), &runtime_version());
    let current_before = fs::read(runtime_root(&root).join("current.json")).unwrap();

    assert!(deployed_current_matches_embedded(&root).unwrap());
    let ensured = ensure_current(&root).unwrap();

    assert_eq!(ensured.version, runtime_version());
    assert_eq!(ensured.release, format!("releases/{}", runtime_version()));
    assert_eq!(
        fs::read(runtime_root(&root).join("current.json")).unwrap(),
        current_before,
        "matching legacy content must not be rewritten just because its pointer lacks a manifest hash"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_pointer_with_old_version_is_stale_even_when_files_match() {
    let root = scratch("legacy-version-stale");
    write_legacy_embedded_release(&root, "0.3.1", "0.3.1");

    assert!(!deployed_current_matches_embedded(&root).unwrap());
    ensure_current(&root).unwrap();

    let current = verify_current_complete(&root);
    assert_eq!(current["version"], runtime_version());
    assert_eq!(current["manifestSha256"], manifest_hash());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn ensure_current_publishes_when_a_legacy_pointer_has_stale_content() {
    let root = scratch("legacy-stale");
    write_old_release(&root, "0.3.1", b"old runtime");

    assert!(!deployed_current_matches_embedded(&root).unwrap());
    ensure_current(&root).unwrap();

    let current = verify_current_complete(&root);
    assert_eq!(
        current["release"],
        format!("releases/{}", expected_new_release())
    );
    assert_eq!(current["manifestSha256"], manifest_hash());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn ensure_current_does_not_trust_a_same_version_pointer_with_different_content() {
    let root = scratch("same-version-stale");
    write_old_release(&root, &runtime_version(), b"old runtime");

    assert!(!deployed_current_matches_embedded(&root).unwrap());
    ensure_current(&root).unwrap();

    let current = verify_current_complete(&root);
    assert_eq!(
        current["release"],
        format!("releases/{}", expected_new_release())
    );
    fs::remove_dir_all(root).unwrap();
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
        manifest_source().as_bytes()
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
        format!(
            "{}-{}",
            runtime_version(),
            sha256_hex(manifest_bytes.as_bytes())
        )
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
