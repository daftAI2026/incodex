//! Embed and publish Electron runtime files from committed `dist/`.

use std::fs;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use sha2::{Digest, Sha256};

const DIR_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;
// The pointer remains schema 1; manifest provenance is an optional extension.
const CURRENT_SCHEMA: u64 = 1;
const MANIFEST_NAME: &str = "runtime-manifest.json";
static PUBLISH_LOCK: Mutex<()> = Mutex::new(());

const LOADER: &str = include_str!("../../../dist/incodex-loader.cjs");
const INJECT: &str = include_str!("../../../dist/incodex-inject.js");
const MAIN: &str = include_str!("../../../dist/incodex-main.cjs");
const PRELOAD: &str = include_str!("../../../dist/incodex-preload.cjs");
const SAFE_HOME: &str = include_str!("../../../dist/incodex-safe-home.cjs");
const IPC_GUARD: &str = include_str!("../../../dist/incodex-ipc-guard.cjs");
const OWNER_CORE: &str = include_str!("../../../dist/incodex-owner-core.cjs");
const OWNER_RECOVERY: &str = include_str!("../../../dist/incodex-owner-recovery.cjs");
const INSTANCE: &str = include_str!("../../../dist/incodex-instance.cjs");
const RUNTIME_LOAD: &str = include_str!("../../../dist/incodex-runtime-load.cjs");
const WINDOW_KIND: &str = include_str!("../../../dist/incodex-window-kind.cjs");
const MANIFEST: &str = include_str!("../../../dist/runtime-manifest.json");

const EXTERNAL_FILES: &[(&str, &str)] = &[
    // Keep this list identical to RUNTIME_FILES in src/runtime/incodex-loader.cts.
    ("incodex-main.cjs", MAIN),
    ("incodex-preload.cjs", PRELOAD),
    ("incodex-inject.js", INJECT),
    ("incodex-safe-home.cjs", SAFE_HOME),
    ("incodex-ipc-guard.cjs", IPC_GUARD),
    ("incodex-owner-core.cjs", OWNER_CORE),
    ("incodex-owner-recovery.cjs", OWNER_RECOVERY),
    ("incodex-instance.cjs", INSTANCE),
    ("incodex-window-kind.cjs", WINDOW_KIND),
    ("incodex-runtime-load.cjs", RUNTIME_LOAD),
];

#[derive(Debug, Clone)]
pub struct PublishedRuntime {
    pub version: String,
    pub release: String,
}

#[derive(Debug, Clone)]
struct EmbeddedManifest {
    version: String,
    source_commit: String,
    files: serde_json::Map<String, serde_json::Value>,
}

pub fn loader_source() -> &'static str {
    LOADER
}

/// Return the fixed set of artifacts that the external loader requires.
pub fn required_runtime_files() -> impl Iterator<Item = &'static str> {
    EXTERNAL_FILES.iter().map(|(name, _)| *name)
}

pub fn runtime_version() -> String {
    embedded_manifest()
        .map(|manifest| manifest.version)
        .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string())
}

pub fn publish(user_root: &Path) -> Result<PublishedRuntime, String> {
    publish_inner(user_root, |_| {})
}

