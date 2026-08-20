//! Embed and publish Electron runtime files from committed `dist/`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use sha2::{Digest, Sha256};

const DIR_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;

const LOADER: &str = include_str!("../../../dist/incodex-loader.cjs");
const INJECT: &str = include_str!("../../../dist/incodex-inject.js");
const MAIN: &str = include_str!("../../../dist/incodex-main.cjs");
const PRELOAD: &str = include_str!("../../../dist/incodex-preload.cjs");
const SAFE_HOME: &str = include_str!("../../../dist/incodex-safe-home.cjs");
const IPC_GUARD: &str = include_str!("../../../dist/incodex-ipc-guard.cjs");
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
    ("incodex-instance.cjs", INSTANCE),
    ("incodex-window-kind.cjs", WINDOW_KIND),
    ("incodex-runtime-load.cjs", RUNTIME_LOAD),
];

#[derive(Debug, Clone)]
pub struct PublishedRuntime {
    pub version: String,
    pub release: String,
}

pub fn loader_source() -> &'static str {
    LOADER
}

/// Return the fixed set of artifacts that the external loader requires.
pub fn required_runtime_files() -> impl Iterator<Item = &'static str> {
    EXTERNAL_FILES.iter().map(|(name, _)| *name)
}

pub fn runtime_version() -> String {
    serde_json::from_str::<serde_json::Value>(MANIFEST)
        .ok()
        .and_then(|raw| {
            raw.get("runtimeVersion")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

pub fn publish(user_root: &Path) -> Result<PublishedRuntime, String> {
    let version = runtime_version();
    let root = user_root.join("runtime");
    mkdir_mode(&root)?;
    let releases = root.join("releases");
    mkdir_mode(&releases)?;
    let staging = releases.join(format!(".staging-{version}-{}", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|err| err.to_string())?;
    }
    mkdir_mode(&staging)?;
    let mut files = serde_json::Map::new();
    for (name, body) in EXTERNAL_FILES {
        let dest = staging.join(name);
        write_mode(&dest, body.as_bytes())?;
        files.insert((*name).to_string(), serde_json::Value::String(sha256_hex(body.as_bytes())));
    }
    let release_rel = format!("releases/{version}");
    let dest = root.join("releases").join(&version);
    if dest.exists() {
        fs::remove_dir_all(&dest).map_err(|err| err.to_string())?;
    }
    fs::rename(&staging, &dest).map_err(|err| err.to_string())?;
    let current = serde_json::json!({
        "schemaVersion": 1,
        "version": version,
        "release": release_rel,
        "files": files,
    });
    write_mode(
        &root.join("current.json"),
        format!("{}\n", serde_json::to_string_pretty(&current).map_err(|err| err.to_string())?).as_bytes(),
    )?;
    Ok(PublishedRuntime {
        version,
        release: release_rel,
    })
}

fn mkdir_mode(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|err| err.to_string())?;
    let mut perms = fs::metadata(path).map_err(|err| err.to_string())?.permissions();
    perms.set_mode(DIR_MODE);
    fs::set_permissions(path, perms).map_err(|err| err.to_string())
}

fn write_mode(path: &Path, body: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        mkdir_mode(parent)?;
    }
    fs::write(path, body).map_err(|err| err.to_string())?;
    let mut perms = fs::metadata(path).map_err(|err| err.to_string())?.permissions();
    perms.set_mode(FILE_MODE);
    fs::set_permissions(path, perms).map_err(|err| err.to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loader_is_embedded() {
        assert!(loader_source().contains("incodex") || loader_source().len() > 10);
        assert!(!runtime_version().is_empty());
    }
}
