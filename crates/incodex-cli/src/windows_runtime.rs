use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_core::windows_session::{
    apply_private_windows_acl, ensure_private_windows_dir, verify_private_acl,
};
use incodex_runtime_assets::external_files;
use incodex_runtime_assets::{external_artifact_names, manifest_source};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::{
    MoveFileExW, FILE_ATTRIBUTE_REPARSE_POINT, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};

const WINDOWS_BOOTSTRAP_NAME: &str = "incodex-windows-bootstrap.cjs";
const WINDOWS_MAIN_NAME: &str = "incodex-main.cjs";
const WINDOWS_BOOTSTRAP: &str = include_str!("../assets/incodex-windows-bootstrap.cjs");
const WINDOWS_ASSETS: &[(&str, &str)] = &[
    (WINDOWS_BOOTSTRAP_NAME, WINDOWS_BOOTSTRAP),
    (
        "incodex-windows-platform.cjs",
        include_str!("../assets/incodex-windows-platform.cjs"),
    ),
];

const LEGACY_RUNTIME_FILE_NAMES: &[&str] = &[
    WINDOWS_MAIN_NAME,
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
const LEGACY_WINDOWS_ASSET_NAMES: &[&str] =
    &[WINDOWS_BOOTSTRAP_NAME, "incodex-windows-platform.cjs"];
const MANIFEST_NAME: &str = "runtime-manifest.json";
const MANIFEST_LIMIT: u64 = 64 * 1024;
const RECORDED_MANIFEST_SCHEMA: u32 = 1;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn windows_runtime_files() -> impl Iterator<Item = &'static str> {
    WINDOWS_ASSETS
        .iter()
        .map(|(name, _)| *name)
        .chain(external_artifact_names().iter().copied())
}

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

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordedWindowsRuntimeManifest {
    schema_version: u32,
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

#[derive(Debug, Deserialize)]
struct ExistingRuntimePointer {
    version: String,
}

pub fn publish_windows_runtime(user_root: &Path) -> Result<PublishedWindowsRuntime, String> {
    let manifest: RuntimeManifest = serde_json::from_str(manifest_source())
        .map_err(|error| format!("invalid Runtime manifest: {error}"))?;
    validate_manifest(&manifest)?;
    let recorded_manifest = recorded_windows_manifest(&manifest)?;
    let recorded_manifest_body = serde_json::to_vec_pretty(&recorded_manifest)
        .map_err(|error| format!("cannot serialize Windows Runtime manifest: {error}"))?;
    let manifest_hash = sha256_hex(&recorded_manifest_body);
    let release_name = format!("{}-{manifest_hash}", manifest.runtime_version);

    let user_root = ensure_private_windows_dir(user_root)?;
    let runtime_root = ensure_private_windows_dir(&user_root.join("runtime"))?;
    reject_runtime_downgrade(&runtime_root, &manifest.runtime_version)?;
    let releases = ensure_private_windows_dir(&runtime_root.join("releases"))?;
    let release_dir = releases.join(&release_name);
    publish_release(&releases, &release_dir, &manifest, &recorded_manifest_body)?;

    let relative_release = format!("releases/{release_name}");
    let files = recorded_manifest.files;
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
    let stable_bootstrap = runtime_root.join(WINDOWS_BOOTSTRAP_NAME);
    replace_private_file(
        &runtime_root,
        &stable_bootstrap,
        WINDOWS_BOOTSTRAP.as_bytes(),
    )?;
    let pointer_path = runtime_root.join("current.json");
    replace_private_file(&runtime_root, &pointer_path, &pointer_body)?;

    let files = windows_runtime_files()
        .map(|name| release_dir.join(name))
        .collect::<Vec<_>>();
    Ok(PublishedWindowsRuntime {
        main: release_dir.join(WINDOWS_MAIN_NAME),
        release_dir,
        pointer: pointer_path,
        files,
    })
}

fn reject_runtime_downgrade(runtime_root: &Path, candidate: &str) -> Result<(), String> {
    let pointer = runtime_root.join("current.json");
    let metadata = match fs::symlink_metadata(&pointer) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("cannot inspect Runtime pointer: {error}")),
    };
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err("Windows Runtime pointer is not a regular file".to_string());
    }
    verify_private_acl(&pointer)?;
    if metadata.len() > MANIFEST_LIMIT {
        return Err("Windows Runtime pointer exceeds the size limit".to_string());
    }
    let body =
        fs::read(&pointer).map_err(|error| format!("cannot read Runtime pointer: {error}"))?;
    let Ok(existing) = serde_json::from_slice::<ExistingRuntimePointer>(&body) else {
        return Ok(());
    };
    let Some(existing) = crate::stable_release::parse_stable_version(&existing.version) else {
        return Ok(());
    };
    let Some(candidate) = crate::stable_release::parse_stable_version(candidate) else {
        return Err(format!("invalid embedded Runtime version: {candidate}"));
    };
    if existing > candidate {
        return Err("a newer Runtime generation is already published".to_string());
    }
    Ok(())
}

