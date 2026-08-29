//! Embed and publish Electron runtime files from committed `dist/`.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use incodex_runtime_assets::external_files;
use incodex_runtime_assets::{loader_source as embedded_loader, manifest_source, LOADER_NAME};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const DIR_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;
// The pointer remains schema 1; manifest provenance is an optional extension.
const CURRENT_SCHEMA: u64 = 1;
const MANIFEST_NAME: &str = "runtime-manifest.json";
const MAX_METADATA_BYTES: u64 = 1024 * 1024;
static PUBLISH_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone)]
pub struct PublishedRuntime {
    pub version: String,
    pub release: String,
}

/// The identity of the Runtime embedded in this CLI.
///
/// `manifest_sha256` is the release address. `files` is kept public because
/// schema-1 pointers may omit manifest provenance and can then only be
/// compared by their complete required-artifact hashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeIdentity {
    pub version: String,
    pub manifest_sha256: String,
    pub files: BTreeMap<String, String>,
}

/// A deployed Runtime pointer after its release and required files have been
/// verified against the pointer's own hashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployedRuntime {
    pub version: String,
    pub release: String,
    pub manifest_sha256: Option<String>,
    pub source_commit: Option<String>,
    pub files: BTreeMap<String, String>,
}

impl RuntimeIdentity {
    pub fn matches(&self, deployed: &DeployedRuntime) -> bool {
        deployed.version == self.version
            && deployed.files == self.files
            && deployed
                .manifest_sha256
                .as_deref()
                .is_none_or(|hash| hash == self.manifest_sha256)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmbeddedManifest {
    runtime_version: String,
    source_commit: String,
    files: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurrentPointer {
    schema_version: Option<u64>,
    version: Option<String>,
    release: Option<String>,
    manifest_sha256: Option<String>,
    source_commit: Option<String>,
    files: Option<BTreeMap<String, String>>,
    #[serde(skip)]
    manifest_sha256_present: bool,
    #[serde(skip)]
    source_commit_present: bool,
}

pub fn loader_source() -> &'static str {
    embedded_loader()
}

/// Return the fixed set of artifacts that the external loader requires.
pub fn required_runtime_files() -> impl Iterator<Item = &'static str> {
    external_files().iter().map(|(name, _)| *name)
}

pub fn runtime_version() -> String {
    embedded_manifest().map_or_else(
        |_| env!("CARGO_PKG_VERSION").to_string(),
        |manifest| manifest.runtime_version,
    )
}

/// Return the validated identity of the Runtime bundled into this binary.
pub fn runtime_identity() -> Result<RuntimeIdentity, String> {
    let (manifest, files, manifest_sha256) = embedded_snapshot()?;
    Ok(RuntimeIdentity {
        version: manifest.runtime_version,
        manifest_sha256,
        files,
    })
}

/// Check the deployed pointer and release against the embedded canonical
/// manifest without writing anything.
///
/// A schema-1 pointer without manifest provenance is accepted only when every
/// required external artifact has the embedded content hash. Version alone is
/// never sufficient.
pub fn deployed_current_matches_embedded(user_root: &Path) -> Result<bool, String> {
    let identity = runtime_identity()?;
    Ok(inspect_deployed(user_root)?
        .as_ref()
        .is_some_and(|deployed| identity.matches(deployed)))
}

/// Publish the embedded Runtime only when the deployed content is stale or
/// absent. The actual write path remains [`publish`], so synchronization keeps
/// the existing content-addressed, lock-protected, atomic publication rules.
pub fn ensure_current(user_root: &Path) -> Result<PublishedRuntime, String> {
    let identity = runtime_identity()?;
    let deployed = inspect_deployed(user_root).ok().flatten();
    if let Some(deployed) = deployed.filter(|deployed| identity.matches(deployed)) {
        return Ok(PublishedRuntime {
            version: identity.version,
            release: deployed.release,
        });
    }
    publish(user_root)
}

/// Read and verify the deployed Runtime without comparing it to this binary.
/// None means the runtime pointer has not been published yet.
pub fn inspect_deployed(user_root: &Path) -> Result<Option<DeployedRuntime>, String> {
    let Some(_user_root_guard) = inspect_user_root(user_root)? else {
        return Ok(None);
    };
    let root = user_root.join("runtime");
    let Some(pointer) = read_current_pointer(&root)? else {
        return Ok(None);
    };
    if pointer.schema_version != Some(CURRENT_SCHEMA) {
        return Err("invalid current.json schema".into());
    }
    let version = pointer
        .version
        .filter(|value| !value.is_empty())
        .ok_or("runtime version is missing")?;
    let release = pointer
        .release
        .filter(|value| !value.is_empty())
        .ok_or("runtime release is missing")?;
    if !safe_release(&release) {
        return Err("runtime release is not a safe relative path".into());
    }
    let Some(release_dir) = existing_release(&root, &release)? else {
        return Err("runtime release is missing".into());
    };
    let pointer_files = pointer.files.ok_or("runtime files are missing")?;
    if pointer_files.is_empty() {
        return Err("runtime files are empty".into());
    }
    for name in required_runtime_files() {
        if !pointer_files.contains_key(name) {
            return Err(format!("runtime file is missing: {name}"));
        }
    }
    let required_count = required_runtime_files().count();
    if pointer_files.len() != required_count {
        return Err("runtime files do not match required artifacts".into());
    }
    verify_release_files(&release_dir, &pointer_files)?;

    let (manifest_sha256, source_commit) = match (
        pointer.manifest_sha256_present,
        pointer.source_commit_present,
        pointer.manifest_sha256,
        pointer.source_commit,
    ) {
        (false, false, None, None) => (None, None),
        (true, true, Some(manifest_hash), Some(source_commit)) => {
            if !is_sha256(&manifest_hash) || !is_source_commit(&source_commit) {
                return Err("runtime manifest provenance is invalid".into());
            }
            let expected_release = format!("{version}-{manifest_hash}");
            if release != format!("releases/{expected_release}") {
                return Err("runtime release name does not match manifest hash".into());
            }
            let manifest_path = release_dir.join(MANIFEST_NAME);
            let manifest_body = read_hashed_metadata_file(&manifest_path, &manifest_hash)?;
            let manifest: EmbeddedManifest = serde_json::from_slice(&manifest_body)
                .map_err(|error| format!("invalid runtime manifest: {error}"))?;
            if manifest.runtime_version != version || manifest.source_commit != source_commit {
                return Err("runtime manifest provenance mismatch".into());
            }
            let manifest_files = declared_runtime_file_hashes(&manifest)?;
            if manifest_files != pointer_files {
                return Err("runtime manifest files do not match current.json".into());
            }
            (Some(manifest_hash), Some(source_commit))
        }
        _ => return Err("runtime manifest pointer fields must be paired non-null strings".into()),
    };

    Ok(Some(DeployedRuntime {
        version,
        release,
        manifest_sha256,
        source_commit,
        files: pointer_files,
    }))
}

pub fn publish(user_root: &Path) -> Result<PublishedRuntime, String> {
    publish_inner(user_root, |_| {})
}

fn publish_inner<F>(user_root: &Path, mut hook: F) -> Result<PublishedRuntime, String>
where
    F: FnMut(&str),
{
    let (manifest, files, manifest_hash) = embedded_snapshot()?;
    let version = manifest.runtime_version.clone();
    validate_path_component(&version, "runtime version")?;
    let release_name = format!("{version}-{manifest_hash}");
    let release = format!("releases/{release_name}");

    let _user_root_guard = ensure_user_root(user_root)?;
    let root = user_root.join("runtime");
    mkdir_mode(&root)?;
    let _lock = RuntimePublishLock::acquire(&root)?;
    let releases = root.join("releases");
    mkdir_mode(&releases)?;
    let final_dir = releases.join(&release_name);

    match fs::symlink_metadata(&final_dir) {
        Ok(_) => verify_release(&final_dir, &version, &manifest_hash, &files)
            .map_err(|error| format!("runtime release {release_name} is not reusable: {error}"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let staging = releases.join(format!(
                ".staging-{version}-{}-{}",
                std::process::id(),
                unique_suffix()
            ));
            mkdir_mode(&staging)?;
            for (name, body) in external_files() {
                write_durable(&staging.join(name), body.as_bytes())?;
            }
            write_durable(&staging.join(MANIFEST_NAME), manifest_source().as_bytes())?;
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
            rename_noreplace(&staging, &final_dir)?;
            hook("final-rename");
            sync_dir(&releases)?;
        }
        Err(error) => return Err(error.to_string()),
    }

    let current = serde_json::json!({
        "schemaVersion": CURRENT_SCHEMA,
        "version": version,
        "release": release,
        "manifestSha256": manifest_hash,
        "sourceCommit": manifest.source_commit,
        "files": files,
    });
    let current_body = format!(
        "{}\n",
        serde_json::to_string_pretty(&current).map_err(|error| error.to_string())?
    );
    write_current(&root, current_body.as_bytes(), &mut hook)?;
    Ok(PublishedRuntime { version, release })
}

fn embedded_manifest() -> Result<EmbeddedManifest, String> {
    let manifest: EmbeddedManifest = serde_json::from_str(manifest_source())
        .map_err(|error| format!("invalid embedded runtime manifest: {error}"))?;
    if manifest.runtime_version.is_empty() {
        return Err("embedded runtime manifest has no runtimeVersion".into());
    }
    validate_source_commit(&manifest.source_commit)?;
    Ok(manifest)
}

fn embedded_snapshot() -> Result<(EmbeddedManifest, BTreeMap<String, String>, String), String> {
    let manifest = embedded_manifest()?;
    let files = runtime_file_hashes(&manifest)?;
    let manifest_hash = sha256_hex(manifest_source().as_bytes());
    Ok((manifest, files, manifest_hash))
}

fn runtime_file_hashes(manifest: &EmbeddedManifest) -> Result<BTreeMap<String, String>, String> {
    let declared = declared_runtime_file_hashes(manifest)?;
    let loader_hash = manifest
        .files
        .get(LOADER_NAME)
        .ok_or("runtime manifest is missing incodex-loader.cjs")?;
    if loader_hash != &sha256_hex(embedded_loader().as_bytes()) {
        return Err("embedded runtime manifest hash mismatch: incodex-loader.cjs".into());
    }
    let mut files = BTreeMap::new();
    for (name, body) in external_files() {
        let actual = sha256_hex(body.as_bytes());
        if declared.get(*name) != Some(&actual) {
            return Err(format!("embedded runtime manifest hash mismatch: {name}"));
        }
        files.insert((*name).to_string(), actual);
    }
    Ok(files)
}

fn declared_runtime_file_hashes(
    manifest: &EmbeddedManifest,
) -> Result<BTreeMap<String, String>, String> {
    if manifest.files.len() != external_files().len() + 1
        || !manifest
            .files
            .get(LOADER_NAME)
            .is_some_and(|hash| is_sha256(hash))
    {
        return Err("runtime manifest files do not match required artifacts".into());
    }
    external_files()
        .iter()
        .map(|(name, _)| {
            let hash = manifest
                .files
                .get(*name)
                .filter(|hash| is_sha256(hash))
                .ok_or_else(|| {
                    format!("runtime manifest is missing or has an invalid {name} hash")
                })?;
            Ok(((*name).to_string(), hash.clone()))
        })
        .collect()
}

fn verify_release(
    release: &Path,
    version: &str,
    manifest_hash: &str,
    files: &BTreeMap<String, String>,
) -> Result<(), String> {
    let expected_name = format!("{version}-{manifest_hash}");
    if release.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str()) {
        return Err("release directory is not named by version and manifest hash".into());
    }
    let manifest_path = release.join(MANIFEST_NAME);
    verify_file(&manifest_path, manifest_source().as_bytes())?;
    verify_release_files(release, files)?;
    Ok(())
}

fn verify_release_files(release: &Path, files: &BTreeMap<String, String>) -> Result<(), String> {
    let metadata = fs::symlink_metadata(release).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{} is a symlink", release.display()));
    }
    if !metadata.file_type().is_dir() {
        return Err("release is not a directory".into());
    }
    let mut paths = Vec::with_capacity(files.len());
    for name in files.keys() {
        let path = release.join(name);
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err(format!("{} is a symlink", path.display()));
        }
        if !metadata.file_type().is_file() {
            return Err(format!("{} is not a regular file", path.display()));
        }
        paths.push((name, path));
    }
    if metadata.permissions().mode() & 0o777 != DIR_MODE {
        return Err("release directory mode is not 0700".into());
    }
    for (name, path) in paths {
        let expected_hash = &files[name];
        if hash_private_file(&path)? != *expected_hash {
            return Err(format!("runtime hash mismatch: {name}"));
        }
    }
    Ok(())
}

