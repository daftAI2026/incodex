use std::fs;
use std::path::Path;

use incodex_core::session::is_canonical_process_start_identity;

use super::{
    diagnose_fs::{
        file_name, file_name_starts, is_directory, is_symlink, live_process_identity, pid_alive,
        read_directory,
    },
    CheckResult, DiagnosticFinding, SessionScan,
};

fn mark_pair(
    unknown: &mut bool,
    orphan_findings: &mut Vec<DiagnosticFinding>,
    chromium_findings: &mut Vec<DiagnosticFinding>,
    code: &str,
    message: &str,
    path: &Path,
) {
    *unknown = true;
    let finding = DiagnosticFinding::warning(code, message, Some(path));
    orphan_findings.push(finding.clone());
    chromium_findings.push(finding);
}

pub fn scan_sessions(root: &Path) -> SessionScan {
    let sessions = root.join("sessions");
    let mut roots = Vec::new();
    let mut orphan_findings = Vec::new();
    let mut chromium_findings = Vec::new();
    let mut unknown = false;
    if is_symlink(&sessions) {
        mark_pair(
            &mut unknown,
            &mut orphan_findings,
            &mut chromium_findings,
            "session.root-symlink",
            "sessions root is a symlink and was not inspected",
            &sessions,
        );
    } else if sessions.exists() {
        if !is_directory(&sessions) {
            unknown = true;
            orphan_findings.push(DiagnosticFinding::warning(
                "session.scan-failed",
                "sessions path is not a directory",
                Some(&sessions),
            ));
            chromium_findings.push(DiagnosticFinding::warning(
                "chromium.scan-failed",
                "sessions path is not a directory",
                Some(&sessions),
            ));
        } else {
            match read_directory(&sessions) {
                Ok(Some(targets)) => {
                    for child in targets {
                        if is_symlink(&child) {
                            let (code, message) = if file_name_starts(&child, "s-") {
                                (
                                    "session.symlink",
                                    "session root is a symlink and was not inspected",
                                )
                            } else {
                                (
                                    "session.target-symlink",
                                    "session target is a symlink and was not inspected",
                                )
                            };
                            mark_pair(
                                &mut unknown,
                                &mut orphan_findings,
                                &mut chromium_findings,
                                code,
                                message,
                                &child,
                            );
                            continue;
                        }
                        if !is_directory(&child) {
                            continue;
                        }
                        if file_name_starts(&child, "s-") {
                            roots.push(child);
                        } else {
                            match read_directory(&child) {
                                Ok(Some(nested)) => {
                                    for path in nested {
                                        if !file_name_starts(&path, "s-") {
                                            continue;
                                        }
                                        if is_symlink(&path) {
                                            mark_pair(
                                                &mut unknown,
                                                &mut orphan_findings,
                                                &mut chromium_findings,
                                                "session.symlink",
                                                "session root is a symlink and was not inspected",
                                                &path,
                                            );
                                        } else if is_directory(&path) {
                                            roots.push(path);
                                        }
                                    }
                                }
                                Ok(None) => {
                                    unknown = true;
                                    orphan_findings.push(DiagnosticFinding::warning(
                                        "session.scan-failed",
                                        "target sessions disappeared during enumeration",
                                        Some(&child),
                                    ));
                                }
                                Err(error) => {
                                    unknown = true;
                                    orphan_findings.push(DiagnosticFinding::warning(
                                        "session.scan-failed",
                                        format!("cannot enumerate target sessions: {error}"),
                                        Some(&child),
                                    ));
                                }
                            }
                        }
                    }
                }
                Ok(None) => {
                    unknown = true;
                    orphan_findings.push(DiagnosticFinding::warning(
                        "session.scan-failed",
                        "sessions disappeared during enumeration",
                        Some(&sessions),
                    ));
                    chromium_findings.push(DiagnosticFinding::warning(
                        "chromium.scan-failed",
                        "sessions disappeared during enumeration",
                        Some(&sessions),
                    ));
                }
                Err(error) => {
                    unknown = true;
                    orphan_findings.push(DiagnosticFinding::warning(
                        "session.scan-failed",
                        format!("cannot enumerate sessions: {error}"),
                        Some(&sessions),
                    ));
                    chromium_findings.push(DiagnosticFinding::warning(
                        "chromium.scan-failed",
                        format!("cannot enumerate sessions: {error}"),
                        Some(&sessions),
                    ));
                }
            }
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
        if owner
            .get("handoffPending")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            mark_pair(
                &mut unknown,
                &mut orphan_findings,
                &mut chromium_findings,
                "session.handoff-pending",
                "session owner handoff is pending; session and Chromium residue were retained",
                &owner_path,
            );
            continue;
        }
        let orphan = match pid {
            Some(pid) if !pid_alive(pid) => true,
            Some(pid) => match owner
                .get("processStartIdentity")
                .or_else(|| owner.get("startedAt"))
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
            {
                None => {
                    unknown = true;
                    chromium_findings.push(DiagnosticFinding::warning(
                        "chromium.session-unknown",
                        "session owner has no process identity; Chromium residue cannot be classified",
                        Some(&owner_path),
                    ));
                    orphan_findings.push(DiagnosticFinding::warning(
                        "session.identity-missing",
                        "session owner has no process identity",
                        Some(&owner_path),
                    ));
                    false
                }
                Some(expected) => match live_process_identity(pid) {
                    Some(_) if !is_canonical_process_start_identity(expected) => {
                        unknown = true;
                        chromium_findings.push(DiagnosticFinding::warning(
                            "chromium.session-unknown",
                            "session process identity cannot be normalized; Chromium residue cannot be classified",
                            Some(&owner_path),
                        ));
                        orphan_findings.push(DiagnosticFinding::warning(
                            "session.identity-unknown",
                            format!("cannot normalize process identity for pid {pid}"),
                            Some(&owner_path),
                        ));
                        false
                    }
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
                if is_symlink(&chromium) {
                    mark_pair(
                        &mut unknown,
                        &mut orphan_findings,
                        &mut chromium_findings,
                        "chromium.session-symlink",
                        "orphan session Chromium data is a symlink and was not inspected",
                        &chromium,
                    );
                } else if is_directory(&chromium) {
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
        if is_symlink(&path) {
            unknown = true;
            chromium_findings.push(DiagnosticFinding::warning(
                "chromium.legacy-symlink",
                "legacy Chromium residue is a symlink and was not inspected",
                Some(&path),
            ));
        } else if is_directory(&path) {
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
        orphan_check: CheckResult::scanned(orphan_findings, unknown),
        chromium_check: CheckResult::scanned(chromium_findings, unknown),
    }
}
