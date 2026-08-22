use std::fs;
use std::path::{Path, PathBuf};

use incodex_core::canonical::{inspect_target, recheck_target};
use incodex_macos::ditto;

use crate::durable::sync_dir;
use crate::journal::{
    load_v2, reconstructed, tx_paths, validate_recovery_proofs, write_journal, JournalV2,
};
use crate::lock::acquire_target_lock;
use crate::proof::{
    directory_identity, parse_identity, record_restore_intent, require_identity, restore_source,
    tree_digest, validate_tree_digest,
};
use crate::{NoopQuiescenceGuard, QuiescenceGuard};

pub fn restore_committed(root: &Path, install_id: &str, live_path: &Path) -> Result<(), String> {
    restore_committed_with_quiescence(root, install_id, live_path, NoopQuiescenceGuard, |_| {})
}

#[doc(hidden)]
pub fn restore_committed_with_checkpoint<F>(
    root: &Path,
    install_id: &str,
    live_path: &Path,
    checkpoint: F,
) -> Result<(), String>
where
    F: FnMut(&str),
{
    restore_committed_with_quiescence(root, install_id, live_path, NoopQuiescenceGuard, checkpoint)
}

pub fn restore_committed_with_quiescence<G, F>(
    root: &Path,
    install_id: &str,
    live_path: &Path,
    quiescence: G,
    mut checkpoint: F,
) -> Result<(), String>
where
    G: QuiescenceGuard,
    F: FnMut(&str),
{
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
    restore_committed_locked(root, &journal, live_path, &quiescence, &mut checkpoint).map(|_| ())
}

pub fn migrate_legacy_committed<F, G>(
    root: &Path,
    install_id: &str,
    live_path: &Path,
    verify_backup: F,
    verify_live: G,
) -> Result<(), String>
where
    F: FnOnce(&Path) -> bool,
    G: FnOnce(&Path) -> bool,
{
    migrate_legacy_committed_with_quiescence(
        root,
        install_id,
        live_path,
        NoopQuiescenceGuard,
        verify_backup,
        verify_live,
    )
}

pub fn migrate_legacy_committed_with_quiescence<Q, F, G>(
    root: &Path,
    install_id: &str,
    live_path: &Path,
    quiescence: Q,
    verify_backup: F,
    verify_live: G,
) -> Result<(), String>
where
    Q: QuiescenceGuard,
    F: FnOnce(&Path) -> bool,
    G: FnOnce(&Path) -> bool,
{
    let initial = load_v2(root, install_id)?;
    if !is_legacy_committed(&initial) {
        return Err("transaction is not a legacy COMMITTED journal".into());
    }
    let lock_target = PathBuf::from(&initial.target.real_path);
    let _lock = acquire_target_lock(root, &lock_target, "uninstall", Some(install_id))?;
    let journal = load_v2(root, install_id)?;
    if !is_legacy_committed(&journal) {
        return Err("legacy transaction changed before migration".into());
    }
    quiescence.ensure_quiescent(live_path)?;
    let current = inspect_target(live_path, None)?;
    if current.real_path != Path::new(&journal.target.real_path) {
        return Err("legacy committed target real path changed before migration".into());
    }
    let paths = reconstructed(root, &journal)?;
    directory_identity(&paths.original)
        .map_err(|error| format!("invalid legacy backup: {error}"))?;
    if !verify_backup(&paths.original) {
        return Err("legacy backup failed codesign verification".into());
    }
    if !verify_live(&current.real_path) {
        return Err("legacy live target failed binding verification".into());
    }
    recheck_target(&current)?;
    let backup_digest = tree_digest(&paths.original)?;
    let staged_identity = directory_identity(&current.real_path)
        .map_err(|error| format!("invalid legacy live target: {error}"))?;
    let staged_digest = tree_digest(&current.real_path)?;
    let mut migrated = journal.clone();
    migrated.target.parent_device = current.parent_device.to_string();
    migrated.target.parent_inode = current.parent_inode.to_string();
    migrated.pre_swap_digest = backup_digest.clone();
    migrated.backup_digest = backup_digest;
    migrated.staged_device = staged_identity.device.to_string();
    migrated.staged_inode = staged_identity.inode.to_string();
    migrated.staged_digest = staged_digest;
    migrated.sequence += 1;
    write_journal(root, &migrated)?;
    let migrated = load_v2(root, install_id)?;
    restore_committed_locked(root, &migrated, live_path, &quiescence, &mut |_| {}).map(|_| ())
}