fn read_current_pointer(root: &Path) -> Result<Option<CurrentPointer>, String> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!("{} is a symlink", root.display()))
        }
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(format!("{} is not a directory", root.display()))
        }
        Ok(metadata) if metadata.permissions().mode() & 0o777 != DIR_MODE => {
            return Err(format!("{} mode is not 0700", root.display()))
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    }
    let current = root.join("current.json");
    match fs::symlink_metadata(&current) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!("{} is a symlink", current.display()))
        }
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(format!("{} is not a regular file", current.display()))
        }
        Ok(metadata) if metadata.permissions().mode() & 0o777 != FILE_MODE => {
            return Err(format!("{} mode is not 0600", current.display()))
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    }
    let body = read_private_file(&current, MAX_METADATA_BYTES)?;
    let value: serde_json::Value =
        serde_json::from_slice(&body).map_err(|error| format!("invalid current.json: {error}"))?;
    let object = value
        .as_object()
        .ok_or("invalid current.json: expected an object")?;
    let manifest_sha256_present = object.contains_key("manifestSha256");
    let source_commit_present = object.contains_key("sourceCommit");
    let mut pointer: CurrentPointer =
        serde_json::from_value(value).map_err(|error| format!("invalid current.json: {error}"))?;
    pointer.manifest_sha256_present = manifest_sha256_present;
    pointer.source_commit_present = source_commit_present;
    Ok(Some(pointer))
}

