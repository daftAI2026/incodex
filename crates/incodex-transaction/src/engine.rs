use std::fs;
use std::path::{Path, PathBuf};

use incodex_core::canonical::{inspect_target, recheck_target, CanonicalTarget};
use incodex_macos::ditto;

use crate::journal::{
    load_v2, reconstructed, tx_paths, validate_recovery_proofs, validate_rel_paths, write_journal,
    JournalTarget, JournalV2, RelPaths, ORIGINAL_REL, OUTGOING_REL, STAGED_REL,
};
use crate::lock::{acquire_target_lock, TargetLock};
use crate::new_install_id;
use crate::proof::{
    directory_identity, matches_recorded_restore, matches_recorded_restore_path,
    optional_directory_identity, parse_identity, record_restore_intent, require_identity,
    restore_source, tree_digest, validate_backup_digest, validate_pre_swap_live,
    validate_staged_snapshot, validate_tree_digest,
};
use crate::Recovery;

#[derive(Debug)]
pub enum TxError {
    Refuse { message: String },
    Other(String),
}

impl std::fmt::Display for TxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TxError::Refuse { message } => write!(f, "{message}"),
            TxError::Other(message) => write!(f, "{message}"),
        }
    }
}

#[derive(Debug)]
pub struct RecoverResult {
    pub action: Recovery,
    pub journal: JournalV2,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CommitResult {
    pub cleanup_warning: Option<String>,
}

pub struct Engine {
    root: PathBuf,
    target: CanonicalTarget,
    journal: JournalV2,
    _lock: TargetLock,
}

impl Engine {
    pub fn begin(root: &Path, live_path: &Path, command: &str) -> Result<Self, String> {
        let target = inspect_target(live_path, None)?;
        let install_id = new_install_id();
        let lock = acquire_target_lock(root, live_path, command, Some(&install_id))?;
        recheck_target(&target)?;
        let pre_swap_digest = tree_digest(&target.real_path)?;
        let journal = JournalV2 {
            schema_version: 2,
            install_id: install_id.clone(),
            target: JournalTarget {
                requested_path: target.requested_path.display().to_string(),
                real_path: target.real_path.display().to_string(),
                device: target.target_device.to_string(),
                inode: target.target_inode.to_string(),
                parent_device: target.parent_device.to_string(),
                parent_inode: target.parent_inode.to_string(),
            },
            paths: RelPaths {
                staged: STAGED_REL.into(),
                outgoing: OUTGOING_REL.into(),
                original: ORIGINAL_REL.into(),
            },
            phase: "DISCOVERED".into(),
            sequence: 1,
            checksum: String::new(),
            pre_swap_digest,
            backup_digest: String::new(),
            staged_device: String::new(),
            staged_inode: String::new(),
            staged_digest: String::new(),
            restored_device: String::new(),
            restored_inode: String::new(),
            restored_digest: String::new(),
        };
        write_journal(root, &journal)?;
        Ok(Self {
            root: root.to_path_buf(),
            target,
            journal: load_v2(root, &install_id)?,
            _lock: lock,
        })
    }

    pub fn install_id(&self) -> &str {
        &self.journal.install_id
    }

    pub fn journal(&self) -> &JournalV2 {
        &self.journal
    }

    pub fn staging_app(&self) -> PathBuf {
        tx_paths(&self.root, self.install_id()).staged
    }

    pub fn outgoing_app(&self) -> PathBuf {
        tx_paths(&self.root, self.install_id()).outgoing
    }

    pub fn place_staging(&mut self, staged: &Path) -> Result<(), String> {
        if self.journal.phase != "BACKUP_COMMITTED" {
            return Err(format!(
                "cannot stage from phase {}; backup must be BACKUP_COMMITTED",
                self.journal.phase
            ));
        }
        require_backup_snapshot(&self.root, self.install_id())?;
        validate_backup_digest(
            &tx_paths(&self.root, self.install_id()).original,
            &self.journal,
        )?;
        validate_pre_swap_live(&self.target, &self.journal)?;
        directory_identity(staged)
            .map_err(|error| format!("cannot use staging source: {error}"))?;
        let dest = self.staging_app();
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        if dest.exists() {
            fs::remove_dir_all(&dest).map_err(|err| err.to_string())?;
        }
        fs::rename(staged, &dest).or_else(|_| ditto(staged, &dest))?;
        let identity = directory_identity(&dest)?;
        let digest = tree_digest(&dest)?;
        self.journal.staged_device = identity.device.to_string();
        self.journal.staged_inode = identity.inode.to_string();
        self.journal.staged_digest = digest;
        self.advance("STAGED")
    }