fn publish_inner<F>(user_root: &Path, mut hook: F) -> Result<PublishedRuntime, String>
where
    F: FnMut(&str),
{
    let manifest = embedded_manifest()?;
    let version = manifest.version.clone();
    validate_path_component(&version, "runtime version")?;
    let manifest_hash = sha256_hex(MANIFEST.as_bytes());
    let release_name = format!("{version}-{manifest_hash}");

    let root = user_root.join("runtime");
    mkdir_mode(&root)?;
    let _lock = RuntimePublishLock::acquire(&root)?;
    let releases = root.join("releases");
    mkdir_mode(&releases)?;
    let final_dir = releases.join(&release_name);
    let files = runtime_file_hashes(&manifest)?;

    match fs::symlink_metadata(&final_dir) {
        Ok(_) => verify_release(&final_dir, &version, &manifest, &files, &manifest_hash)
            .map_err(|error| format!("runtime release {release_name} is not reusable: {error}"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let staging = releases.join(format!(
                ".staging-{version}-{}-{}",
                std::process::id(),
                unique_suffix()
            ));
            mkdir_mode(&staging)?;
            for (name, body) in EXTERNAL_FILES {
                write_durable(&staging.join(name), body.as_bytes())?;
            }
            write_durable(&staging.join(MANIFEST_NAME), MANIFEST.as_bytes())?;
            hook("staging-write");
            sync_dir(&staging)?;
            hook("staging-dir-sync");

            // The destination is content-addressed. Never replace a path that
            // appeared after staging began; a damaged address is evidence.
            match fs::symlink_metadata(&final_dir) {
                Ok(_) => {
                    return Err(format!(
                        "runtime release {release_name} appeared during publish"
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
            fs::rename(&staging, &final_dir).map_err(|error| error.to_string())?;
            hook("final-rename");
            sync_dir(&releases)?;
        }
        Err(error) => return Err(error.to_string()),
    }

    let current = serde_json::json!({
        "schemaVersion": CURRENT_SCHEMA,
        "version": version,
        "release": format!("releases/{release_name}"),
        "manifestSha256": manifest_hash,
        "sourceCommit": manifest.source_commit,
        "files": files,
    });
    let current_body = format!(
        "{}\n",
        serde_json::to_string_pretty(&current).map_err(|error| error.to_string())?
    );
    write_current(&root, current_body.as_bytes(), &mut hook)?;
    Ok(PublishedRuntime {
        version,
        release: format!("releases/{release_name}"),
    })
}

fn embedded_manifest() -> Result<EmbeddedManifest, String> {
    let raw: serde_json::Value = serde_json::from_str(MANIFEST)
        .map_err(|error| format!("invalid embedded runtime manifest: {error}"))?;
    let version = raw
        .get("runtimeVersion")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("embedded runtime manifest has no runtimeVersion")?
        .to_string();
    let source_commit = raw
        .get("sourceCommit")
        .and_then(serde_json::Value::as_str)
        .ok_or("embedded runtime manifest has no sourceCommit")?
        .to_string();
    validate_source_commit(&source_commit)?;
    let files = raw
        .get("files")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .ok_or("embedded runtime manifest has no files")?;
    Ok(EmbeddedManifest {
        version,
        source_commit,
        files,
    })
}

fn runtime_file_hashes(
    manifest: &EmbeddedManifest,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let mut files = serde_json::Map::new();
    for (name, body) in EXTERNAL_FILES {
        let actual = sha256_hex(body.as_bytes());
        let declared = manifest
            .files
            .get(*name)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("embedded runtime manifest is missing {name}"))?;
        if declared != actual {
            return Err(format!("embedded runtime manifest hash mismatch: {name}"));
        }
        files.insert((*name).to_string(), serde_json::Value::String(actual));
    }
    Ok(files)
}

fn verify_release(
    release: &Path,
    version: &str,
    manifest: &EmbeddedManifest,
    files: &serde_json::Map<String, serde_json::Value>,
    manifest_hash: &str,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(release).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_dir() {
        return Err("release is not a directory".into());
    }
    if metadata.permissions().mode() & 0o777 != DIR_MODE {
        return Err("release directory mode is not 0700".into());
    }
    let expected_name = format!("{version}-{manifest_hash}");
    if release.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str()) {
        return Err("release directory is not named by version and manifest hash".into());
    }
    let manifest_path = release.join(MANIFEST_NAME);
    verify_file(&manifest_path, MANIFEST.as_bytes(), manifest_hash)?;
    for (name, body) in EXTERNAL_FILES {
        let expected = files
            .get(*name)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("release pointer is missing {name}"))?;
        verify_file(&release.join(name), body.as_bytes(), expected)?;
    }
    if manifest.version != version {
        return Err("manifest version does not match release".into());
    }
    Ok(())
}

fn verify_file(path: &Path, expected_body: &[u8], expected_hash: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    if metadata.permissions().mode() & 0o777 != FILE_MODE {
        return Err(format!("{} mode is not 0600", path.display()));
    }
    let body = fs::read(path).map_err(|error| error.to_string())?;
    if body != expected_body {
        return Err(format!(
            "{} contents are not the embedded release",
            path.display()
        ));
    }
    if sha256_hex(&body) != expected_hash {
        return Err(format!("{} hash mismatch", path.display()));
    }
    Ok(())
}

struct RuntimePublishLock {
    file: File,
    _thread: MutexGuard<'static, ()>,
}

impl RuntimePublishLock {
    fn acquire(root: &Path) -> Result<Self, String> {
        let thread = PUBLISH_LOCK
            .lock()
            .map_err(|_| "runtime publish lock is poisoned".to_string())?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create(true)
            .mode(FILE_MODE)
            .custom_flags(libc::O_NOFOLLOW);
        let file = options
            .open(root.join(".publish.lock"))
            .map_err(|error| error.to_string())?;
        let status = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if status != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(Self {
            file,
            _thread: thread,
        })
    }
}

impl Drop for RuntimePublishLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn mkdir_mode(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| error.to_string())?;
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }
    let mut perms = metadata.permissions();
    perms.set_mode(DIR_MODE);
    fs::set_permissions(path, perms).map_err(|error| error.to_string())
}

fn write_durable(path: &Path, body: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("runtime file needs a parent directory")?;
    mkdir_mode(parent)?;
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .mode(FILE_MODE)
        .custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(path).map_err(|error| error.to_string())?;
    file.write_all(body).map_err(|error| error.to_string())?;
    let mut perms = file
        .metadata()
        .map_err(|error| error.to_string())?
        .permissions();
    perms.set_mode(FILE_MODE);
    file.set_permissions(perms)
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

fn write_current<F>(root: &Path, body: &[u8], hook: &mut F) -> Result<(), String>
where
    F: FnMut(&str),
{
    let temporary = root.join(format!(
        ".current.json.tmp-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    write_durable(&temporary, body)?;
    if let Ok(metadata) = fs::symlink_metadata(root.join("current.json")) {
        if metadata.file_type().is_symlink() {
            return Err("refusing symlink current.json".into());
        }
    }
    hook("current-rename-before");
    fs::rename(&temporary, root.join("current.json")).map_err(|error| error.to_string())?;
    hook("current-rename-after");
    sync_dir(root)
}

fn sync_dir(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| error.to_string())
}

fn validate_path_component(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
    {
        return Err(format!("invalid {label}"));
    }
    Ok(())
}

fn validate_source_commit(value: &str) -> Result<(), String> {
    if value.is_empty() || (value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        Ok(())
    } else {
        Err("runtime sourceCommit must be empty or a 40-character hex SHA".into())
    }
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
fn publish_with_test_hook<F>(user_root: &Path, hook: F) -> Result<PublishedRuntime, String>
where
    F: FnMut(&str),
{
    publish_inner(user_root, hook)
}

#[cfg(test)]
mod tests;
