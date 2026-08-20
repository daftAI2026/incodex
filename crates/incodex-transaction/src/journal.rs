use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::durable::write_atomic;

pub const STAGED_REL: &str = "staging/ChatGPT.app";
pub const OUTGOING_REL: &str = "outgoing/ChatGPT.app";
pub const ORIGINAL_REL: &str = "original/ChatGPT.app";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JournalTarget {
    pub requested_path: String,
    pub real_path: String,
    pub device: String,
    pub inode: String,
    #[serde(default)]
    pub parent_device: String,
    #[serde(default)]
    pub parent_inode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelPaths {
    pub staged: String,
    pub outgoing: String,
    pub original: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JournalV2 {
    pub schema_version: u32,
    pub install_id: String,
    pub target: JournalTarget,
    pub paths: RelPaths,
    pub phase: String,
    pub sequence: u64,
    pub checksum: String,
    #[serde(default)]
    pub pre_swap_digest: String,
    #[serde(default)]
    pub backup_digest: String,
    #[serde(default)]
    pub staged_device: String,
    #[serde(default)]
    pub staged_inode: String,
    #[serde(default)]
    pub staged_digest: String,
    #[serde(default)]
    pub restored_device: String,
    #[serde(default)]
    pub restored_inode: String,
    #[serde(default)]
    pub restored_digest: String,
}

#[derive(Debug, Clone)]
pub struct TxPaths {
    pub dir: PathBuf,
    pub journal: PathBuf,
    pub staged: PathBuf,
    pub outgoing: PathBuf,
    pub original: PathBuf,
}

pub fn tx_dir(root: &Path, install_id: &str) -> PathBuf {
    root.join("transactions").join(install_id)
}

pub fn tx_paths(root: &Path, install_id: &str) -> TxPaths {
    let dir = tx_dir(root, install_id);
    TxPaths {
        journal: dir.join("journal.json"),
        staged: dir.join(STAGED_REL),
        outgoing: dir.join(OUTGOING_REL),
        original: dir.join(ORIGINAL_REL),
        dir,
    }
}

pub fn is_uuid(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    let hex = |b: u8| b.is_ascii_hexdigit();
    let dash = |i: usize| bytes[i] == b'-';
    bytes[8] == b'-'
        && dash(13)
        && dash(18)
        && dash(23)
        && bytes[14].is_ascii_hexdigit()
        && (0..8).all(|i| hex(bytes[i]))
        && (9..13).all(|i| hex(bytes[i]))
        && (15..18).all(|i| hex(bytes[i]))
        && (19..23).all(|i| hex(bytes[i]))
        && (24..36).all(|i| hex(bytes[i]))
}

pub fn new_install_id() -> String {
    let mut bytes = [0u8; 16];
    if let Ok(mut file) = fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = file.read_exact(&mut bytes);
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

pub fn is_safe_relative(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.contains('\0') {
        return false;
    }
    let parsed = Path::new(path);
    if parsed.is_absolute() {
        return false;
    }
    parsed
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
}

pub fn checksum_of(journal: &JournalV2) -> String {
    let mut copy = journal.clone();
    copy.checksum.clear();
    let body = serde_json::to_vec(&copy).unwrap_or_default();
    checksum_hex(&body)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyJournalTarget<'a> {
    requested_path: &'a str,
    real_path: &'a str,
    device: &'a str,
    inode: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyJournal<'a> {
    schema_version: u32,
    install_id: &'a str,
    target: LegacyJournalTarget<'a>,
    paths: &'a RelPaths,
    phase: &'a str,
    sequence: u64,
    checksum: &'a str,
}

fn legacy_checksum_of(journal: &JournalV2) -> String {
    let legacy = LegacyJournal {
        schema_version: journal.schema_version,
        install_id: &journal.install_id,
        target: LegacyJournalTarget {
            requested_path: &journal.target.requested_path,
            real_path: &journal.target.real_path,
            device: &journal.target.device,
            inode: &journal.target.inode,
        },
        paths: &journal.paths,
        phase: &journal.phase,
        sequence: journal.sequence,
        checksum: "",
    };
    let body = serde_json::to_vec(&legacy).unwrap_or_default();
    checksum_hex(&body)
}

fn checksum_hex(body: &[u8]) -> String {
    Sha256::digest(body)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_legacy_shape(raw: &serde_json::Value) -> bool {
    let target_has_new_fields = raw
        .get("target")
        .and_then(serde_json::Value::as_object)
        .map(|target| target.contains_key("parentDevice") || target.contains_key("parentInode"))
        .unwrap_or(false);
    let proof_fields = [
        "preSwapDigest",
        "backupDigest",
        "stagedDevice",
        "stagedInode",
        "stagedDigest",
        "restoredDevice",
        "restoredInode",
        "restoredDigest",
    ];
    !target_has_new_fields
        && proof_fields.iter().all(|field| {
            !raw.as_object()
                .is_some_and(|object| object.contains_key(*field))
        })
}

pub fn seal(mut journal: JournalV2) -> JournalV2 {
    journal.checksum = checksum_of(&journal);
    journal
}

pub fn write_journal(root: &Path, journal: &JournalV2) -> Result<(), String> {
    if journal.install_id.is_empty() || !is_uuid(&journal.install_id) {
        return Err("install id must be an RFC 4122 UUID".into());
    }
    let path = tx_paths(root, &journal.install_id).journal;
    let sealed = seal(journal.clone());
    let body = format!(
        "{}\n",
        serde_json::to_string_pretty(&sealed).map_err(|err| err.to_string())?
    );
    write_atomic(&path, body.as_bytes())
}

pub fn load_v2(root: &Path, install_id: &str) -> Result<JournalV2, String> {
    if !is_uuid(install_id) {
        return Err("install id must be an RFC 4122 UUID".into());
    }
    let path = tx_paths(root, install_id).journal;
    let body = fs::read_to_string(&path).map_err(|err| err.to_string())?;
    let raw: serde_json::Value = serde_json::from_str(&body).map_err(|err| err.to_string())?;
    let legacy = is_legacy_shape(&raw);
    let journal: JournalV2 = serde_json::from_value(raw).map_err(|err| err.to_string())?;
    if journal.schema_version != 2 {
        return Err("unsupported journal schema".into());
    }
    if journal.install_id != install_id {
        return Err("journal install id does not match filename".into());
    }
    let expected_checksum = if legacy {
        legacy_checksum_of(&journal)
    } else {
        checksum_of(&journal)
    };
    if journal.checksum != expected_checksum {
        return Err("journal checksum mismatch".into());
    }
    validate_rel_paths(&journal)?;
    Ok(journal)
}

pub(crate) fn validate_recovery_proofs(journal: &JournalV2) -> Result<(), String> {
    if matches!(journal.phase.as_str(), "COMMITTED" | "ROLLED_BACK") {
        return Ok(());
    }
    if journal.target.parent_device.is_empty() || journal.target.parent_inode.is_empty() {
        return Err("journal lacks recovery proof: target parent identity".into());
    }
    if journal.pre_swap_digest.is_empty() {
        return Err("journal lacks recovery proof: pre-swap tree digest".into());
    }
    if !matches!(journal.phase.as_str(), "DISCOVERED" | "INTENT")
        && journal.backup_digest.is_empty()
    {
        return Err("journal lacks recovery proof: backup tree digest".into());
    }
    if matches!(
        journal.phase.as_str(),
        "STAGED"
            | "PATCHED"
            | "SIGNED"
            | "VERIFIED"
            | "TARGET_MOVED_OUT"
            | "SWAPPED"
            | "TARGET_VERIFIED"
    ) && (journal.staged_device.is_empty()
        || journal.staged_inode.is_empty()
        || journal.staged_digest.is_empty())
    {
        return Err("journal lacks recovery proof: staged tree identity and digest".into());
    }
    Ok(())
}

pub fn validate_rel_paths(journal: &JournalV2) -> Result<(), String> {
    if journal.paths.staged != STAGED_REL
        || journal.paths.outgoing != OUTGOING_REL
        || journal.paths.original != ORIGINAL_REL
    {
        return Err("journal paths do not match the transaction layout".into());
    }
    for path in [
        &journal.paths.staged,
        &journal.paths.outgoing,
        &journal.paths.original,
    ] {
        if !is_safe_relative(path) {
            return Err(format!("journal path is not a relative child: {path}"));
        }
    }
    Ok(())
}

pub fn reconstructed(root: &Path, journal: &JournalV2) -> Result<TxPaths, String> {
    validate_rel_paths(journal)?;
    let paths = tx_paths(root, &journal.install_id);
    reject_symlink(&root.join("transactions"), "transactions directory")?;
    reject_symlink(&paths.dir, "transaction directory")?;
    for (rel, allow_leaf_symlink) in [
        (journal.paths.staged.as_str(), true),
        (journal.paths.outgoing.as_str(), true),
        (journal.paths.original.as_str(), true),
        ("restore/ChatGPT.app", false),
        ("trash/ChatGPT.app", false),
    ] {
        validate_path_ancestors(&paths.dir, rel)?;
        let full = paths.dir.join(rel);
        if let Ok(meta) = fs::symlink_metadata(&full) {
            if meta.file_type().is_symlink() && !allow_leaf_symlink {
                return Err(format!("journal path is a symlink: {rel}"));
            }
            if meta.file_type().is_symlink() {
                // +-----------------------------------------------------------+
                // | 叶子 symlink 只允许进入受控清理；恢复源会在 restore 前拒绝。 |
                // +-----------------------------------------------------------+
                continue;
            }
        }
        if let Ok(real) = fs::canonicalize(&full) {
            let dir_real = fs::canonicalize(&paths.dir).unwrap_or_else(|_| paths.dir.clone());
            if !real.starts_with(&dir_real) {
                return Err(format!("journal path escaped transaction dir: {rel}"));
            }
        }
    }
    Ok(paths)
}

fn reject_symlink(path: &Path, label: &str) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            Err(format!("{label} is a symlink: {}", path.display()))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot inspect {label}: {error}")),
    }
}

fn validate_path_ancestors(base: &Path, rel: &str) -> Result<(), String> {
    let mut current = base.to_path_buf();
    let components: Vec<_> = Path::new(rel).components().collect();
    let component_count = components.len();
    for (index, component) in components.into_iter().enumerate() {
        if index + 1 == component_count {
            break;
        }
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(format!("journal path ancestor is a symlink: {rel}"));
            }
            Ok(meta) if !meta.file_type().is_dir() => {
                return Err(format!("journal path ancestor is not a directory: {rel}"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(format!("cannot inspect journal path: {error}")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal_with_paths(paths: RelPaths) -> JournalV2 {
        JournalV2 {
            schema_version: 2,
            install_id: "00000000-0000-4000-8000-000000000000".into(),
            target: JournalTarget {
                requested_path: "/Applications/ChatGPT.app".into(),
                real_path: "/Applications/ChatGPT.app".into(),
                device: "1".into(),
                inode: "2".into(),
                parent_device: "1".into(),
                parent_inode: "3".into(),
            },
            paths,
            phase: "DISCOVERED".into(),
            sequence: 1,
            checksum: String::new(),
            pre_swap_digest: String::new(),
            backup_digest: String::new(),
            staged_device: String::new(),
            staged_inode: String::new(),
            staged_digest: String::new(),
            restored_device: String::new(),
            restored_inode: String::new(),
            restored_digest: String::new(),
        }
    }

    #[test]
    fn relative_paths_are_frozen_to_the_transaction_layout() {
        let journal = journal_with_paths(RelPaths {
            staged: "staging/other.app".into(),
            outgoing: OUTGOING_REL.into(),
            original: ORIGINAL_REL.into(),
        });

        assert!(
            validate_rel_paths(&journal).is_err(),
            "a safe but non-canonical staged path was accepted"
        );
    }
}
