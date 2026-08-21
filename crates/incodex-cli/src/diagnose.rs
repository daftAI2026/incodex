use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use incodex_asar::Archive;
use incodex_core::paths::{user_root, ASAR_REL, RUNTIME_CURRENT_NAME, RUNTIME_DIR_NAME};
use incodex_core::target_id;
use incodex_macos::{
    diagnose_spctl, has_hardened_runtime, read_architecture, read_asar_integrity, read_plist_info,
    verify_app,
};
use incodex_transaction::{journal_v2, load_journal};

use crate::diagnose_checks::{
    empty_checks, scan_journals, scan_owner_processes, scan_sessions, CheckResult, CheckStatus,
    DiagnosticChecks, DiagnosticFinding, JournalRecord,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalRuntimeReport {
    pub status: CheckStatus,
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
    pub journal_records: Vec<JournalRecord>,
    pub checks: DiagnosticChecks,
    pub findings: Vec<DiagnosticFinding>,
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
    let (external_runtime, runtime_check) = inspect_external_runtime(root);
    let runtime_version = package
        .as_ref()
        .filter(|package| package.already_patched)
        .and_then(|_| external_runtime.version.clone());
    let codesign_ok = exists && verify_app(app_path);
    let spctl = exists.then(|| diagnose_spctl(app_path));
    let (backup, backup_check) = match package
        .as_ref()
        .and_then(|package| package.install_id.as_deref())
    {
        Some(install_id) => inspect_backup(root, app_path, install_id, runtime_version.as_deref()),
        None => (None, CheckResult::checked(Vec::new())),
    };
    let (signing, signing_check) =
        inspect_signing(backup.as_ref(), spctl.as_ref(), codesign_ok, app_path);
    let owner_scan = scan_owner_processes(root);
    let session_scan = scan_sessions(root);
    let journal_scan = scan_journals(root);
    let mut checks = empty_checks();
    checks.process_identity = owner_scan.check;
    checks.orphan_sessions = session_scan.orphan_check;
    checks.chromium_residue = session_scan.chromium_check;
    checks.runtime = runtime_check;
    checks.backup = backup_check;
    checks.signing = signing_check;
    checks.journals = journal_scan.check;
    let mut findings = Vec::new();
    for check in [
        &checks.process_identity,
        &checks.orphan_sessions,
        &checks.chromium_residue,
        &checks.runtime,
        &checks.backup,
        &checks.signing,
        &checks.journals,
    ] {
        findings.extend(check.findings.clone());
    }
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
        stale_pid: owner_scan.stale_pid,
        orphan_sessions: session_scan.orphan_sessions,
        leftover_chromium: session_scan.leftover_chromium,
        asar_loader_only: archive.as_ref().map(Archive::has_only_loader),
        external_runtime,
        signing,
        spctl,
        interrupted_transactions: journal_scan
            .interrupted
            .into_iter()
            .map(|(install_id, phase, action)| InterruptedTransaction {
                install_id,
                phase,
                action: action.as_str().to_string(),
            })
            .collect(),
        journal_records: journal_scan.records,
        checks,
        findings,
    }
}

