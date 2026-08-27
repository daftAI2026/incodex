use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_core::windows_session::{
    apply_private_windows_acl, ensure_private_windows_dir, verify_private_acl,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::{
    MoveFileExW, FILE_ATTRIBUTE_REPARSE_POINT, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};

pub const WINDOWS_RUNTIME_FILES: &[&str] = &[
    "incodex-windows-bootstrap.cjs",
    "incodex-windows-platform.cjs",
    "incodex-main.cjs",
    "incodex-preload.cjs",
    "incodex-inject.js",
    "incodex-safe-home.cjs",
    "incodex-ipc-guard.cjs",
    "incodex-owner-core.cjs",
    "incodex-owner-recovery.cjs",
    "incodex-instance.cjs",
    "incodex-window-kind.cjs",
    "incodex-runtime-load.cjs",
];

const WINDOWS_BOOTSTRAP_NAME: &str = "incodex-windows-bootstrap.cjs";
const WINDOWS_BOOTSTRAP: &str = include_str!("../assets/incodex-windows-bootstrap.cjs");
const WINDOWS_ASSETS: &[(&str, &str)] = &[
    (WINDOWS_BOOTSTRAP_NAME, WINDOWS_BOOTSTRAP),
    (
        "incodex-windows-platform.cjs",
        include_str!("../assets/incodex-windows-platform.cjs"),
    ),
];

const RUNTIME_FILES: &[(&str, &str)] = &[
    (
        "incodex-main.cjs",
        include_str!("../../../dist/incodex-main.cjs"),
    ),
    (
        "incodex-preload.cjs",
        include_str!("../../../dist/incodex-preload.cjs"),
    ),
    (
        "incodex-inject.js",
        include_str!("../../../dist/incodex-inject.js"),
    ),
    (
        "incodex-safe-home.cjs",
        include_str!("../../../dist/incodex-safe-home.cjs"),
    ),
    (
        "incodex-ipc-guard.cjs",
        include_str!("../../../dist/incodex-ipc-guard.cjs"),
    ),
    (
        "incodex-owner-core.cjs",
        include_str!("../../../dist/incodex-owner-core.cjs"),
    ),
    (
        "incodex-owner-recovery.cjs",
        include_str!("../../../dist/incodex-owner-recovery.cjs"),
    ),
    (
        "incodex-instance.cjs",
        include_str!("../../../dist/incodex-instance.cjs"),
    ),
    (
        "incodex-window-kind.cjs",
        include_str!("../../../dist/incodex-window-kind.cjs"),
    ),
    (
        "incodex-runtime-load.cjs",
        include_str!("../../../dist/incodex-runtime-load.cjs"),
    ),
];
const MANIFEST: &str = include_str!("../../../dist/runtime-manifest.json");
const MANIFEST_NAME: &str = "runtime-manifest.json";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedWindowsRuntime {
    pub release_dir: PathBuf,
    pub main: PathBuf,
    pub pointer: PathBuf,
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeManifest {
    runtime_version: String,
    source_commit: String,
    files: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimePointer<'a> {
    schema_version: u32,
    version: &'a str,
    release: String,
    manifest_sha256: String,
    source_commit: &'a str,
    files: BTreeMap<String, String>,
}

pub fn publish_windows_runtime(user_root: &Path) -> Result<PublishedWindowsRuntime, String> {
    let manifest: RuntimeManifest = serde_json::from_str(MANIFEST)
        .map_err(|error| format!("invalid Runtime manifest: {error}"))?;
    validate_manifest(&manifest)?;
    let manifest_hash = sha256_hex(MANIFEST.as_bytes());
    let release_hash = windows_release_hash();
    let release_name = format!("{}-{release_hash}", manifest.runtime_version);

    let user_root = ensure_private_windows_dir(user_root)?;
    let runtime_root = ensure_private_windows_dir(&user_root.join("runtime"))?;
    let releases = ensure_private_windows_dir(&runtime_root.join("releases"))?;
    let release_dir = releases.join(&release_name);
    publish_release(&releases, &release_dir, &manifest)?;

    let relative_release = format!("releases/{release_name}");
    let files = runtime_hashes(&manifest)?;
    let pointer = RuntimePointer {
        schema_version: 1,
        version: &manifest.runtime_version,
        release: relative_release,
        manifest_sha256: manifest_hash,
        source_commit: &manifest.source_commit,
        files,
    };
    let pointer_body = serde_json::to_vec_pretty(&pointer)
        .map_err(|error| format!("cannot serialize Runtime pointer: {error}"))?;
    let pointer_path = runtime_root.join("current.json");
    replace_private_file(&runtime_root, &pointer_path, &pointer_body)?;

    let files = WINDOWS_RUNTIME_FILES
        .iter()
        .map(|name| release_dir.join(name))
        .collect::<Vec<_>>();
    Ok(PublishedWindowsRuntime {
        main: release_dir.join("incodex-main.cjs"),
        release_dir,
        pointer: pointer_path,
        files,
    })
}

pub fn publish_windows_activation_bootstrap(user_root: &Path) -> Result<PathBuf, String> {
    let runtime = publish_windows_runtime(user_root)?;
    Ok(runtime.release_dir.join(WINDOWS_BOOTSTRAP_NAME))
}

fn publish_release(
    releases: &Path,
    release_dir: &Path,
    manifest: &RuntimeManifest,
) -> Result<(), String> {
    match fs::symlink_metadata(release_dir) {
        Ok(_) => return verify_release(release_dir, manifest),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect Runtime release: {error}")),
    }

    let staging = releases.join(format!(
        ".staging-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let staging = ensure_private_windows_dir(&staging)?;
    let result = (|| {
        for (name, body) in RUNTIME_FILES {
            write_private_file(&staging.join(name), body.as_bytes())?;
        }
        for (name, body) in WINDOWS_ASSETS {
            write_private_file(&staging.join(name), body.as_bytes())?;
        }
        write_private_file(&staging.join(MANIFEST_NAME), MANIFEST.as_bytes())?;
        verify_release(&staging, manifest)?;
        match fs::rename(&staging, release_dir) {
            Ok(()) => Ok(()),
            Err(_error) if release_dir.exists() => verify_release(release_dir, manifest),
            Err(error) => Err(format!("cannot commit Runtime release: {error}")),
        }
    })();
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn verify_release(path: &Path, manifest: &RuntimeManifest) -> Result<(), String> {
    ensure_regular_directory(path)?;
    verify_private_acl(path)?;
    for (name, body) in RUNTIME_FILES {
        let file = path.join(name);
        ensure_regular_file(&file)?;
        verify_private_acl(&file)?;
        let actual = fs::read(&file).map_err(|error| format!("cannot read {name}: {error}"))?;
        if actual != body.as_bytes() {
            return Err(format!(
                "Runtime artifact does not match embedded release: {name}"
            ));
        }
    }
    for (name, body) in WINDOWS_ASSETS {
        let asset = path.join(name);
        ensure_regular_file(&asset)?;
        verify_private_acl(&asset)?;
        let actual = fs::read(&asset)
            .map_err(|error| format!("cannot read Windows Runtime asset {name}: {error}"))?;
        if actual != body.as_bytes() {
            return Err(format!(
                "Windows Runtime asset does not match embedded release: {name}"
            ));
        }
    }
    let manifest_path = path.join(MANIFEST_NAME);
    ensure_regular_file(&manifest_path)?;
    verify_private_acl(&manifest_path)?;
    let actual = fs::read(&manifest_path)
        .map_err(|error| format!("cannot read Runtime manifest: {error}"))?;
    if actual != MANIFEST.as_bytes() {
        return Err("Runtime manifest does not match embedded release".to_string());
    }
    validate_manifest(manifest)
}

fn validate_manifest(manifest: &RuntimeManifest) -> Result<(), String> {
    if manifest.runtime_version.is_empty() {
        return Err("Runtime manifest has no version".to_string());
    }
    if !manifest.source_commit.is_empty()
        && (manifest.source_commit.len() != 40
            || !manifest
                .source_commit
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err("Runtime manifest has an invalid source commit".to_string());
    }
    let _ = runtime_hashes(manifest)?;
    Ok(())
}

fn runtime_hashes(manifest: &RuntimeManifest) -> Result<BTreeMap<String, String>, String> {
    let mut hashes = BTreeMap::new();
    for (name, body) in RUNTIME_FILES {
        let actual = sha256_hex(body.as_bytes());
        if manifest.files.get(*name) != Some(&actual) {
            return Err(format!("Runtime manifest hash mismatch: {name}"));
        }
        hashes.insert((*name).to_string(), actual);
    }
    Ok(hashes)
}

fn write_private_file(path: &Path, body: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    file.write_all(body)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    apply_private_windows_acl(path)?;
    verify_private_acl(path)
}

pub(crate) fn replace_private_file(
    parent: &Path,
    target: &Path,
    body: &[u8],
) -> Result<(), String> {
    let temporary = parent.join(format!(
        ".current.json.tmp-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    write_private_file(&temporary, body)?;
    let from = wide_nul(&temporary)?;
    let to = wide_nul(target)?;
    let status = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if status == 0 {
        let error = std::io::Error::last_os_error();
        let _ = fs::remove_file(&temporary);
        return Err(format!("cannot replace Runtime pointer: {error}"));
    }
    apply_private_windows_acl(target)?;
    verify_private_acl(target)
}

fn ensure_regular_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !metadata.is_dir() {
        return Err(format!(
            "Runtime path is not a regular directory: {}",
            path.display()
        ));
    }
    Ok(())
}

fn ensure_regular_file(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !metadata.is_file() {
        return Err(format!(
            "Runtime path is not a regular file: {}",
            path.display()
        ));
    }
    Ok(())
}

fn wide_nul(path: &Path) -> Result<Vec<u16>, String> {
    use std::os::windows::ffi::OsStrExt;

    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(format!("path contains NUL: {}", path.display()));
    }
    wide.push(0);
    Ok(wide)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn windows_release_hash() -> String {
    let mut hash = Sha256::new();
    hash.update(MANIFEST.as_bytes());
    hash.update([0]);
    for (name, body) in WINDOWS_ASSETS {
        hash.update(name.as_bytes());
        hash.update([0]);
        hash.update(body.as_bytes());
        hash.update([0]);
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