fn safe_release(value: &str) -> bool {
    let Some(name) = value.strip_prefix("releases/") else {
        return false;
    };
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && name != "." && name != ".."
}

fn existing_release(root: &Path, release: &str) -> Result<Option<PathBuf>, String> {
    let releases = root.join("releases");
    match fs::symlink_metadata(&releases) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!("{} is a symlink", releases.display()))
        }
        Ok(metadata) if !metadata.file_type().is_dir() => return Ok(None),
        Ok(metadata) if metadata.permissions().mode() & 0o777 != DIR_MODE => {
            return Err(format!("{} mode is not 0700", releases.display()))
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    }
    let path = root.join(release);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("{} is a symlink", path.display()))
        }
        Ok(metadata) if !metadata.file_type().is_dir() => Ok(None),
        Ok(_) => Ok(Some(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn verify_file(path: &Path, expected_body: &[u8]) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{} is a symlink", path.display()));
    }
    if !metadata.file_type().is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    if metadata.permissions().mode() & 0o777 != FILE_MODE {
        return Err(format!("{} mode is not 0600", path.display()));
    }
    let body = read_private_file(path, expected_body.len() as u64 + 1)?;
    if body != expected_body {
        return Err(format!(
            "{} contents are not the embedded release",
            path.display()
        ));
    }
    Ok(())
}