pub fn publish_windows_activation_bootstrap(user_root: &Path) -> Result<PathBuf, String> {
    let runtime = publish_windows_runtime(user_root)?;
    Ok(runtime.release_dir.join(WINDOWS_BOOTSTRAP_NAME))
}

pub(crate) fn verify_installed_windows_runtime(
    user_root: &Path,
    runtime_release: &str,
) -> Result<(), String> {
    if runtime_release.is_empty()
        || runtime_release == "."
        || runtime_release == ".."
        || runtime_release.contains(['/', '\\', '\0', '\r', '\n'])
    {
        return Err("Windows Runtime release name is invalid".to_string());
    }
    let runtime_root = user_root.join("runtime");
    let releases = runtime_root.join("releases");
    for directory in [user_root, runtime_root.as_path(), releases.as_path()] {
        ensure_regular_directory(directory)?;
        verify_private_acl(directory)?;
    }
    let stable_bootstrap = runtime_root.join(WINDOWS_BOOTSTRAP_NAME);
    ensure_regular_file(&stable_bootstrap)?;
    verify_private_acl(&stable_bootstrap)?;
    let bootstrap = fs::read(&stable_bootstrap)
        .map_err(|error| format!("cannot read stable Windows Runtime bootstrap: {error}"))?;
    if bootstrap != WINDOWS_BOOTSTRAP.as_bytes() {
        return Err("stable Windows Runtime bootstrap does not match the current CLI".to_string());
    }
    verify_recorded_release(&releases.join(runtime_release), runtime_release)
}