fn is_legacy_committed(journal: &JournalV2) -> bool {
    journal.phase == "COMMITTED"
        && journal.target.parent_device.is_empty()
        && journal.target.parent_inode.is_empty()
        && journal.pre_swap_digest.is_empty()
        && journal.backup_digest.is_empty()
        && journal.staged_device.is_empty()
        && journal.staged_inode.is_empty()
        && journal.staged_digest.is_empty()
        && journal.restored_device.is_empty()
        && journal.restored_inode.is_empty()
        && journal.restored_digest.is_empty()
}

fn restore_committed_locked<F>(
    root: &Path,
    journal: &JournalV2,
    live_path: &Path,
    quiescence: &dyn QuiescenceGuard,
    checkpoint: &mut F,
) -> Result<JournalV2, String>
where
    F: FnMut(&str),
{
    // 不要先把 COMMITTED journal 推进成 UNINSTALLING；拒绝时 journal 必须保持真相。
    quiescence.ensure_quiescent(live_path)?;
    validate_committed_restore_target(root, journal, live_path)?;
    let uninstalling = begin_uninstall(root, journal)?;
    let restored =
        restore_live_with_quiescence(root, live_path, &uninstalling, quiescence, checkpoint)?;
    let mut done = restored;
    done.phase = "ROLLED_BACK".into();
    done.sequence += 1;
    write_journal(root, &done)?;
    load_v2(root, &done.install_id)
}

fn begin_uninstall(root: &Path, journal: &JournalV2) -> Result<JournalV2, String> {
    if journal.phase != "COMMITTED" {
        return Err(format!(
            "cannot begin uninstall from phase {}",
            journal.phase
        ));
    }
    let mut next = journal.clone();
    next.phase = "UNINSTALLING".into();
    next.sequence += 1;
    write_journal(root, &next)?;
    load_v2(root, &journal.install_id)
}

pub(crate) fn validate_committed_restore_target(
    root: &Path,
    journal: &JournalV2,
    live: &Path,
) -> Result<(), String> {
    if journal.phase != "COMMITTED" {
        return Err(format!(
            "cannot restore an uncommitted transaction in phase {}",
            journal.phase
        ));
    }
    reconstructed(root, journal)?;
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
    validate_tree_digest(live, &journal.staged_digest, "committed live target")
}

pub(crate) fn restore_live_with_quiescence<F>(
    root: &Path,
    live: &Path,
    journal: &JournalV2,
    quiescence: &dyn QuiescenceGuard,
    checkpoint: &mut F,
) -> Result<JournalV2, String>
where
    F: FnMut(&str),
{
    // restore 之前重新观察；调用方不能把之前的 quiescent 结果当永久事实。
    quiescence.ensure_quiescent(live)?;
    let paths = tx_paths(root, &journal.install_id);
    let source = restore_source(root, journal)?;
    let candidate = if source == paths.original {
        // +---------------------------------------------------------------+
        // | 原始快照是卸载所需的耐久备份；先复制再替换，回滚绝不消费它。       |
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
    replace_live_with_checkpoint(&candidate, live, &paths.dir, checkpoint)?;
    cleanup_restored(root, &journal)?;
    Ok(journal)
}

pub(crate) fn cleanup_restored(root: &Path, journal: &JournalV2) -> Result<(), String> {
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

pub(crate) fn replace_live_with_checkpoint<F>(
    source: &Path,
    live: &Path,
    transaction_dir: &Path,
    mut checkpoint: F,
) -> Result<(), String>
where
    F: FnMut(&str),
{
    let trash = transaction_dir.join("trash").join("ChatGPT.app");
    if let Some(parent) = trash.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    remove_path(&trash)?;
    let moved_live = if live.exists() {
        fs::rename(live, &trash).map_err(|err| err.to_string())?;
        sync_rename_parents(live, &trash)?;
        checkpoint("LIVE_MOVED_TO_TRASH_DURABLE");
        true
    } else {
        false
    };
    if let Err(error) = fs::rename(source, live) {
        if moved_live {
            let _ = fs::rename(&trash, live);
            let _ = sync_rename_parents(&trash, live);
        }
        return Err(error.to_string());
    }
    sync_rename_parents(source, live)?;
    checkpoint("RESTORE_MOVED_TO_LIVE_DURABLE");
    remove_path(&trash)?;
    sync_parent(&trash)?;
    Ok(())
}

fn sync_parent(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("path has no parent to sync: {}", path.display()))?;
    sync_dir(parent)
}

pub(crate) fn sync_rename_parents(first: &Path, second: &Path) -> Result<(), String> {
    sync_parent(first)?;
    if first.parent() != second.parent() {
        sync_parent(second)?;
    }
    Ok(())
}

pub(crate) fn remove_path(path: &Path) -> Result<(), String> {
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