fn read_hashed_metadata_file(path: &Path, expected_hash: &str) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{} is a symlink", path.display()));
    }
    if !metadata.file_type().is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    if metadata.permissions().mode() & 0o777 != FILE_MODE {
        return Err(format!("{} mode is not 0600", path.display()));
    }
    let body = read_private_file(path, MAX_METADATA_BYTES)?;
    if sha256_hex(&body) != expected_hash {
        Err(format!(
            "{} contents do not match the declared hash",
            path.display()
        ))
    } else {
        Ok(body)
    }
}

fn read_private_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>, String> {
    let mut options = OpenOptions::new();
    options.read(true).custom_flags(libc::O_NOFOLLOW);
    let file = options.open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    if metadata.permissions().mode() & 0o777 != FILE_MODE {
        return Err(format!("{} mode is not 0600", path.display()));
    }
    let mut body = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    if body.len() as u64 > max_bytes {
        return Err(format!("{} is too large", path.display()));
    }
    Ok(body)
}

fn hash_private_file(path: &Path) -> Result<String, String> {
    let mut options = OpenOptions::new();
    options.read(true).custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    if metadata.permissions().mode() & 0o777 != FILE_MODE {
        return Err(format!("{} mode is not 0600", path.display()));
    }
    let mut hash = Sha256::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hash.update(&chunk[..read]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn inspect_user_root(path: &Path) -> Result<Option<File>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!("{} is a symlink", path.display()))
        }
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(format!("{} is not a directory", path.display()))
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    }
    let directory = open_directory(path)?;
    let mode = directory
        .metadata()
        .map_err(|error| error.to_string())?
        .permissions()
        .mode()
        & 0o777;
    if mode != DIR_MODE {
        return Err(format!("{} mode is not 0700", path.display()));
    }
    Ok(Some(directory))
}