fn publish_release(
    releases: &Path,
    release_dir: &Path,
    manifest: &RuntimeManifest,
    recorded_manifest_body: &[u8],
) -> Result<(), String> {
    match fs::symlink_metadata(release_dir) {
        Ok(_) => return verify_release(release_dir, manifest, recorded_manifest_body),
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
        for (name, body) in external_files() {
            write_private_file(&staging.join(name), body.as_bytes())?;
        }
        for (name, body) in WINDOWS_ASSETS {
            write_private_file(&staging.join(name), body.as_bytes())?;
        }
        write_private_file(&staging.join(MANIFEST_NAME), recorded_manifest_body)?;
        verify_release(&staging, manifest, recorded_manifest_body)?;
        match fs::rename(&staging, release_dir) {
            Ok(()) => Ok(()),
            Err(_error) if release_dir.exists() => {
                verify_release(release_dir, manifest, recorded_manifest_body)
            }
            Err(error) => Err(format!("cannot commit Runtime release: {error}")),
        }
    })();
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn verify_release(
    path: &Path,
    manifest: &RuntimeManifest,
    recorded_manifest_body: &[u8],
) -> Result<(), String> {
    ensure_regular_directory(path)?;
    verify_private_acl(path)?;
    for (name, body) in external_files() {
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
    if actual != recorded_manifest_body {
        return Err("Runtime manifest does not match embedded release".to_string());
    }
    validate_manifest(manifest)
}

fn verify_recorded_release(path: &Path, runtime_release: &str) -> Result<(), String> {
    ensure_regular_directory(path)?;
    verify_private_acl(path)?;

    let manifest_path = path.join(MANIFEST_NAME);
    ensure_regular_file(&manifest_path)?;
    verify_private_acl(&manifest_path)?;
    let manifest_metadata = fs::metadata(&manifest_path)
        .map_err(|error| format!("cannot inspect Runtime manifest: {error}"))?;
    if manifest_metadata.len() > MANIFEST_LIMIT {
        return Err("Runtime manifest exceeds the size limit".to_string());
    }
    let manifest_body = fs::read(&manifest_path)
        .map_err(|error| format!("cannot read Runtime manifest: {error}"))?;
    let manifest_value: serde_json::Value = serde_json::from_slice(&manifest_body)
        .map_err(|error| format!("invalid recorded Runtime manifest: {error}"))?;
    if manifest_value.get("schemaVersion").is_some() {
        let manifest: RecordedWindowsRuntimeManifest = serde_json::from_value(manifest_value)
            .map_err(|error| format!("invalid recorded Windows Runtime manifest: {error}"))?;
        verify_self_describing_release(path, runtime_release, &manifest_body, &manifest)
    } else {
        verify_legacy_recorded_release(path, runtime_release, &manifest_body)
    }
}

fn verify_self_describing_release(
    path: &Path,
    runtime_release: &str,
    manifest_body: &[u8],
    manifest: &RecordedWindowsRuntimeManifest,
) -> Result<(), String> {
    if manifest.schema_version != RECORDED_MANIFEST_SCHEMA {
        return Err("recorded Windows Runtime manifest schema is unsupported".to_string());
    }
    validate_runtime_metadata(&manifest.runtime_version, &manifest.source_commit)?;
    if manifest.files.is_empty() || manifest.files.len() > 64 {
        return Err("recorded Windows Runtime manifest file count is invalid".to_string());
    }
    if !manifest.files.contains_key(WINDOWS_BOOTSTRAP_NAME) {
        return Err("recorded Windows Runtime manifest has no bootstrap".to_string());
    }
    if !manifest.files.contains_key(WINDOWS_MAIN_NAME) {
        return Err("recorded Windows Runtime manifest has no main entrypoint".to_string());
    }

    let mut casefolded_names = BTreeSet::new();
    for (name, expected) in &manifest.files {
        validate_recorded_file_name(name)?;
        if !casefolded_names.insert(name.to_ascii_lowercase()) {
            return Err("recorded Windows Runtime manifest has colliding file names".to_string());
        }
        let file = path.join(name);
        ensure_regular_file(&file)?;
        verify_private_acl(&file)?;
        validate_sha256(expected, &format!("recorded Runtime hash for {name}"))?;
        let actual = crate::windows_file::sha256_file(&file)?;
        if &actual != expected {
            return Err(format!("recorded Runtime artifact hash mismatch: {name}"));
        }
    }

    verify_recorded_directory_entries(path, manifest.files.keys())?;
    let release_hash = sha256_hex(manifest_body);
    let expected_release = format!("{}-{release_hash}", manifest.runtime_version);
    if runtime_release != expected_release {
        return Err(
            "Windows Runtime release identity does not match its recorded contents".to_string(),
        );
    }
    Ok(())
}

fn verify_legacy_recorded_release(
    path: &Path,
    runtime_release: &str,
    manifest_body: &[u8],
) -> Result<(), String> {
    let manifest: RuntimeManifest = serde_json::from_slice(manifest_body)
        .map_err(|error| format!("invalid recorded Runtime manifest: {error}"))?;
    validate_manifest_metadata(&manifest)?;

    for name in LEGACY_RUNTIME_FILE_NAMES {
        let file = path.join(name);
        ensure_regular_file(&file)?;
        verify_private_acl(&file)?;
        let expected = manifest
            .files
            .get(*name)
            .ok_or_else(|| format!("recorded Runtime manifest is missing {name}"))?;
        validate_sha256(expected, &format!("recorded Runtime hash for {name}"))?;
        let actual = crate::windows_file::sha256_file(&file)?;
        if &actual != expected {
            return Err(format!("recorded Runtime artifact hash mismatch: {name}"));
        }
    }

    let release_hash = recorded_windows_release_hash(path, manifest_body)?;
    let expected_release = format!("{}-{release_hash}", manifest.runtime_version);
    if runtime_release != expected_release {
        return Err(
            "Windows Runtime release identity does not match its recorded contents".to_string(),
        );
    }
    Ok(())
}

fn validate_manifest(manifest: &RuntimeManifest) -> Result<(), String> {
    validate_manifest_metadata(manifest)?;
    let _ = runtime_hashes(manifest)?;
    Ok(())
}

fn validate_manifest_metadata(manifest: &RuntimeManifest) -> Result<(), String> {
    validate_runtime_metadata(&manifest.runtime_version, &manifest.source_commit)
}

fn validate_runtime_metadata(runtime_version: &str, source_commit: &str) -> Result<(), String> {
    if runtime_version.is_empty() {
        return Err("Runtime manifest has no version".to_string());
    }
    if !source_commit.is_empty()
        && (source_commit.len() != 40
            || !source_commit.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err("Runtime manifest has an invalid source commit".to_string());
    }
    Ok(())
}

fn recorded_windows_manifest(
    manifest: &RuntimeManifest,
) -> Result<RecordedWindowsRuntimeManifest, String> {
    let mut files = runtime_hashes(manifest)?;
    for (name, body) in WINDOWS_ASSETS {
        files.insert((*name).to_string(), sha256_hex(body.as_bytes()));
    }
    Ok(RecordedWindowsRuntimeManifest {
        schema_version: RECORDED_MANIFEST_SCHEMA,
        runtime_version: manifest.runtime_version.clone(),
        source_commit: manifest.source_commit.clone(),
        files,
    })
}

fn validate_recorded_file_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name == MANIFEST_NAME
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(format!(
            "recorded Windows Runtime file name is invalid: {name}"
        ));
    }
    Ok(())
}

