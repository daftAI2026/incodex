use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use incodex_transaction::{journal_v2, parse_journal, recover_action, recover_action_phase};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Checked,
    Unknown,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticFinding {
    pub code: String,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl DiagnosticFinding {
    pub fn warning(code: &str, message: impl Into<String>, path: Option<&Path>) -> Self {
        Self {
            code: code.to_string(),
            severity: "warning".to_string(),
            message: message.into(),
            path: path.map(|path| path.display().to_string()),
        }
    }

    pub fn info(code: &str, message: impl Into<String>, path: Option<&Path>) -> Self {
        Self {
            code: code.to_string(),
            severity: "info".to_string(),
            message: message.into(),
            path: path.map(|path| path.display().to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub status: CheckStatus,
    pub findings: Vec<DiagnosticFinding>,
}

impl CheckResult {
    pub fn checked(findings: Vec<DiagnosticFinding>) -> Self {
        Self {
            status: CheckStatus::Checked,
            findings,
        }
    }

    pub fn unknown(code: &str, message: impl Into<String>) -> Self {
        Self {
            status: CheckStatus::Unknown,
            findings: vec![DiagnosticFinding::warning(code, message, None)],
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticChecks {
    pub process_identity: CheckResult,
    pub orphan_sessions: CheckResult,
    pub chromium_residue: CheckResult,
    pub runtime: CheckResult,
    pub backup: CheckResult,
    pub journals: CheckResult,
    pub signing: CheckResult,
}

#[derive(Debug, Clone)]
pub struct ProcessScan {
    pub stale_pid: bool,
    pub check: CheckResult,
}

#[derive(Debug, Clone)]
pub struct SessionScan {
    pub orphan_sessions: Vec<String>,
    pub leftover_chromium: Vec<String>,
    pub orphan_check: CheckResult,
    pub chromium_check: CheckResult,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JournalRecord {
    pub kind: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct JournalScan {
    pub interrupted: Vec<(String, String, String)>,
    pub records: Vec<JournalRecord>,
    pub check: CheckResult,
}

pub fn empty_checks() -> DiagnosticChecks {
    DiagnosticChecks {
        process_identity: CheckResult::checked(Vec::new()),
        orphan_sessions: CheckResult::checked(Vec::new()),
        chromium_residue: CheckResult::checked(Vec::new()),
        runtime: CheckResult::checked(Vec::new()),
        backup: CheckResult::checked(Vec::new()),
        journals: CheckResult::checked(Vec::new()),
        signing: CheckResult::unknown(
            "signing.not-checked",
            "nested signing components and entitlement retention were not inspected",
        ),
    }
}

pub fn scan_owner_processes(root: &Path) -> ProcessScan {
    let targets = root.join("targets");
    let Some(entries) = read_directory(&targets) else {
        return if !targets.exists() {
            ProcessScan {
                stale_pid: false,
                check: CheckResult::checked(Vec::new()),
            }
        } else {
            ProcessScan {
                stale_pid: false,
                check: CheckResult::unknown(
                    "owner.scan-failed",
                    "cannot enumerate Runtime owner records",
                ),
            }
        };
    };
    let mut findings = Vec::new();
    let mut stale_pid = false;
    let mut unknown = false;
    for target in entries {
        if !is_directory(&target) {
            continue;
        }
        let Some(records) = read_directory(&target) else {
            unknown = true;
            findings.push(DiagnosticFinding::warning(
                "owner.scan-failed",
                "cannot enumerate target owner records",
                Some(&target),
            ));
            continue;
        };
        for record in records.into_iter().filter(|path| {
            path.file_name().and_then(|name| name.to_str()) == Some("incognito.lock")
                || path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("incognito.lock.active."))
        }) {
            if is_symlink(&record) {
                unknown = true;
                findings.push(DiagnosticFinding::warning(
                    "owner.symlink",
                    "owner record is a symlink and was not inspected",
                    Some(&record),
                ));
                continue;
            }
            let body = match fs::read_to_string(&record) {
                Ok(body) => body,
                Err(error) => {
                    unknown = true;
                    findings.push(DiagnosticFinding::warning(
                        "owner.unreadable",
                        error.to_string(),
                        Some(&record),
                    ));
                    continue;
                }
            };
            let owner: serde_json::Value = match serde_json::from_str(&body) {
                Ok(owner) => owner,
                Err(error) => {
                    unknown = true;
                    findings.push(DiagnosticFinding::warning(
                        "owner.invalid",
                        error.to_string(),
                        Some(&record),
                    ));
                    continue;
                }
            };
            let pid = owner.get("pid").and_then(serde_json::Value::as_i64);
            let expected_start = owner
                .get("processStartIdentity")
                .or_else(|| owner.get("startedAt"))
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty());
            let expected_exec = owner
                .get("execIdentity")
                .or_else(|| owner.get("execPath"))
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty());
            let Some(pid) = pid.and_then(|value| i32::try_from(value).ok()) else {
                unknown = true;
                findings.push(DiagnosticFinding::warning(
                    "owner.invalid",
                    "owner record has no valid pid",
                    Some(&record),
                ));
                continue;
            };
            if expected_start.is_none() || expected_exec.is_none() {
                unknown = true;
                findings.push(DiagnosticFinding::warning(
                    "owner.identity-missing",
                    "owner record lacks process start or executable identity",
                    Some(&record),
                ));
                continue;
            }
            match live_process_identity(pid) {
                None if !pid_alive(pid) => {
                    stale_pid = true;
                    findings.push(DiagnosticFinding::warning(
                        "owner.stale",
                        format!("owner pid {pid} is no longer running"),
                        Some(&record),
                    ));
                }
                None => {
                    unknown = true;
                    findings.push(DiagnosticFinding::warning(
                        "owner.identity-unknown",
                        format!("cannot verify process identity for pid {pid}"),
                        Some(&record),
                    ));
                }
                Some(live) => {
                    let start_matches = expected_start == Some(live.start.as_str());
                    let exec_matches = expected_exec
                        .is_some_and(|expected| basename(expected) == basename(&live.exec));
                    if !start_matches || !exec_matches {
                        stale_pid = true;
                        findings.push(DiagnosticFinding::warning(
                            "owner.identity-mismatch",
                            format!("owner pid {pid} no longer identifies the recorded process"),
                            Some(&record),
                        ));
                    }
                }
            }
        }
    }
    ProcessScan {
        stale_pid,
        check: if unknown {
            CheckResult {
                status: CheckStatus::Unknown,
                findings,
            }
        } else {
            CheckResult::checked(findings)
        },
    }
}

pub fn scan_sessions(root: &Path) -> SessionScan {
    let sessions = root.join("sessions");
    if !sessions.exists() {
        return SessionScan {
            orphan_sessions: Vec::new(),
            leftover_chromium: Vec::new(),
            orphan_check: CheckResult::checked(Vec::new()),
            chromium_check: CheckResult::checked(Vec::new()),
        };
    }
    if !is_directory(&sessions) {
        return SessionScan {
            orphan_sessions: Vec::new(),
            leftover_chromium: Vec::new(),
            orphan_check: CheckResult::unknown(
                "session.scan-failed",
                "sessions path is not a directory",
            ),
            chromium_check: CheckResult::unknown(
                "chromium.scan-failed",
                "sessions path is not a directory",
            ),
        };
    }
    let mut roots = Vec::new();
    let mut orphan_findings = Vec::new();
    let mut chromium_findings = Vec::new();
    let mut unknown = false;
    let Some(targets) = read_directory(&sessions) else {
        return SessionScan {
            orphan_sessions: Vec::new(),
            leftover_chromium: Vec::new(),
            orphan_check: CheckResult::unknown("session.scan-failed", "cannot enumerate sessions"),
            chromium_check: CheckResult::unknown(
                "chromium.scan-failed",
                "cannot enumerate sessions",
            ),
        };
    };
    for child in targets {
        if !is_directory(&child) {
            continue;
        }
        if file_name_starts(&child, "s-") {
            roots.push(child);
        } else if let Some(nested) = read_directory(&child) {
            roots.extend(
                nested
                    .into_iter()
                    .filter(|path| is_directory(path) && file_name_starts(path, "s-")),
            );
        } else {
            unknown = true;
            orphan_findings.push(DiagnosticFinding::warning(
                "session.scan-failed",
                "cannot enumerate target sessions",
                Some(&child),
            ));
        }
    }
    let mut orphan_sessions = Vec::new();
    let mut leftover_chromium = Vec::new();
    for session in roots {
        let owner_path = session.join("owner.json");
        if is_symlink(&owner_path) {
            unknown = true;
            chromium_findings.push(DiagnosticFinding::warning(
                "chromium.session-unknown",
                "session owner record is unavailable; Chromium residue cannot be classified",
                Some(&owner_path),
            ));
            orphan_findings.push(DiagnosticFinding::warning(
                "session.owner-symlink",
                "session owner record is a symlink and was not inspected",
                Some(&owner_path),
            ));
            continue;
        }
        let body = match fs::read_to_string(&owner_path) {
            Ok(body) => body,
            Err(error) => {
                unknown = true;
                chromium_findings.push(DiagnosticFinding::warning(
                    "chromium.session-unknown",
                    "session owner record is unreadable; Chromium residue cannot be classified",
                    Some(&owner_path),
                ));
                orphan_findings.push(DiagnosticFinding::warning(
                    "session.owner-unreadable",
                    error.to_string(),
                    Some(&owner_path),
                ));
                continue;
            }
        };
        let owner: serde_json::Value = match serde_json::from_str(&body) {
            Ok(owner) => owner,
            Err(error) => {
                unknown = true;
                chromium_findings.push(DiagnosticFinding::warning(
                    "chromium.session-unknown",
                    "session owner record is invalid; Chromium residue cannot be classified",
                    Some(&owner_path),
                ));
                orphan_findings.push(DiagnosticFinding::warning(
                    "session.owner-invalid",
                    error.to_string(),
                    Some(&owner_path),
                ));
                continue;
            }
        };
        let pid = owner
            .get("pid")
            .and_then(serde_json::Value::as_i64)
            .and_then(|value| i32::try_from(value).ok());
        let session_id = owner
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| file_name(&session));
        let orphan = match pid {
            Some(pid) if !pid_alive(pid) => true,
            Some(pid) => match owner
                .get("processStartIdentity")
                .and_then(serde_json::Value::as_str)
            {
                None => false,
                Some(expected) => match live_process_identity(pid) {
                    Some(live) => expected != live.start,
                    None => {
                        unknown = true;
                        chromium_findings.push(DiagnosticFinding::warning(
                            "chromium.session-unknown",
                            "session process identity cannot be verified; Chromium residue cannot be classified",
                            Some(&owner_path),
                        ));
                        orphan_findings.push(DiagnosticFinding::warning(
                            "session.identity-unknown",
                            format!("cannot verify process identity for pid {pid}"),
                            Some(&owner_path),
                        ));
                        false
                    }
                },
            },
            None => {
                unknown = true;
                chromium_findings.push(DiagnosticFinding::warning(
                    "chromium.session-unknown",
                    "session owner has no valid pid; Chromium residue cannot be classified",
                    Some(&owner_path),
                ));
                orphan_findings.push(DiagnosticFinding::warning(
                    "session.owner-invalid",
                    "session owner has no valid pid",
                    Some(&owner_path),
                ));
                false
            }
        };
        if orphan {
            orphan_sessions.push(session.display().to_string());
            orphan_findings.push(DiagnosticFinding::warning(
                "session.orphan",
                format!("session {session_id} belongs to a dead or changed process"),
                Some(&session),
            ));
            for name in ["chromium", "incognito-chromium"] {
                let chromium = session.join(name);
                if is_directory(&chromium) {
                    leftover_chromium.push(chromium.display().to_string());
                    chromium_findings.push(DiagnosticFinding::warning(
                        "chromium.residue",
                        "orphan session Chromium data remains",
                        Some(&chromium),
                    ));
                }
            }
        }
    }
    for name in ["incognito-home", "incognito-chromium"] {
        let path = root.join(name);
        if is_directory(&path) {
            leftover_chromium.push(path.display().to_string());
            chromium_findings.push(DiagnosticFinding::warning(
                "chromium.residue",
                "legacy Chromium residue remains outside a session",
                Some(&path),
            ));
        }
    }
    SessionScan {
        orphan_sessions,
        leftover_chromium,
        orphan_check: if unknown {
            CheckResult {
                status: CheckStatus::Unknown,
                findings: orphan_findings,
            }
        } else {
            CheckResult::checked(orphan_findings)
        },
        chromium_check: if unknown {
            CheckResult {
                status: CheckStatus::Unknown,
                findings: chromium_findings,
            }
        } else {
            CheckResult::checked(chromium_findings)
        },
    }
}

pub fn scan_journals(root: &Path, current_install_id: Option<&str>) -> JournalScan {
    let dir = root.join("transactions");
    if !dir.exists() {
        return JournalScan {
            interrupted: Vec::new(),
            records: Vec::new(),
            check: CheckResult::checked(Vec::new()),
        };
    }
    if !is_directory(&dir) {
        return JournalScan {
            interrupted: Vec::new(),
            records: Vec::new(),
            check: CheckResult::unknown(
                "journal.scan-failed",
                "transactions path is not a directory",
            ),
        };
    }
    let Some(entries) = read_directory(&dir) else {
        return JournalScan {
            interrupted: Vec::new(),
            records: Vec::new(),
            check: CheckResult::unknown("journal.scan-failed", "cannot enumerate transactions"),
        };
    };
    let mut records = Vec::new();
    let mut interrupted = Vec::new();
    let mut findings = Vec::new();
    for path in entries {
        if is_directory(&path) {
            let install_id = file_name(&path);
            match journal_v2(root, &install_id) {
                Ok(journal) => {
                    let action = recover_action_phase(&journal.phase);
                    let kind = if action != incodex_transaction::Recovery::Done {
                        interrupted.push((
                            journal.install_id.clone(),
                            journal.phase.clone(),
                            action.as_str().to_string(),
                        ));
                        "validInterrupted"
                    } else if journal.phase == "COMMITTED"
                        && current_install_id == Some(journal.install_id.as_str())
                    {
                        "currentCommitted"
                    } else if journal.phase == "COMMITTED" {
                        "staleCommitted"
                    } else {
                        "completed"
                    };
                    records.push(JournalRecord {
                        kind: kind.to_string(),
                        path: path.display().to_string(),
                        install_id: Some(journal.install_id),
                        phase: Some(journal.phase),
                        action: Some(action.as_str().to_string()),
                        error: None,
                    });
                    if kind == "staleCommitted" {
                        findings.push(DiagnosticFinding::info(
                            "journal.stale-committed",
                            "committed transaction remains in the local journal store",
                            Some(&path),
                        ));
                    }
                }
                Err(error) => {
                    records.push(JournalRecord {
                        kind: if looks_like_uuid(&install_id) {
                            "malformed".to_string()
                        } else {
                            "unrecognizedLegacy".to_string()
                        },
                        path: path.display().to_string(),
                        install_id: Some(install_id),
                        phase: None,
                        action: None,
                        error: Some(error.clone()),
                    });
                    findings.push(DiagnosticFinding::warning(
                        "journal.malformed",
                        error,
                        Some(&path),
                    ));
                }
            }
            continue;
        }
        let body = match fs::read_to_string(&path) {
            Ok(body) => body,
            Err(error) => {
                records.push(JournalRecord {
                    kind: "malformed".to_string(),
                    path: path.display().to_string(),
                    install_id: None,
                    phase: None,
                    action: None,
                    error: Some(error.to_string()),
                });
                findings.push(DiagnosticFinding::warning(
                    "journal.malformed",
                    error.to_string(),
                    Some(&path),
                ));
                continue;
            }
        };
        match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(raw) => match parse_journal(&raw) {
                Some(journal) => {
                    let action = recover_action(&journal);
                    let kind = if action != incodex_transaction::Recovery::Done {
                        interrupted.push((
                            journal.install_id.clone(),
                            journal.phase.clone(),
                            action.as_str().to_string(),
                        ));
                        "validInterrupted"
                    } else if journal.phase == "COMMITTED"
                        && current_install_id == Some(journal.install_id.as_str())
                    {
                        "currentCommitted"
                    } else if journal.phase == "COMMITTED" {
                        "staleCommitted"
                    } else {
                        "completed"
                    };
                    records.push(JournalRecord {
                        kind: kind.to_string(),
                        path: path.display().to_string(),
                        install_id: Some(journal.install_id),
                        phase: Some(journal.phase),
                        action: Some(action.as_str().to_string()),
                        error: None,
                    });
                    if kind == "staleCommitted" {
                        findings.push(DiagnosticFinding::info(
                            "journal.stale-committed",
                            "committed legacy transaction remains in the local journal store",
                            Some(&path),
                        ));
                    }
                }
                None => {
                    let kind = if raw.get("schemaVersion").and_then(serde_json::Value::as_u64)
                        == Some(1)
                    {
                        "malformed"
                    } else {
                        "unrecognizedLegacy"
                    };
                    records.push(JournalRecord {
                        kind: kind.to_string(),
                        path: path.display().to_string(),
                        install_id: raw
                            .get("installId")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string),
                        phase: raw
                            .get("phase")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string),
                        action: None,
                        error: Some("journal schema is not recognized".to_string()),
                    });
                    findings.push(DiagnosticFinding::warning(
                        if kind == "malformed" {
                            "journal.malformed"
                        } else {
                            "journal.unrecognized-legacy"
                        },
                        "journal schema is not recognized",
                        Some(&path),
                    ));
                }
            },
            Err(error) => {
                records.push(JournalRecord {
                    kind: "malformed".to_string(),
                    path: path.display().to_string(),
                    install_id: None,
                    phase: None,
                    action: None,
                    error: Some(error.to_string()),
                });
                findings.push(DiagnosticFinding::warning(
                    "journal.malformed",
                    error.to_string(),
                    Some(&path),
                ));
            }
        }
    }
    JournalScan {
        interrupted,
        records,
        check: CheckResult::checked(findings),
    }
}

fn read_directory(path: &Path) -> Option<Vec<PathBuf>> {
    fs::read_dir(path)
        .ok()
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
}

fn is_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

fn file_name_starts(path: &Path, prefix: &str) -> bool {
    file_name(path).starts_with(prefix)
}

fn looks_like_uuid(value: &str) -> bool {
    value.len() == 36
        && value.as_bytes().iter().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn basename(value: &str) -> &str {
    value.rsplit('/').next().unwrap_or(value)
}

#[derive(Debug, Clone)]
struct LiveProcess {
    start: String,
    exec: String,
}

fn live_process_identity(pid: i32) -> Option<LiveProcess> {
    if pid <= 0 {
        return None;
    }
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart=,comm="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let (start, exec) = line.rsplit_once(char::is_whitespace)?;
    if start.trim().is_empty() || exec.trim().is_empty() {
        return None;
    }
    Some(LiveProcess {
        start: start.trim().to_string(),
        exec: exec.trim().to_string(),
    })
}

fn pid_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    unsafe { libc::kill(pid, 0) == 0 }
}