    pub fn mark_backup_committed(&mut self) -> Result<(), String> {
        if self.journal.phase != "DISCOVERED" {
            return Err(format!(
                "cannot commit backup from phase {}",
                self.journal.phase
            ));
        }
        require_backup_snapshot(&self.root, self.install_id())?;
        let original = tx_paths(&self.root, self.install_id()).original;
        validate_pre_swap_live(&self.target, &self.journal)?;
        let backup_digest = tree_digest(&original)?;
        if backup_digest != self.journal.pre_swap_digest {
            return Err("backup snapshot does not match the sealed pre-swap tree".into());
        }
        self.journal.backup_digest = backup_digest;
        self.advance("BACKUP_COMMITTED")
    }

    pub fn swap(&mut self) -> Result<(), String> {
        self.swap_with_checkpoint(|_| {})
    }

    #[doc(hidden)]
    pub fn swap_with_checkpoint<F>(&mut self, mut checkpoint: F) -> Result<(), String>
    where
        F: FnMut(&str),
    {
        require_phase(&self.journal, "STAGED", "swap")?;
        validate_pre_swap_live(&self.target, &self.journal)?;
        validate_backup_digest(
            &tx_paths(&self.root, self.install_id()).original,
            &self.journal,
        )?;
        validate_staged_snapshot(&self.staging_app(), &self.journal)?;
        self.advance("TARGET_MOVED_OUT")?;
        checkpoint("TARGET_MOVED_OUT");
        let outgoing = self.outgoing_app();
        if let Some(parent) = outgoing.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        fs::rename(&self.target.real_path, &outgoing).map_err(|err| err.to_string())?;
        checkpoint("LIVE_MOVED_OUT");
        fs::rename(self.staging_app(), &self.target.real_path).map_err(|err| {
            let _ = fs::rename(&outgoing, &self.target.real_path);
            err.to_string()
        })?;
        checkpoint("STAGING_MOVED_IN");
        self.advance("SWAPPED")?;
        checkpoint("SWAPPED");
        Ok(())
    }

    pub fn commit(&mut self) -> Result<CommitResult, String> {
        self.commit_with_checkpoint(|_| {})
    }

    #[doc(hidden)]
    pub fn commit_with_checkpoint<F>(&mut self, mut checkpoint: F) -> Result<CommitResult, String>
    where
        F: FnMut(&str),
    {
        require_phase(&self.journal, "SWAPPED", "commit")?;
        let paths = reconstructed(&self.root, &self.journal)?;
        validate_recovery_target(&self.journal, &paths, &self.target.real_path)?;
        self.advance("COMMITTED")?;
        checkpoint("COMMITTED_BEFORE_CLEANUP");
        let cleanup_warning = cleanup_outgoing(&self.outgoing_app()).err();
        Ok(CommitResult { cleanup_warning })
    }

    pub fn rollback(&mut self, _reason: &str) -> Result<(), String> {
        match self.journal.phase.as_str() {
            "COMMITTED" => return Err("cannot rollback a committed transaction".into()),
            "ROLLED_BACK" => return Err("transaction is already rolled back".into()),
            phase if is_pre_swap_phase(phase) || is_post_swap_phase(phase) => {}
            phase => return Err(format!("cannot rollback transaction in phase {phase}")),
        }
        let paths = reconstructed(&self.root, &self.journal)?;
        validate_recovery_target(&self.journal, &paths, &self.target.real_path)?;
        if is_pre_swap_phase(&self.journal.phase) {
            cleanup_pre_swap(&self.root, &self.journal)?;
        } else {
            self.journal = restore_live(&self.root, &self.target.real_path, &self.journal)?;
        }
        self.advance("ROLLED_BACK")
    }