fn ensure_user_root(path: &Path) -> Result<File, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!("{} is a symlink", path.display()))
        }
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(format!("{} is not a directory", path.display()))
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            builder.mode(DIR_MODE);
            match builder.create(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        Err(error) => return Err(error.to_string()),
    }
    let directory = open_directory(path)?;
    let status = unsafe { libc::fchmod(directory.as_raw_fd(), DIR_MODE as libc::mode_t) };
    if status != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(directory)
}

fn open_directory(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW);
    options
        .open(path)
        .map_err(|error| format!("cannot open {} safely: {error}", path.display()))
}

fn c_path(path: &Path) -> Result<CString, String> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| format!("path contains NUL: {}", path.display()))
}

#[cfg(target_os = "macos")]
fn rename_noreplace(from: &Path, to: &Path) -> Result<(), String> {
    let from = c_path(from)?;
    let to = c_path(to)?;
    let status = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

#[cfg(target_os = "linux")]
fn rename_noreplace(from: &Path, to: &Path) -> Result<(), String> {
    let from = c_path(from)?;
    let to = c_path(to)?;
    let status = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn rename_noreplace(from: &Path, to: &Path) -> Result<(), String> {
    if fs::symlink_metadata(to).is_ok() {
        return Err(format!("{} already exists", to.display()));
    }
    fs::rename(from, to).map_err(|error| error.to_string())
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
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.to_string()),
    }
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
    let current = root.join("current.json");
    let temporary = root.join(format!(
        ".current.json.tmp-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    write_durable(&temporary, body)?;
    let result = (|| {
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            if metadata.file_type().is_symlink() {
                return Err(format!("{} is a symlink", current.display()));
            }
        }
        hook("current-rename-before");
        fs::rename(&temporary, current).map_err(|error| error.to_string())?;
        hook("current-rename-after");
        sync_dir(root)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
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

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_source_commit(value: &str) -> bool {
    value.is_empty() || (value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
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
