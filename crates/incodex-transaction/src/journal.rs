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
    let digest = Sha256::digest(&body);
    digest.iter().map(|b| format!("{b:02x}")).collect()
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
    let body = format!("{}\n", serde_json::to_string_pretty(&sealed).map_err(|err| err.to_string())?);
    write_atomic(&path, body.as_bytes())
}

pub fn load_v2(root: &Path, install_id: &str) -> Result<JournalV2, String> {
    if !is_uuid(install_id) {
        return Err("install id must be an RFC 4122 UUID".into());
    }
    let path = tx_paths(root, install_id).journal;
    let body = fs::read_to_string(&path).map_err(|err| err.to_string())?;
    let journal: JournalV2 = serde_json::from_str(&body).map_err(|err| err.to_string())?;
    if journal.schema_version != 2 {
        return Err("unsupported journal schema".into());
    }
    if journal.install_id != install_id {
        return Err("journal install id does not match filename".into());
    }
    if journal.checksum != checksum_of(&journal) {
        return Err("journal checksum mismatch".into());
    }
    validate_rel_paths(&journal)?;
    Ok(journal)
}

pub fn validate_rel_paths(journal: &JournalV2) -> Result<(), String> {
    for path in [&journal.paths.staged, &journal.paths.outgoing, &journal.paths.original] {
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
    for rel in [&journal.paths.staged, &journal.paths.outgoing, &journal.paths.original] {
        validate_path_ancestors(&paths.dir, rel)?;
        let full = paths.dir.join(rel);
        if let Ok(meta) = fs::symlink_metadata(&full) {
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
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(format!("cannot inspect journal path: {error}")),
        }
    }
    Ok(())
}
