use std::fs;
use std::path::Path;

use incodex_transaction::{journal_v2, parse_journal, recover_action, recover_action_phase};
use serde::Serialize;

#[path = "diagnose_fs.rs"]
mod diagnose_fs;
#[path = "diagnose_sessions.rs"]
mod diagnose_sessions;
use diagnose_fs::{
    basename, file_name, is_directory, is_symlink, live_process_identity, looks_like_uuid,
    pid_alive, read_directory,
};
pub use diagnose_sessions::scan_sessions;

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
    if is_symlink(&targets) {
        return ProcessScan {
            stale_pid: false,
            check: CheckResult {
                status: CheckStatus::Unknown,
                findings: vec![DiagnosticFinding::warning(
                    "owner.targets-symlink",
                    "Runtime targets root is a symlink and was not inspected",
                    Some(&targets),
                )],
            },
        };
    }
    let entries = match read_directory(&targets) {
        Ok(Some(entries)) => entries,
        Ok(None) => {
            return ProcessScan {
                stale_pid: false,
                check: CheckResult::checked(Vec::new()),
            };
        }
        Err(error) => {
            return ProcessScan {
                stale_pid: false,
                check: CheckResult {
                    status: CheckStatus::Unknown,
                    findings: vec![DiagnosticFinding::warning(
                        "owner.scan-failed",
                        format!("cannot enumerate Runtime owner records: {error}"),
                        Some(&targets),
                    )],
                },
            };
        }
    };
    let mut findings = Vec::new();
    let mut stale_pid = false;
    let mut unknown = false;
    for target in entries {
        if is_symlink(&target) {
            unknown = true;
            findings.push(DiagnosticFinding::warning(
                "owner.target-symlink",
                "Runtime target is a symlink and was not inspected",
                Some(&target),
            ));
            continue;
        }
        if !is_directory(&target) {
            continue;
        }
        let records = match read_directory(&target) {
            Ok(Some(records)) => records,
            Ok(None) => {
                unknown = true;
                findings.push(DiagnosticFinding::warning(
                    "owner.scan-failed",
                    "target owner records disappeared during enumeration",
                    Some(&target),
                ));
                continue;
            }
            Err(error) => {
                unknown = true;
                findings.push(DiagnosticFinding::warning(
                    "owner.scan-failed",
                    format!("cannot enumerate target owner records: {error}"),
                    Some(&target),
                ));
                continue;
            }
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

pub fn scan_journals(root: &Path, current_install_id: Option<&str>) -> JournalScan {
    let dir = root.join("transactions");
    if is_symlink(&dir) {
        return JournalScan {
            interrupted: Vec::new(),
            records: Vec::new(),
            check: CheckResult {
                status: CheckStatus::Unknown,
                findings: vec![DiagnosticFinding::warning(
                    "journal.root-symlink",
                    "transactions root is a symlink and was not inspected",
                    Some(&dir),
                )],
            },
        };
    }
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
    let entries = match read_directory(&dir) {
        Ok(Some(entries)) => entries,
        Ok(None) => {
            return JournalScan {
                interrupted: Vec::new(),
                records: Vec::new(),
                check: CheckResult::unknown(
                    "journal.scan-failed",
                    "transactions disappeared during enumeration",
                ),
            };
        }
        Err(error) => {
            return JournalScan {
                interrupted: Vec::new(),
                records: Vec::new(),
                check: CheckResult::unknown(
                    "journal.scan-failed",
                    format!("cannot enumerate transactions: {error}"),
                ),
            };
        }
    };
    let mut records = Vec::new();
    let mut interrupted = Vec::new();
    let mut findings = Vec::new();
    let mut unknown = false;
    for path in entries {
        if is_symlink(&path) {
            let is_transaction = looks_like_uuid(&file_name(&path));
            let (kind, code, message) = if is_transaction {
                (
                    "transaction symlink",
                    "journal.transaction-symlink",
                    "transaction directory is a symlink and was not inspected",
                )
            } else {
                (
                    "journal file symlink",
                    "journal.file-symlink",
                    "journal file is a symlink and was not inspected",
                )
            };
            unknown = true;
            records.push(JournalRecord {
                kind: "symlink".to_string(),
                path: path.display().to_string(),
                install_id: None,
                phase: None,
                action: None,
                error: Some(message.to_string()),
            });
            findings.push(DiagnosticFinding::warning(
                code,
                format!("{kind} was not inspected"),
                Some(&path),
            ));
            continue;
        }
        if is_directory(&path) {
            let install_id = file_name(&path);
            let journal_path = path.join("journal.json");
            if is_symlink(&journal_path) {
                unknown = true;
                records.push(JournalRecord {
                    kind: "symlink".to_string(),
                    path: journal_path.display().to_string(),
                    install_id: Some(install_id),
                    phase: None,
                    action: None,
                    error: Some("journal file is a symlink and was not inspected".to_string()),
                });
                findings.push(DiagnosticFinding::warning(
                    "journal.file-symlink",
                    "journal file is a symlink and was not inspected",
                    Some(&journal_path),
                ));
                continue;
            }
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
#[cfg(test)]
#[path = "diagnose_checks_tests.rs"]
mod tests;