    fn advance(&mut self, phase: &str) -> Result<(), String> {
        self.journal.phase = phase.to_string();
        self.journal.sequence += 1;
        write_journal(&self.root, &self.journal)?;
        self.journal = load_v2(&self.root, self.install_id())?;
        Ok(())
    }
}

pub fn restore_committed(root: &Path, install_id: &str, live_path: &Path) -> Result<(), String> {
    let initial = load_v2(root, install_id)?;
    if initial.phase != "COMMITTED" {
        return Err(format!(
            "cannot restore an uncommitted transaction in phase {}",
            initial.phase
        ));
    }
    let lock_target = PathBuf::from(&initial.target.real_path);
    let _lock = acquire_target_lock(root, &lock_target, "uninstall", Some(install_id))?;
    let journal = load_v2(root, install_id)?;
    if journal.phase != "COMMITTED" {
        return Err(format!(
            "transaction changed to phase {} before committed restore",
            journal.phase
        ));
    }
    let current = inspect_target(live_path, None)?;
    if current.real_path != PathBuf::from(&journal.target.real_path) {
        return Err("committed target real path changed before restore".into());
    }
    let paths = reconstructed(root, &journal)?;
    validate_committed_restore_target(&journal, &paths, &current.real_path)?;
    restore_live(root, &current.real_path, &journal).map(|_| ())
}

fn require_backup_snapshot(root: &Path, install_id: &str) -> Result<(), String> {
    let original = tx_paths(root, install_id).original;
    directory_identity(&original).map_err(|error| {
        format!(
            "cannot use backup: original snapshot {} is unavailable: {error}",
            original.display()
        )
    })?;
    Ok(())
}

fn require_phase(journal: &JournalV2, expected: &str, operation: &str) -> Result<(), String> {
    if journal.phase == expected {
        Ok(())
    } else {
        Err(format!(
            "cannot {operation} transaction in phase {}; expected {expected}",
            journal.phase
        ))
    }
}

fn validate_recovery_target(
    journal: &JournalV2,
    paths: &crate::journal::TxPaths,
    live: &Path,
) -> Result<(), String> {
    validate_recovery_proofs(journal)?;
    let original_target = parse_identity(&journal.target.device, &journal.target.inode, "target")?;
    let parent = live
        .parent()
        .ok_or("target has no parent during recovery")?;
    let expected_parent = parse_identity(
        &journal.target.parent_device,
        &journal.target.parent_inode,
        "target parent",
    )?;
    require_identity(
        optional_directory_identity(parent, "target parent")?,
        expected_parent,
        "target parent",
    )?;

    let live_identity = optional_directory_identity(live, "live target")?;
    let staged_expected = if is_post_swap_phase(&journal.phase) {
        Some(parse_identity(
            &journal.staged_device,
            &journal.staged_inode,
            "staged target",
        )?)
    } else {
        None
    };
    let staged_identity = if is_post_swap_phase(&journal.phase) {
        optional_directory_identity(&paths.staged, "staged target")?
    } else {
        None
    };
    if let Some(identity) = staged_identity {
        let expected =
            staged_expected.ok_or("staged target identity is missing from the journal")?;
        require_identity(Some(identity), expected, "staged target")?;
        validate_tree_digest(&paths.staged, &journal.staged_digest, "staged target")?;
    }

    if matches!(journal.phase.as_str(), "BACKUP_COMMITTED" | "STAGED") {
        validate_backup_digest(&paths.original, journal)?;
    }

    match journal.phase.as_str() {
        phase if is_pre_swap_phase(phase) => {
            require_identity(live_identity, original_target, "live target")?;
            validate_tree_digest(live, &journal.pre_swap_digest, "pre-swap live")
        }
        "TARGET_MOVED_OUT" => {
            let outgoing = optional_directory_identity(&paths.outgoing, "outgoing target")?;
            if let Some(identity) = outgoing.as_ref() {
                require_identity(Some(*identity), original_target, "outgoing target")?;
                validate_tree_digest(&paths.outgoing, &journal.backup_digest, "outgoing target")?;
            }
            let staged =
                staged_expected.ok_or("staged target identity is missing from the journal")?;
            match live_identity {
                Some(identity) if identity == original_target => {
                    validate_tree_digest(live, &journal.pre_swap_digest, "pre-swap live")
                }
                Some(identity) if identity == staged => {
                    validate_tree_digest(live, &journal.staged_digest, "swapped live")?;
                    if outgoing.is_none() {
                        return Err("staged live has no outgoing original".into());
                    }
                    Ok(())
                }
                Some(_) if matches_recorded_restore(live, journal)? => Ok(()),
                None if outgoing.is_some() && staged_identity.is_some() => Ok(()),
                Some(_) => Err("live target is neither the original nor staged inode".into()),
                None => Err("live target and outgoing target are both missing".into()),
            }
        }
        "SWAPPED" | "TARGET_VERIFIED" => {
            let restore_root = paths
                .dir
                .parent()
                .and_then(Path::parent)
                .ok_or("transaction root is missing during recovery")?;
            let _ = restore_source(restore_root, journal)?;
            let staged =
                staged_expected.ok_or("staged target identity is missing from the journal")?;
            if matches!(live_identity, Some(identity) if identity == staged) {
                validate_tree_digest(live, &journal.staged_digest, "swapped live")?;
            } else if matches_recorded_restore(live, journal)? {
                // +-----------------------------------------------------------+
                // | restore rename 已完成；下一次 recover 只需重试垃圾清理。      |
                // +-----------------------------------------------------------+
            } else if live_identity.is_none()
                && matches_recorded_restore_path(
                    &paths.dir.join("restore/ChatGPT.app"),
                    journal,
                    "restore candidate",
                )?
            {
                // +-----------------------------------------------------------+
                // | durable intent 已落盘但 rename 尚未完成，允许安全重试。       |
                // +-----------------------------------------------------------+
            } else {
                return Err("swapped live target identity changed".into());
            }
            if let Some(identity) = optional_directory_identity(&paths.outgoing, "outgoing target")?
            {
                require_identity(Some(identity), original_target, "outgoing target")?;
            }
            Ok(())
        }
        phase => Err(format!("cannot validate recovery target in phase {phase}")),
    }
}

fn validate_committed_restore_target(
    journal: &JournalV2,
    paths: &crate::journal::TxPaths,
    live: &Path,
) -> Result<(), String> {
    validate_recovery_proofs(journal)?;
    let current = inspect_target(live, None)?;
    let expected_parent = parse_identity(
        &journal.target.parent_device,
        &journal.target.parent_inode,
        "target parent",
    )?;
    if current.parent_device != expected_parent.device
        || current.parent_inode != expected_parent.inode
    {
        return Err("committed target parent identity changed".into());
    }
    let expected_live = parse_identity(
        &journal.staged_device,
        &journal.staged_inode,
        "committed live target",
    )?;
    let actual_live = directory_identity(live)?;
    require_identity(Some(actual_live), expected_live, "committed live target")?;
    validate_tree_digest(live, &journal.staged_digest, "committed live target")?;
    validate_backup_digest(&paths.original, journal)
        .map_err(|error| format!("invalid committed backup: {error}"))
}

pub fn recover(root: &Path, install_id: &str) -> Result<RecoverResult, TxError> {
    recover_with(root, install_id, |_| true)
}

pub fn recover_with<F>(
    root: &Path,
    install_id: &str,
    verify_restored: F,
) -> Result<RecoverResult, TxError>
where
    F: FnOnce(&Path) -> bool,
{
    let mut journal = load_v2(root, install_id).map_err(|message| TxError::Refuse { message })?;
    validate_rel_paths(&journal).map_err(|message| TxError::Refuse { message })?;
    reconstructed(root, &journal).map_err(|message| TxError::Refuse { message })?;
    let live = PathBuf::from(&journal.target.real_path);
    let _lock = acquire_target_lock(root, &live, "recover", Some(install_id))
        .map_err(|message| TxError::Refuse { message })?;
    // +---------------------------------------------------------------+
    // | 路径检查必须在拿到 target lock 后再做一次，避免锁外检查的 TOCTOU。 |
    // +---------------------------------------------------------------+
    let paths = reconstructed(root, &journal).map_err(|message| TxError::Refuse { message })?;
    let action = match journal.phase.as_str() {
        "COMMITTED" | "ROLLED_BACK" => Recovery::Done,
        "DISCOVERED" | "INTENT" | "BACKUP_COMMITTED" | "STAGED" | "PATCHED" | "SIGNED"
        | "VERIFIED" | "TARGET_MOVED_OUT" | "SWAPPED" | "TARGET_VERIFIED" => Recovery::Rollback,
        _ => Recovery::Refuse,
    };
    if action == Recovery::Refuse {
        return Err(TxError::Refuse {
            message: format!(
                "cannot recover transaction {install_id} in phase {}",
                journal.phase
            ),
        });
    }
    if action == Recovery::Done {
        if journal.phase == "COMMITTED" {
            cleanup_committed(root, &journal).map_err(TxError::Other)?;
        }
        return Ok(RecoverResult { action, journal });
    }
    validate_recovery_target(&journal, &paths, &live)
        .map_err(|message| TxError::Refuse { message })?;
    let already_restored = is_post_swap_phase(&journal.phase)
        && matches_recorded_restore(&live, &journal)
            .map_err(|message| TxError::Refuse { message })?;
    if is_pre_swap_phase(&journal.phase) {
        cleanup_pre_swap(root, &journal).map_err(TxError::Other)?;
    } else if already_restored {
        cleanup_restored(root, &journal).map_err(TxError::Other)?;
    } else {
        journal = restore_live(root, &live, &journal).map_err(TxError::Other)?;
    }
    if !verify_restored(&live) {
        return Err(TxError::Other(
            "restored target failed codesign verification".into(),
        ));
    }
    let mut next = journal.clone();
    next.phase = "ROLLED_BACK".into();
    next.sequence += 1;
    write_journal(root, &next).map_err(TxError::Other)?;
    Ok(RecoverResult {
        action: Recovery::Rollback,
        journal: load_v2(root, install_id).map_err(TxError::Other)?,
    })
}

fn is_pre_swap_phase(phase: &str) -> bool {
    matches!(
        phase,
        "DISCOVERED" | "INTENT" | "BACKUP_COMMITTED" | "STAGED" | "PATCHED" | "SIGNED" | "VERIFIED"
    )
}

fn is_post_swap_phase(phase: &str) -> bool {
    matches!(phase, "TARGET_MOVED_OUT" | "SWAPPED" | "TARGET_VERIFIED")
}

fn cleanup_outgoing(outgoing: &Path) -> Result<(), String> {
    remove_path(outgoing)
}

fn cleanup_committed(root: &Path, journal: &JournalV2) -> Result<(), String> {
    let paths = tx_paths(root, &journal.install_id);
    cleanup_outgoing(&paths.outgoing)?;
    for path in [
        paths.staged,
        paths.dir.join("restore"),
        paths.dir.join("trash"),
    ] {
        remove_path(&path)?;
    }
    Ok(())
}

fn cleanup_pre_swap(root: &Path, journal: &JournalV2) -> Result<(), String> {
    let paths = tx_paths(root, &journal.install_id);
    for path in [
        paths.staged,
        paths.outgoing,
        paths.dir.join("restore"),
        paths.dir.join("trash"),
    ] {
        remove_path(&path)?;
    }
    if matches!(journal.phase.as_str(), "DISCOVERED" | "INTENT") {
        // +---------------------------------------------------------------+
        // | 备份尚未进入可用阶段；被中断的 ditto 产物只能当垃圾清掉，不能回写 live。 |
        // +---------------------------------------------------------------+
        remove_path(&paths.original)?;
    }
    Ok(())
}

fn restore_live(root: &Path, live: &Path, journal: &JournalV2) -> Result<JournalV2, String> {
    let paths = tx_paths(root, &journal.install_id);
    let source = restore_source(root, journal)?;
    let candidate = if source == paths.original {
        // +---------------------------------------------------------------+
        // | 原始快照是卸载所需的耐久备份；先复制再替换，回滚绝不消费它。          |
        // +---------------------------------------------------------------+
        let restore = paths.dir.join("restore").join("ChatGPT.app");
        remove_path(&restore)?;
        ditto(&source, &restore)?;
        restore
    } else {
        source
    };
    // +--------------------------------------------------------------------+
    // | 先把待迁入目录的 inode/digest 写入 journal；rename 后 inode 不变。   |
    // | 若进程死在两个动作之间，下一进程只重试可证明的 restore intent。    |
    // +--------------------------------------------------------------------+
    let journal = record_restore_intent(root, &candidate, journal)?;
    replace_live(&candidate, live, &paths.dir)?;
    cleanup_restored(root, &journal)?;
    Ok(journal)
}

fn cleanup_restored(root: &Path, journal: &JournalV2) -> Result<(), String> {
    let paths = tx_paths(root, &journal.install_id);
    for path in [
        paths.outgoing,
        paths.staged,
        paths.dir.join("restore"),
        paths.dir.join("trash"),
    ] {
        remove_path(&path)?;
    }
    Ok(())
}

fn replace_live(source: &Path, live: &Path, transaction_dir: &Path) -> Result<(), String> {
    let trash = transaction_dir.join("trash").join("ChatGPT.app");
    if let Some(parent) = trash.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    remove_path(&trash)?;
    let moved_live = if live.exists() {
        fs::rename(live, &trash).map_err(|err| err.to_string())?;
        true
    } else {
        false
    };
    if let Err(error) = fs::rename(source, live) {
        if moved_live {
            let _ = fs::rename(&trash, live);
        }
        return Err(error.to_string());
    }
    remove_path(&trash)?;
    Ok(())
}

fn remove_path(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_dir() {
        fs::remove_dir_all(path).map_err(|err| err.to_string())
    } else {
        // +--------------------------------------------------------------+
        // | symlink_metadata 不跟随 leaf；目录递归，其余类型只 unlink。   |
        // | 这样 socket/FIFO 等垃圾不会阻塞 committed/recovery 收敛。    |
        // +--------------------------------------------------------------+
        fs::remove_file(path).map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn tree_digest_streams_large_files_and_binds_permissions() {
        let root = std::env::temp_dir().join(format!(
            "incodex-tree-digest-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let large = root.join("large.bin");
        fs::write(&large, vec![0x5a; 4 * 1024 * 1024]).unwrap();

        let first = tree_digest(&root).unwrap();
        fs::set_permissions(&large, fs::Permissions::from_mode(0o600)).unwrap();
        let mode_changed = tree_digest(&root).unwrap();
        assert_ne!(first, mode_changed);
        fs::write(&large, vec![0xa5; 4 * 1024 * 1024]).unwrap();
        assert_ne!(mode_changed, tree_digest(&root).unwrap());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replace_live_checkpoints_after_durable_renames() {
        let root = std::env::temp_dir().join(format!(
            "incodex-replace-live-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let live = root.join("ChatGPT.app");
        let source = root.join("restore/ChatGPT.app");
        let transaction = root.join("transaction");
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("marker"), "patched").unwrap();
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("marker"), "original").unwrap();
        fs::create_dir_all(&transaction).unwrap();

        let mut checkpoints = Vec::new();
        replace_live_with_checkpoint(&source, &live, &transaction, |phase| {
            checkpoints.push(phase.to_string())
        })
        .unwrap();

        assert_eq!(
            checkpoints,
            [
                "LIVE_MOVED_TO_TRASH_DURABLE",
                "RESTORE_MOVED_TO_LIVE_DURABLE"
            ]
        );
        assert_eq!(fs::read_to_string(live.join("marker")).unwrap(), "original");
        fs::remove_dir_all(root).unwrap();
    }
}
