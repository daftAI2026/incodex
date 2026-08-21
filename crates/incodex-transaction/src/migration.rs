use std::fs;
use std::path::{Path, PathBuf};

use incodex_core::canonical::inspect_target;
use incodex_macos::ditto;
use sha2::{Digest, Sha256};

use crate::journal::{
    load_v2, tx_paths, write_journal, JournalTarget, JournalV2, RelPaths, ORIGINAL_REL,
    OUTGOING_REL, STAGED_REL,
};
use crate::lock::TargetLock;
use crate::proof::{directory_identity, tree_digest};

/// Inputs sealed by the read-only legacy proof gate.
#[derive(Debug, Clone)]
pub struct LegacyMigrationInput {
    pub install_id: String,
    pub requested_path: PathBuf,
    pub real_path: PathBuf,
    pub target_device: u64,
    pub target_inode: u64,
    pub parent_device: u64,
    pub parent_inode: u64,
    pub original_source: PathBuf,
    pub live_asar_file_hash: String,
    pub original_asar_file_hash: String,
    pub original_plist_file_hash: String,
}

/// Adopt a verified TS v1 backup as a committed Rust v2 transaction.
///
/// The lock parameter is deliberately required: callers cannot turn a proof
/// result into a v2 mutation record after releasing the target lock.
pub fn adopt_legacy_committed_locked(
    root: &Path,
    _lock: &TargetLock,
    input: &LegacyMigrationInput,
) -> Result<JournalV2, String> {
    let current = inspect_target(&input.real_path, None)?;
    if current.real_path != input.real_path
        || current.target_device != input.target_device
        || current.target_inode != input.target_inode
        || current.parent_device != input.parent_device
        || current.parent_inode != input.parent_inode
    {
        return Err("legacy migration target identity changed before v2 adoption".into());
    }
    let paths = tx_paths(root, &input.install_id);
    if let Ok(existing) = load_v2(root, &input.install_id) {
        if existing.phase != "COMMITTED"
            || existing.target.real_path != input.real_path.display().to_string()
        {
            return Err(
                "legacy migration install id already belongs to another transaction".into(),
            );
        }
        return Ok(existing);
    }
    if !input.original_source.is_dir() {
        return Err(format!(
            "legacy original backup is missing: {}",
            input.original_source.display()
        ));
    }
    verify_file_hash(
        &input.real_path.join("Contents/Resources/app.asar"),
        &input.live_asar_file_hash,
        "legacy live ASAR",
    )?;
    verify_file_hash(
        &input.original_source.join("Contents/Resources/app.asar"),
        &input.original_asar_file_hash,
        "legacy original ASAR",
    )?;
    verify_file_hash(
        &input.original_source.join("Contents/Info.plist"),
        &input.original_plist_file_hash,
        "legacy original Info.plist",
    )?;
    if paths.original.exists() {
        return Err("legacy migration destination already contains an unrecognized backup".into());
    }
    fs::create_dir_all(paths.original.parent().unwrap()).map_err(|error| error.to_string())?;
    ditto(&input.original_source, &paths.original)?;
    verify_file_hash(
        &paths.original.join("Contents/Resources/app.asar"),
        &input.original_asar_file_hash,
        "migrated original ASAR",
    )?;
    verify_file_hash(
        &paths.original.join("Contents/Info.plist"),
        &input.original_plist_file_hash,
        "migrated original Info.plist",
    )?;
    let backup_digest = tree_digest(&paths.original)?;
    let live_identity = directory_identity(&input.real_path)?;
    let live_digest = tree_digest(&input.real_path)?;
    if live_identity.device != input.target_device || live_identity.inode != input.target_inode {
        return Err("legacy migration live identity changed during v2 adoption".into());
    }
    verify_file_hash(
        &input.real_path.join("Contents/Resources/app.asar"),
        &input.live_asar_file_hash,
        "legacy live ASAR",
    )?;
    let journal = JournalV2 {
        schema_version: 2,
        install_id: input.install_id.clone(),
        target: JournalTarget {
            requested_path: input.requested_path.display().to_string(),
            real_path: input.real_path.display().to_string(),
            device: input.target_device.to_string(),
            inode: input.target_inode.to_string(),
            parent_device: input.parent_device.to_string(),
            parent_inode: input.parent_inode.to_string(),
        },
        paths: RelPaths {
            staged: STAGED_REL.into(),
            outgoing: OUTGOING_REL.into(),
            original: ORIGINAL_REL.into(),
        },
        phase: "COMMITTED".into(),
        sequence: 1,
        checksum: String::new(),
        pre_swap_digest: backup_digest.clone(),
        backup_digest,
        staged_device: live_identity.device.to_string(),
        staged_inode: live_identity.inode.to_string(),
        staged_digest: live_digest,
        restored_device: String::new(),
        restored_inode: String::new(),
        restored_digest: String::new(),
    };
    write_journal(root, &journal)?;
    load_v2(root, &input.install_id)
}

fn verify_file_hash(path: &Path, expected: &str, label: &str) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|error| format!("cannot read {label}: {error}"))?;
    let actual = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual != expected {
        return Err(format!("{label} hash changed during migration"));
    }
    Ok(())
}
