mod durable;
mod engine;
mod journal;
mod lock;
mod proof;

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub use engine::{recover, recover_with, CommitResult, Engine, RecoverResult, TxError};
pub use journal::{new_install_id, JournalV2};

pub fn journal_v2(root: &Path, install_id: &str) -> Result<JournalV2, String> {
    journal::load_v2(root, install_id)
}
pub use lock::{acquire_target_lock, lock_path_for};

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
    "COMMITTED",
    "ROLLED_BACK",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Journal {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(rename = "installId")]
    pub install_id: String,
    #[serde(rename = "targetRealPath")]
    pub target_real_path: String,
    #[serde(rename = "stagedApp")]
    pub staged_app: String,
    #[serde(rename = "originalSnapshot")]
    pub original_snapshot: String,
    #[serde(rename = "outgoingApp", skip_serializing_if = "Option::is_none")]
    pub outgoing_app: Option<String>,
    pub phase: String,
    #[serde(rename = "updatedAt")]
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
    let obj = raw.as_object()?;
    if obj.get("schemaVersion")?.as_u64()? != 1 {
        return None;
    }
    let install_id = obj.get("installId")?.as_str()?.to_string();
    if install_id.is_empty() {
        return None;
    }
    let target_real_path = obj.get("targetRealPath")?.as_str()?.to_string();
    let staged_app = obj.get("stagedApp")?.as_str()?.to_string();
    let original_snapshot = obj.get("originalSnapshot")?.as_str()?.to_string();
    if target_real_path.is_empty() || staged_app.is_empty() || original_snapshot.is_empty() {
        return None;
    }
    let outgoing_app = match obj.get("outgoingApp") {
        None => None,
        Some(value) => {
            let text = value.as_str()?.to_string();
            if text.is_empty() {
                return None;
            }
            Some(text)
        }
    };
    let phase = obj.get("phase")?.as_str()?.to_string();
    if !PHASES.contains(&phase.as_str()) {
        return None;
    }
    let updated_at = obj.get("updatedAt")?.as_str()?.to_string();
    Some(Journal {
        schema_version: 1,
        install_id,
        target_real_path,
        staged_app,
        original_snapshot,
        outgoing_app,
        phase,
        updated_at,
    })
}

pub fn recover_action(journal: &Journal) -> Recovery {
    recover_action_phase(&journal.phase)
}

pub fn recover_action_phase(phase: &str) -> Recovery {
    match phase {
        "COMMITTED" | "ROLLED_BACK" => Recovery::Done,
        "DISCOVERED" | "INTENT" | "BACKUP_COMMITTED" | "STAGED" | "PATCHED" | "SIGNED"
        | "VERIFIED" | "TARGET_MOVED_OUT" | "SWAPPED" | "TARGET_VERIFIED" => Recovery::Rollback,
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
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
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
