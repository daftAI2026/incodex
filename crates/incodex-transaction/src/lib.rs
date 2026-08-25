mod durable;
mod engine;
mod journal;
mod lock;
mod proof;
mod uninstall;

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub use engine::{
    recover, recover_with, recover_with_quiescence, validate_post_swap_rollback, CommitResult,
    Engine, RecoverResult, TxError,
};
pub use journal::{new_install_id, validate_path_ancestors, JournalV2};
pub use uninstall::{
    migrate_legacy_committed, migrate_legacy_committed_with_quiescence, restore_committed,
    restore_committed_with_checkpoint, restore_committed_with_quiescence,
};

pub fn journal_v2(root: &Path, install_id: &str) -> Result<JournalV2, String> {
    journal::load_v2(root, install_id)
}

/// Validate the sealed original snapshot for a diagnostic consumer.
pub fn validate_backup_snapshot(root: &Path, install_id: &str) -> Result<(), String> {
    let journal = journal::load_v2(root, install_id)?;
    let paths = journal::reconstructed(root, &journal)?;
    proof::validate_backup_digest(&paths.original, &journal)
}

/// Validate that a committed live target still matches its sealed transaction.
///
/// Install uses this read-only proof before deciding that a marker is safe to
/// reuse.  A marker alone is not an original snapshot authority.
pub fn validate_committed_live_snapshot(
    root: &Path,
    install_id: &str,
    live_path: &Path,
) -> Result<(), String> {
    let journal = journal::load_v2(root, install_id)?;
    uninstall::validate_committed_restore_target(root, &journal, live_path)
}

/// Validate that a committed transaction's cleanup paths remain recoverable.
pub fn validate_committed_cleanup(root: &Path, install_id: &str) -> Result<(), String> {
    let journal = journal::load_v2(root, install_id)?;
    if journal.phase != "COMMITTED" {
        return Err(format!(
            "transaction {install_id} is not committed: {}",
            journal.phase
        ));
    }
    journal::reconstructed(root, &journal).map(|_| ())
}
pub use lock::{acquire_target_lock, lock_path_for, TargetLock};

/// 事务只依赖这一条窄 seam；具体的 macOS 进程策略由 CLI 注入。
///
/// Guard 不保存“已 quiescent”的事实。每次调用都必须重新观察目标进程。
pub trait QuiescenceGuard: 'static {
    fn ensure_quiescent(&self, target: &Path) -> Result<(), String>;
}

impl<F> QuiescenceGuard for F
where
    F: Fn(&Path) -> Result<(), String> + 'static,
{
    fn ensure_quiescent(&self, target: &Path) -> Result<(), String> {
        self(target)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopQuiescenceGuard;

impl QuiescenceGuard for NoopQuiescenceGuard {
    fn ensure_quiescent(&self, _target: &Path) -> Result<(), String> {
        Ok(())
    }
}

pub const PHASES: &[&str] = &[
    "DISCOVERED",
    "BACKUP_COMMITTED",
    "STAGED",
    "PATCHED",
    "SIGNED",
    "VERIFIED",
    "TARGET_MOVED_OUT",
    "SWAPPED",
    "TARGET_VERIFIED",
    "UNINSTALLING",
    "COMMITTED",
    "ROLLED_BACK",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Journal {
    pub schema_version: u32,
    pub install_id: String,
    pub target_real_path: String,
    pub staged_app: String,
    pub original_snapshot: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outgoing_app: Option<String>,
    pub phase: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    Rollback,
    Refuse,
    Done,
}

impl Recovery {
    pub fn as_str(self) -> &'static str {
        match self {
            Recovery::Rollback => "rollback",
            Recovery::Refuse => "refuse",
            Recovery::Done => "done",
        }
    }
}

pub fn parse_journal(raw: &serde_json::Value) -> Option<Journal> {
    let journal: Journal = serde_json::from_value(raw.clone()).ok()?;
    let required_paths_are_present = !journal.install_id.is_empty()
        && !journal.target_real_path.is_empty()
        && !journal.staged_app.is_empty()
        && !journal.original_snapshot.is_empty();
    let outgoing_is_valid = !raw
        .get("outgoingApp")
        .is_some_and(serde_json::Value::is_null)
        && journal
            .outgoing_app
            .as_deref()
            .is_none_or(|path| !path.is_empty());
    (journal.schema_version == 1
        && required_paths_are_present
        && outgoing_is_valid
        && PHASES.contains(&journal.phase.as_str()))
    .then_some(journal)
}

pub fn recover_action(journal: &Journal) -> Recovery {
    recover_action_phase(&journal.phase)
}

pub fn recover_action_phase(phase: &str) -> Recovery {
    match phase {
        "COMMITTED" | "ROLLED_BACK" => Recovery::Done,
        "DISCOVERED" | "INTENT" | "BACKUP_COMMITTED" | "STAGED" | "PATCHED" | "SIGNED"
        | "VERIFIED" | "TARGET_MOVED_OUT" | "SWAPPED" | "TARGET_VERIFIED" | "UNINSTALLING" => {
            Recovery::Rollback
        }
        _ => Recovery::Refuse,
    }
}

pub fn load_journal(install_id: &str, root: &Path) -> Option<Journal> {
    let path = root.join("transactions").join(format!("{install_id}.json"));
    let body = fs::read_to_string(path).ok()?;
    let raw: serde_json::Value = serde_json::from_str(&body).ok()?;
    parse_journal(&raw)
}

pub fn list_journals(root: &Path) -> Vec<Journal> {
    let dir = root.join("transactions");
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.ends_with(".json") || name.ends_with(".tmp") {
            continue;
        }
        let id = name.trim_end_matches(".json");
        if let Some(journal) = load_journal(id, root) {
            out.push(journal);
        }
    }
    out
}

pub fn list_interrupted(root: &Path) -> Vec<(String, String, Recovery)> {
    let mut out = Vec::new();
    for journal in list_journals(root) {
        let action = recover_action(&journal);
        if action != Recovery::Done {
            out.push((journal.install_id, journal.phase, action));
        }
    }
    let dir = root.join("transactions");
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        if let Ok(journal) = journal::load_v2(root, &id) {
            let action = recover_action_phase(&journal.phase);
            if action != Recovery::Done {
                out.push((journal.install_id, journal.phase, action));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patched_rolls_back() {
        let journal = parse_journal(&serde_json::json!({
            "schemaVersion": 1,
            "installId": "tx-1",
            "targetRealPath": "/tmp/x.app",
            "stagedApp": "/tmp/staged",
            "originalSnapshot": "/tmp/original",
            "phase": "PATCHED",
            "updatedAt": "2026-01-01T00:00:00.000Z"
        }))
        .unwrap();
        assert_eq!(recover_action(&journal), Recovery::Rollback);
        assert_eq!(recover_action(&journal).as_str(), "rollback");
    }
}
