use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::legacy_typescript::{load_legacy_ts_v1, LegacyState};
use incodex_asar::Archive;
use incodex_core::paths::{user_root, ASAR_REL, RUNTIME_CURRENT_NAME, RUNTIME_DIR_NAME};
use incodex_core::{canonical::canonical_path, is_official_app, target_id};
use incodex_macos::{
    diagnose_spctl, has_hardened_runtime, read_architecture, read_asar_integrity, read_plist_info,
    verify_app,
};
use incodex_transaction::{journal_v2, list_interrupted, tree_digest};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalRuntimeReport {
    pub present: bool,
    pub ok: bool,
    pub version: Option<String>,
    pub release: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterruptedTransaction {
    pub install_id: String,
    pub phase: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    pub target: String,
    pub target_id: String,
    pub exists: bool,
    pub patched: bool,
    pub bundle_id: Option<String>,
    pub app_version: Option<String>,
    pub app_build: Option<String>,
    pub architecture: Option<String>,
    pub asar_file_hash: Option<String>,
    pub asar_header_hash: Option<String>,
    pub plist_file_hash: Option<String>,
    pub plist_integrity_hash: Option<String>,
    pub runtime_version: Option<String>,
    pub original_main: String,
    pub codesign_ok: bool,
    pub backup: Option<serde_json::Value>,
    pub stale_pid: bool,
    pub orphan_sessions: Vec<String>,
    pub leftover_chromium: Vec<String>,
    pub asar_loader_only: Option<bool>,
    pub external_runtime: ExternalRuntimeReport,
    pub signing: Option<serde_json::Value>,
    pub spctl: Option<serde_json::Value>,
    pub interrupted_transactions: Vec<InterruptedTransaction>,
}

pub fn diagnose(app_path: &Path) -> Diagnosis {
    diagnose_with_root(app_path, &user_root())
}

pub fn diagnose_with_root(app_path: &Path, root: &Path) -> Diagnosis {
    let exists = app_path.exists();
    let asar_path = app_path.join(ASAR_REL);
    let asar_exists = asar_path.exists();
    let plist_path = app_path.join("Contents/Info.plist");
    let plist = exists.then(|| read_plist_info(app_path)).flatten();
    let archive = asar_exists
        .then(|| Archive::open(&asar_path).ok())
        .flatten();
    let package = archive
        .as_ref()
        .and_then(|archive| archive.read_package_main().ok());
    let external_runtime = inspect_external_runtime(root);
    let runtime_version = package
        .as_ref()
        .filter(|package| package.already_patched)
        .and_then(|_| external_runtime.version.clone());
    let codesign_ok = exists && verify_app(app_path);
    let spctl = exists.then(|| diagnose_spctl(app_path));
    let backup = package
        .as_ref()
        .and_then(|package| package.install_id.as_deref())
        .and_then(|install_id| {
            inspect_backup(root, app_path, install_id, runtime_version.as_deref())
        })
        .or_else(|| inspect_legacy_backup(root, app_path));
    let signing = backup.as_ref().and_then(|_| {
        spctl.as_ref().map(|spctl| {
            serde_json::json!({
                "verified": codesign_ok,
                "componentCount": 0,
                "hardenedRuntimeOk": has_hardened_runtime(app_path),
                "unretainable": [],
                "spctl": spctl,
            })
        })
    });
    Diagnosis {
        target: app_path.display().to_string(),
        target_id: target_id(app_path),
        exists,
        patched: package
            .as_ref()
            .map(|package| package.already_patched)
            .unwrap_or(false),
        bundle_id: plist
            .as_ref()
            .map(|plist| plist.bundle_identifier.clone())
            .filter(|value| !value.is_empty()),
        app_version: plist
            .as_ref()
            .map(|plist| plist.app_version.clone())
            .filter(|value| !value.is_empty()),
        app_build: plist
            .as_ref()
            .map(|plist| plist.app_build.clone())
            .filter(|value| !value.is_empty()),
        architecture: plist
            .as_ref()
            .and_then(|plist| read_architecture(app_path, &plist.executable)),
        asar_file_hash: archive.as_ref().map(Archive::file_hash),
        asar_header_hash: archive.as_ref().map(Archive::header_hash),
        plist_file_hash: hash_file(&plist_path),
        plist_integrity_hash: exists.then(|| read_asar_integrity(app_path)).flatten(),
        runtime_version,
        original_main: package.map(|package| package.main).unwrap_or_default(),
        codesign_ok,
        backup,
        stale_pid: false,
        orphan_sessions: Vec::new(),
        leftover_chromium: Vec::new(),
        asar_loader_only: archive.as_ref().map(Archive::has_only_loader),
        external_runtime,
        signing,
        spctl,
        interrupted_transactions: list_interrupted(root)
            .into_iter()
            .map(|(install_id, phase, action)| InterruptedTransaction {
                install_id,
                phase,
                action: action.as_str().to_string(),
            })
            .collect(),
    }
}

fn inspect_external_runtime(root: &Path) -> ExternalRuntimeReport {
    let current = root.join(RUNTIME_DIR_NAME).join(RUNTIME_CURRENT_NAME);
    if !current.exists() {
        return ExternalRuntimeReport {
            present: false,
            ok: false,
            version: None,
            release: None,
            error: Some("missing current.json".to_string()),
        };
    }
    match verify_external_runtime(root, &current) {
        Ok((version, release)) => ExternalRuntimeReport {
            present: true,
            ok: true,
            version: Some(version),
            release: Some(release),
            error: None,
        },
        Err(error) => ExternalRuntimeReport {
            present: true,
            ok: false,
            version: None,
            release: None,
            error: Some(error),
        },
    }
}

fn verify_external_runtime(root: &Path, current_path: &Path) -> Result<(String, String), String> {
    let body = fs::read(current_path).map_err(|error| error.to_string())?;
    let current: serde_json::Value =
        serde_json::from_slice(&body).map_err(|error| error.to_string())?;
    if current
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
    {
        return Err("invalid current.json schema".to_string());
    }
    let version = required_string(&current, "version")?;
    let release = required_string(&current, "release")?;
    if !safe_relative(&release) {
        return Err("runtime release is not a safe relative path".to_string());
    }
    let runtime_root = root.join(RUNTIME_DIR_NAME);
    let release_dir = runtime_root.join(&release);
    reject_symlink(&release_dir, "runtime release")?;
    let release_real = fs::canonicalize(&release_dir).map_err(|error| error.to_string())?;
    let runtime_real = fs::canonicalize(&runtime_root).map_err(|error| error.to_string())?;
    if !release_real.starts_with(&runtime_real) {
        return Err("runtime release escaped runtime root".to_string());
    }
    let files = current
        .get("files")
        .and_then(serde_json::Value::as_object)
        .ok_or("runtime files are missing")?;
    if files.is_empty() {
        return Err("runtime files are empty".to_string());
    }
    for name in incodex_runtime_bundle::required_runtime_files() {
        if !files.contains_key(name) {
            return Err(format!("runtime file is missing: {name}"));
        }
    }
    for (name, expected) in files {
        if !safe_relative(name) {
            return Err(format!("runtime file is not a safe relative path: {name}"));
        }
        let expected = expected
            .as_str()
            .filter(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or_else(|| format!("runtime hash is invalid: {name}"))?;
        let path = release_dir.join(name);
        reject_symlink(&path, "runtime file")?;
        let file_real = fs::canonicalize(&path).map_err(|error| error.to_string())?;
        if !file_real.starts_with(&release_real) {
            return Err(format!("runtime file escaped release: {name}"));
        }
        let actual = hash_file(&path).ok_or_else(|| format!("runtime file is missing: {name}"))?;
        if actual != expected {
            return Err(format!("runtime hash mismatch: {name}"));
        }
    }
    Ok((version, release))
}

fn inspect_backup(
    root: &Path,
    app_path: &Path,
    install_id: &str,
    runtime_version: Option<&str>,
) -> Option<serde_json::Value> {
    let journal = journal_v2(root, install_id).ok()?;
    let target = fs::canonicalize(app_path).unwrap_or_else(|_| app_path.to_path_buf());
    let journal_target = fs::canonicalize(&journal.target.real_path)
        .unwrap_or_else(|_| PathBuf::from(&journal.target.real_path));
    let original = root
        .join("transactions")
        .join(install_id)
        .join(&journal.paths.original);
    Some(serde_json::json!({
        "belongsToTarget": target == journal_target,
        "complete": journal.phase == "COMMITTED",
        "originalExists": original.exists(),
        "runtimeVersion": runtime_version,
        "originalAsarFileHash": hash_file(&original.join(ASAR_REL)),
        "patchedAsarFileHash": hash_file(&app_path.join(ASAR_REL)),
    }))
}

fn inspect_legacy_backup(root: &Path, app_path: &Path) -> Option<serde_json::Value> {
    let state = load_legacy_ts_v1(root, app_path).ok()??;
    let LegacyState::Committed {
        manifest,
        original_app,
        ..
    } = state.state
    else {
        return None;
    };
    let belongs_to_target = canonical_path(app_path) == canonical_path(&manifest.target_real_path);
    let original_exists = original_app.is_dir();
    let original_asar_hash = hash_file(&original_app.join(ASAR_REL));
    let patched_asar_hash = hash_file(&app_path.join(ASAR_REL));
    let original_plist_hash = hash_file(&original_app.join("Contents/Info.plist"));
    let original_tree_hash = tree_digest(&original_app).ok();
    let original_info = read_plist_info(&original_app);
    let original_path_integrity = original_info.as_ref().is_some_and(|info| {
        [
            &original_app.join("Contents/Info.plist"),
            &original_app.join(ASAR_REL),
            &original_app.join("Contents/MacOS").join(&info.executable),
        ]
        .into_iter()
        .all(|path| reject_symlink(path, "legacy backup").is_ok())
    });
    let original_signature = if is_official_app(app_path, None) {
        crate::legacy_proof::verify_official_vendor_bundle(
            &original_app,
            &manifest.bundle_identifier,
        )
        .is_ok()
    } else {
        verify_app(&original_app)
    };
    let complete = belongs_to_target
        && original_exists
        && original_asar_hash.as_deref() == Some(manifest.original_asar_file_hash.as_str())
        && patched_asar_hash.as_deref() == Some(manifest.patched_asar_file_hash.as_str())
        && original_plist_hash.as_deref() == Some(manifest.original_plist_file_hash.as_str())
        && original_path_integrity
        && original_signature;
    Some(serde_json::json!({
        "legacy": true,
        "complete": complete,
        "installId": state.install_id,
        "belongsToTarget": belongs_to_target,
        "originalExists": original_exists,
        "originalAsarFileHash": original_asar_hash,
        "patchedAsarFileHash": patched_asar_hash,
        "originalPathIntegrity": original_path_integrity,
        "originalTreeDigest": original_tree_hash,
        "originalTreeProof": "not recorded by TS v1",
        "originalSignatureOk": original_signature,
        "runtimeVersion": manifest.runtime_version,
    }))
}

fn required_string(raw: &serde_json::Value, key: &str) -> Result<String, String> {
    raw.get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("runtime {key} is missing"))
}

fn safe_relative(value: &str) -> bool {
    !value.is_empty()
        && !Path::new(value).is_absolute()
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn reject_symlink(path: &Path, label: &str) -> Result<(), String> {
    if fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(format!("{label} is a symlink: {}", path.display()));
    }
    Ok(())
}

fn hash_file(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut hash = Sha256::new();
    let mut chunk = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        hash.update(&chunk[..read]);
    }
    Some(
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}

pub fn format_status(report: &Diagnosis) -> String {
    let mut lines = vec![incodex_core::format_step("Status", None)];
    let app_path = Path::new(&report.target);
    if !report.exists {
        lines.push(incodex_core::format_warn(
            &format!("Codex app not found: {}", app_path.display()),
            None,
        ));
        lines.push(String::new());
        return lines.join("\n");
    }
    lines.push(incodex_core::format_kv("App", &report.target, None));
    lines.push(incodex_core::format_kv(
        "Exists",
        if report.exists { "yes" } else { "no" },
        None,
    ));
    lines.push(incodex_core::format_kv(
        "Installed",
        if report.patched { "yes" } else { "no" },
        None,
    ));
    if report.patched {
        if let Some(loader_only) = report.asar_loader_only {
            lines.push(incodex_core::format_kv(
                "Loader",
                if loader_only {
                    "asar loader only"
                } else {
                    "mixed"
                },
                None,
            ));
        }
    }
    let runtime = if app_path.join(ASAR_REL).exists() {
        runtime_description(&report)
    } else {
        "missing".to_string()
    };
    lines.push(incodex_core::format_kv("Runtime", &runtime, None));
    if report.patched {
        if let Some(version) = app_version_description(&report) {
            lines.push(incodex_core::format_kv("Version", &version, None));
        }
        if let Some(package) = Archive::open(app_path.join(ASAR_REL))
            .ok()
            .and_then(|archive| archive.read_package_main().ok())
        {
            if let Some(install_id) = package.install_id {
                lines.push(incodex_core::format_kv("Install id", &install_id, None));
            }
        }
    }
    lines.push(incodex_core::format_kv("Target", &report.target_id, None));
    if !app_path.join(ASAR_REL).exists() {
        lines.push(incodex_core::format_warn("asar missing", None));
    } else if report.patched {
        if !report.original_main.is_empty() {
            lines.push(incodex_core::format_kv("Main", &report.original_main, None));
        }
        lines.push(incodex_core::format_ok(
            "Incodex is installed. Use doctor for hashes and signing.",
            None,
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

pub fn format_diagnosis(report: &Diagnosis) -> String {
    let runtime = if report.external_runtime.ok {
        format!(
            "{} {}",
            report
                .external_runtime
                .version
                .as_deref()
                .unwrap_or("unknown"),
            report.external_runtime.release.as_deref().unwrap_or("")
        )
        .trim()
        .to_string()
    } else if report.external_runtime.present {
        "invalid".to_string()
    } else {
        "missing".to_string()
    };
    let backup = match report.backup.as_ref() {
        Some(backup) if json_bool(backup, "originalExists") && json_bool(backup, "complete") => {
            "ok"
        }
        Some(_) => "incomplete",
        None => "none",
    };
    let loader = match report.asar_loader_only {
        None => "unknown",
        Some(true) => "asar only",
        Some(false) => "mixed",
    };
    let app_version = format!(
        "{} {}",
        report.app_version.as_deref().unwrap_or("unknown"),
        report.app_build.as_deref().unwrap_or("")
    );
    let mut lines = vec![
        incodex_core::format_step("App", None),
        incodex_core::format_kv("Path", &report.target, None),
        incodex_core::format_kv("Exists", if report.exists { "yes" } else { "no" }, None),
        incodex_core::format_kv("Installed", if report.patched { "yes" } else { "no" }, None),
        incodex_core::format_kv(
            "Bundle",
            report.bundle_id.as_deref().unwrap_or("unknown"),
            None,
        ),
        incodex_core::format_kv("Version", app_version.trim(), None),
        incodex_core::format_kv(
            "Arch",
            report.architecture.as_deref().unwrap_or("unknown"),
            None,
        ),
        String::new(),
        incodex_core::format_step("Runtime", None),
        incodex_core::format_kv(
            "Version",
            report.runtime_version.as_deref().unwrap_or("unknown"),
            None,
        ),
        incodex_core::format_kv("External", &runtime, None),
    ];
    if let Some(error) = &report.external_runtime.error {
        lines.push(incodex_core::format_warn(error, None));
    }
    let main = if report.original_main.is_empty() {
        "unknown"
    } else {
        report.original_main.as_str()
    };
    lines.extend([
        incodex_core::format_kv("Loader", loader, None),
        incodex_core::format_kv("Main", main, None),
        String::new(),
        incodex_core::format_step("Signing", None),
        incodex_core::format_kv(
            "Verify",
            if report.codesign_ok { "ok" } else { "failed" },
            None,
        ),
    ]);
    if let Some(signing) = &report.signing {
        lines.push(incodex_core::format_kv(
            "Hardened",
            if json_bool(signing, "hardenedRuntimeOk") {
                "yes"
            } else {
                "no"
            },
            None,
        ));
        let dropped = signing
            .get("unretainable")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "none".to_string());
        lines.push(incodex_core::format_kv("Dropped", &dropped, None));
    }
    if let Some(spctl) = &report.spctl {
        lines.push(incodex_core::format_kv(
            "Gatekeeper",
            if json_bool(spctl, "accepted") {
                "accepted"
            } else {
                "not accepted (diagnostic)"
            },
            None,
        ));
    }
    lines.extend([
        String::new(),
        incodex_core::format_step("Backup", None),
        incodex_core::format_kv("State", backup, None),
    ]);
    if let Some(backup) = &report.backup {
        lines.push(incodex_core::format_kv(
            "Matches",
            if json_bool(backup, "belongsToTarget") {
                "yes"
            } else {
                "no"
            },
            None,
        ));
    }
    lines.extend([
        String::new(),
        incodex_core::format_step("Sessions", None),
        incodex_core::format_kv("Orphans", &report.orphan_sessions.len().to_string(), None),
        incodex_core::format_kv(
            "Chromium",
            &report.leftover_chromium.len().to_string(),
            None,
        ),
        incodex_core::format_kv(
            "Stale pid",
            if report.stale_pid { "yes" } else { "no" },
            None,
        ),
        incodex_core::format_kv(
            "Journals",
            &report.interrupted_transactions.len().to_string(),
            None,
        ),
    ]);
    for item in &report.interrupted_transactions {
        lines.push(incodex_core::format_kv(
            "Journal",
            &format!("{}  {} -> {}", item.install_id, item.phase, item.action),
            None,
        ));
    }
    if !report.interrupted_transactions.is_empty() {
        lines.push(incodex_core::format_warn(
            "Old install journals are leftover. They do not mean the current app is broken.",
            None,
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

pub fn diagnosis_json(report: &Diagnosis) -> String {
    format!("{}\n", serde_json::to_string_pretty(report).expect("json"))
}

fn runtime_description(report: &Diagnosis) -> String {
    if report.external_runtime.ok {
        format!(
            "{} {}",
            report
                .external_runtime
                .version
                .as_deref()
                .unwrap_or("unknown"),
            report.external_runtime.release.as_deref().unwrap_or("")
        )
        .trim()
        .to_string()
    } else if report.external_runtime.present {
        "invalid".to_string()
    } else {
        "missing".to_string()
    }
}

fn app_version_description(report: &Diagnosis) -> Option<String> {
    report.app_version.as_ref().map(|version| {
        format!("{} {}", version, report.app_build.as_deref().unwrap_or(""))
            .trim()
            .to_string()
    })
}

fn json_bool(value: &serde_json::Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}