fn inspect_external_runtime(root: &Path) -> (ExternalRuntimeReport, CheckResult) {
    let runtime_root = root.join(RUNTIME_DIR_NAME);
    let current = runtime_root.join(RUNTIME_CURRENT_NAME);
    if is_symlink(&runtime_root) {
        let error = "runtime root is a symlink".to_string();
        return (
            ExternalRuntimeReport {
                status: CheckStatus::Checked,
                present: true,
                ok: false,
                version: None,
                release: None,
                error: Some(error.clone()),
            },
            CheckResult::checked(vec![DiagnosticFinding::warning(
                "runtime.symlink",
                error,
                Some(&runtime_root),
            )]),
        );
    }
    if is_symlink(&current) {
        let error = "current.json is a symlink".to_string();
        return (
            ExternalRuntimeReport {
                status: CheckStatus::Checked,
                present: true,
                ok: false,
                version: None,
                release: None,
                error: Some(error.clone()),
            },
            CheckResult::checked(vec![DiagnosticFinding::warning(
                "runtime.symlink",
                error,
                Some(&current),
            )]),
        );
    }
    if !current.exists() {
        let report = ExternalRuntimeReport {
            status: CheckStatus::Checked,
            present: false,
            ok: false,
            version: None,
            release: None,
            error: Some("missing current.json".to_string()),
        };
        return (
            report,
            CheckResult::checked(vec![DiagnosticFinding::info(
                "runtime.missing",
                "external Runtime has not been published",
                Some(&current),
            )]),
        );
    }
    match verify_external_runtime(root, &current) {
        Ok((version, release)) => (
            ExternalRuntimeReport {
                status: CheckStatus::Checked,
                present: true,
                ok: true,
                version: Some(version),
                release: Some(release),
                error: None,
            },
            CheckResult::checked(Vec::new()),
        ),
        Err(error) => (
            ExternalRuntimeReport {
                status: CheckStatus::Checked,
                present: true,
                ok: false,
                version: None,
                release: None,
                error: Some(error.clone()),
            },
            CheckResult::checked(vec![DiagnosticFinding::warning(
                if error_contains_symlink(&error) {
                    "runtime.symlink"
                } else {
                    "runtime.invalid"
                },
                error,
                Some(&current),
            )]),
        ),
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
    reject_symlink(&runtime_root, "runtime root")?;
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

fn inspect_signing(
    backup: Option<&serde_json::Value>,
    spctl: Option<&serde_json::Value>,
    codesign_ok: bool,
    app_path: &Path,
) -> (Option<serde_json::Value>, CheckResult) {
    let Some(spctl) = spctl else {
        return (
            None,
            CheckResult::unknown(
                "signing.not-checked",
                "the application does not exist, so nested signing was not inspected",
            ),
        );
    };
    if backup.is_none() {
        return (
            None,
            CheckResult::unknown(
                "signing.not-checked",
                "no verified install backup binds signing diagnostics to this target",
            ),
        );
    }
    let report = serde_json::json!({
        "status": "unknown",
        "verified": codesign_ok,
        "componentCount": serde_json::Value::Null,
        "hardenedRuntimeOk": has_hardened_runtime(app_path),
        "unretainable": serde_json::Value::Null,
        "spctl": spctl,
    });
    (
        Some(report),
        CheckResult::unknown(
            "signing.components-unknown",
            "nested signing components and entitlement retention were not inspected",
        ),
    )
}

fn inspect_backup(
    root: &Path,
    app_path: &Path,
    install_id: &str,
    runtime_version: Option<&str>,
) -> (Option<serde_json::Value>, CheckResult) {
    let journal = match journal_v2(root, install_id) {
        Ok(journal) => journal,
        Err(error) => {
            let legacy = load_journal(install_id, root).is_some();
            let message = if legacy {
                "legacy TypeScript journal is visible but not a native backup proof"
            } else {
                "native backup journal is missing or malformed"
            };
            return (
                Some(serde_json::json!({
                    "status": "unknown",
                    "complete": false,
                    "originalExists": false,
                    "error": error,
                    "legacy": legacy,
                })),
                CheckResult::unknown("backup.unverified", message),
            );
        }
    };
    let target = fs::canonicalize(app_path).unwrap_or_else(|_| app_path.to_path_buf());
    let journal_target = fs::canonicalize(&journal.target.real_path)
        .unwrap_or_else(|_| PathBuf::from(&journal.target.real_path));
    let original = root
        .join("transactions")
        .join(install_id)
        .join(&journal.paths.original);
    let belongs_to_target = target == journal_target;
    let complete = journal.phase == "COMMITTED";
    let original_exists = original.exists();
    let mut findings = Vec::new();
    if !belongs_to_target {
        findings.push(DiagnosticFinding::warning(
            "backup.target-mismatch",
            "backup journal target does not match the inspected application",
            Some(&original),
        ));
    }
    if !original_exists {
        findings.push(DiagnosticFinding::warning(
            "backup.missing",
            "native original backup is missing",
            Some(&original),
        ));
    }
    (
        Some(serde_json::json!({
            "status": "checked",
            "belongsToTarget": belongs_to_target,
            "complete": complete,
            "originalExists": original_exists,
            "runtimeVersion": runtime_version,
            "originalAsarFileHash": hash_file(&original.join(ASAR_REL)),
            "patchedAsarFileHash": hash_file(&app_path.join(ASAR_REL)),
        })),
        CheckResult::checked(findings),
    )
}

fn error_contains_symlink(error: &str) -> bool {
    error.contains("symlink")
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
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