fn verify_recorded_directory_entries<'a>(
    path: &Path,
    recorded_names: impl Iterator<Item = &'a String>,
) -> Result<(), String> {
    let mut expected = recorded_names
        .map(|name| name.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    expected.insert(MANIFEST_NAME.to_string());
    let mut actual = BTreeSet::new();
    for entry in fs::read_dir(path)
        .map_err(|error| format!("cannot enumerate recorded Windows Runtime: {error}"))?
    {
        let entry =
            entry.map_err(|error| format!("cannot enumerate recorded Windows Runtime: {error}"))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "recorded Windows Runtime has a non-Unicode file name".to_string())?;
        actual.insert(name.to_ascii_lowercase());
    }
    if actual != expected {
        return Err("recorded Windows Runtime directory does not match its manifest".to_string());
    }
    Ok(())
}

fn validate_sha256(value: &str, label: &str) -> Result<(), String> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(format!("{label} is invalid"))
    }
}

fn runtime_hashes(manifest: &RuntimeManifest) -> Result<BTreeMap<String, String>, String> {
    let mut hashes = BTreeMap::new();
    for (name, body) in external_files() {
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

fn recorded_windows_release_hash(path: &Path, manifest_body: &[u8]) -> Result<String, String> {
    let mut hash = Sha256::new();
    hash.update(manifest_body);
    hash.update([0]);
    for name in LEGACY_WINDOWS_ASSET_NAMES {
        let asset = path.join(name);
        ensure_regular_file(&asset)?;
        verify_private_acl(&asset)?;
        hash.update(name.as_bytes());
        hash.update([0]);
        let mut file = fs::File::open(&asset)
            .map_err(|error| format!("cannot read Windows Runtime asset {name}: {error}"))?;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|error| format!("cannot read Windows Runtime asset {name}: {error}"))?;
            if read == 0 {
                break;
            }
            hash.update(&buffer[..read]);
        }
        hash.update([0]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_runtime_verifies_its_recorded_generation() {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let user_root = std::env::temp_dir().join(format!(
            "incodex-windows-runtime-generation-{}-{sequence}",
            std::process::id()
        ));
        let published = publish_windows_runtime(&user_root).expect("publish current Runtime");

        let previous_main = b"previous Runtime generation";
        let previous_inject = b"window.__publishedRuntimeGeneration = true;";
        fs::write(
            published.release_dir.join("incodex-main.cjs"),
            previous_main,
        )
        .expect("replace previous generation main");
        fs::write(
            published.release_dir.join("incodex-inject.js"),
            previous_inject,
        )
        .expect("replace previous generation injector");
        let manifest_path = published.release_dir.join(MANIFEST_NAME);
        let mut manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(&manifest_path).expect("read current Runtime manifest"),
        )
        .expect("parse current Runtime manifest");
        manifest
            .as_object_mut()
            .expect("Runtime manifest object")
            .remove("schemaVersion");
        let files = manifest["files"]
            .as_object_mut()
            .expect("Runtime manifest files");
        for name in LEGACY_WINDOWS_ASSET_NAMES {
            files.remove(*name);
        }
        manifest["files"]["incodex-main.cjs"] =
            serde_json::Value::String(sha256_hex(previous_main));
        manifest["files"]["incodex-inject.js"] =
            serde_json::Value::String(sha256_hex(previous_inject));
        let manifest_body = serde_json::to_vec_pretty(&manifest).expect("write previous manifest");
        fs::write(&manifest_path, &manifest_body).expect("replace previous Runtime manifest");

        let version = manifest["runtimeVersion"]
            .as_str()
            .expect("manifest Runtime version");
        let mut release_hash = Sha256::new();
        release_hash.update(&manifest_body);
        release_hash.update([0]);
        for name in LEGACY_WINDOWS_ASSET_NAMES {
            release_hash.update(name.as_bytes());
            release_hash.update([0]);
            release_hash.update(
                fs::read(published.release_dir.join(name)).expect("read Windows Runtime asset"),
            );
            release_hash.update([0]);
        }
        let release_hash = release_hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let previous_release = format!("{version}-{release_hash}");
        let previous_dir = published
            .release_dir
            .parent()
            .expect("Runtime releases parent")
            .join(&previous_release);
        fs::rename(&published.release_dir, &previous_dir).expect("record previous generation");

        let verification = verify_installed_windows_runtime(&user_root, &previous_release);
        let selected_inject =
            read_verified_windows_runtime_artifact(&user_root, &previous_release, "incodex-inject.js");
        fs::remove_dir_all(&user_root).expect("remove previous Runtime fixture");

        verification.expect("recorded Runtime generation must remain verifiable");
        assert_eq!(
            selected_inject.expect("read selected Runtime injector"),
            previous_inject
        );
    }

    #[test]
    fn installed_runtime_uses_the_recorded_generation_file_set() {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let user_root = std::env::temp_dir().join(format!(
            "incodex-windows-runtime-layout-{}-{sequence}",
            std::process::id()
        ));
        let published = publish_windows_runtime(&user_root).expect("publish current Runtime");

        let retired_name = "incodex-owner-recovery.cjs";
        fs::remove_file(published.release_dir.join(retired_name))
            .expect("remove retired Runtime artifact");
        let future_name = "incodex-future-owner.cjs";
        write_private_file(
            &published.release_dir.join(future_name),
            b"future Runtime generation",
        )
        .expect("write future Runtime artifact");

        let mut files = BTreeMap::new();
        for name in windows_runtime_files()
            .filter(|name| *name != retired_name)
            .chain([future_name])
        {
            files.insert(
                name.to_string(),
                crate::windows_file::sha256_file(&published.release_dir.join(name))
                    .expect("hash recorded Runtime artifact"),
            );
        }
        let manifest = serde_json::json!({
            "schemaVersion": 1,
            "runtimeVersion": "0.5.0",
            "sourceCommit": "",
            "files": files,
        });
        let manifest_body = serde_json::to_vec_pretty(&manifest).expect("write recorded manifest");
        fs::write(published.release_dir.join(MANIFEST_NAME), &manifest_body)
            .expect("replace recorded Runtime manifest");
        let future_release = format!("0.5.0-{}", sha256_hex(&manifest_body));
        let future_dir = published
            .release_dir
            .parent()
            .expect("Runtime releases parent")
            .join(&future_release);
        fs::rename(&published.release_dir, &future_dir).expect("record future generation");

        let verification = verify_installed_windows_runtime(&user_root, &future_release);
        fs::remove_dir_all(&user_root).expect("remove future Runtime fixture");

        verification.expect("recorded Runtime file set must define its own generation");
    }

    #[test]
    fn installed_runtime_requires_the_recorded_main_entrypoint() {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let user_root = std::env::temp_dir().join(format!(
            "incodex-windows-runtime-entrypoint-{}-{sequence}",
            std::process::id()
        ));
        let published = publish_windows_runtime(&user_root).expect("publish current Runtime");
        fs::remove_file(published.release_dir.join("incodex-main.cjs"))
            .expect("remove recorded main entrypoint");

        let manifest_path = published.release_dir.join(MANIFEST_NAME);
        let mut manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(&manifest_path).expect("read current Runtime manifest"),
        )
        .expect("parse current Runtime manifest");
        manifest["files"]
            .as_object_mut()
            .expect("Runtime manifest files")
            .remove("incodex-main.cjs");
        let manifest_body = serde_json::to_vec_pretty(&manifest).expect("write recorded manifest");
        fs::write(&manifest_path, &manifest_body).expect("replace recorded Runtime manifest");
        let version = manifest["runtimeVersion"]
            .as_str()
            .expect("manifest Runtime version");
        let incomplete_release = format!("{version}-{}", sha256_hex(&manifest_body));
        let incomplete_dir = published
            .release_dir
            .parent()
            .expect("Runtime releases parent")
            .join(&incomplete_release);
        fs::rename(&published.release_dir, &incomplete_dir).expect("record incomplete generation");

        let verification = verify_installed_windows_runtime(&user_root, &incomplete_release);
        fs::remove_dir_all(&user_root).expect("remove incomplete Runtime fixture");

        let error = verification.expect_err("Runtime without main must be unhealthy");
        assert!(error.to_ascii_lowercase().contains("main"), "{error}");
    }
}
