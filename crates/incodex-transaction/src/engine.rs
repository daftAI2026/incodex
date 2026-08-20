use std::fs;
use std::path::{Path, PathBuf};

use incodex_core::canonical::{inspect_target, recheck_target, CanonicalTarget};
use incodex_macos::ditto;

use crate::journal::{
    load_v2, reconstructed, tx_paths, validate_rel_paths, write_journal, JournalTarget, JournalV2,
    RelPaths, ORIGINAL_REL, OUTGOING_REL, STAGED_REL,
};
use crate::lock::{acquire_target_lock, TargetLock};
use crate::new_install_id;
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

pub struct Engine {
    root: PathBuf,
    live_path: PathBuf,
    target: CanonicalTarget,
    journal: JournalV2,
    _lock: TargetLock,
}

impl Engine {
    pub fn begin(root: &Path, live_path: &Path, command: &str) -> Result<Self, String> {
        let target = inspect_target(live_path, None)?;
        let install_id = new_install_id();
        let lock = acquire_target_lock(root, live_path, command, Some(&install_id))?;
        let journal = JournalV2 {
            schema_version: 2,
            install_id: install_id.clone(),
            target: JournalTarget {
                requested_path: target.requested_path.display().to_string(),
                real_path: target.real_path.display().to_string(),
                device: target.target_device.to_string(),
                inode: target.target_inode.to_string(),
            },
            paths: RelPaths {
                staged: STAGED_REL.into(),
                outgoing: OUTGOING_REL.into(),
                original: ORIGINAL_REL.into(),
            },
            phase: "DISCOVERED".into(),
            sequence: 1,
            checksum: String::new(),
        };
        write_journal(root, &journal)?;
        Ok(Self {
            root: root.to_path_buf(),
            live_path: live_path.to_path_buf(),
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
        let dest = self.staging_app();
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        if dest.exists() {
            fs::remove_dir_all(&dest).map_err(|err| err.to_string())?;
        }
        fs::rename(staged, &dest).or_else(|_| ditto(staged, &dest))?;
        self.advance("STAGED")
    }

    pub fn mark_backup_committed(&mut self) -> Result<(), String> {
        if self.journal.phase != "DISCOVERED" {
            return Err(format!(
                "cannot commit backup from phase {}",
                self.journal.phase
            ));
        }
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
        recheck_target(&self.target)?;
        self.advance("TARGET_MOVED_OUT")?;
        checkpoint("TARGET_MOVED_OUT");
        let outgoing = self.outgoing_app();
        if let Some(parent) = outgoing.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        fs::rename(&self.live_path, &outgoing).map_err(|err| err.to_string())?;
        checkpoint("LIVE_MOVED_OUT");
        fs::rename(self.staging_app(), &self.live_path).map_err(|err| {
            let _ = fs::rename(&outgoing, &self.live_path);
            err.to_string()
        })?;
        checkpoint("STAGING_MOVED_IN");
        self.advance("SWAPPED")?;
        checkpoint("SWAPPED");
        Ok(())
    }

    pub fn commit(&mut self) -> Result<(), String> {
        self.commit_with_checkpoint(|_| {})
    }

    #[doc(hidden)]
    pub fn commit_with_checkpoint<F>(&mut self, mut checkpoint: F) -> Result<(), String>
    where
        F: FnMut(&str),
    {
        self.advance("COMMITTED")?;
        checkpoint("COMMITTED_BEFORE_CLEANUP");
        cleanup_outgoing(&self.outgoing_app())
    }

    pub fn rollback(&mut self, _reason: &str) -> Result<(), String> {
        restore_live(&self.root, &self.live_path, &self.journal)?;
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
    let journal = load_v2(root, install_id).map_err(|message| TxError::Refuse { message })?;
    validate_rel_paths(&journal).map_err(|message| TxError::Refuse { message })?;
    reconstructed(root, &journal).map_err(|message| TxError::Refuse { message })?;
    let live = PathBuf::from(&journal.target.real_path);
    let _lock = acquire_target_lock(root, &live, "recover", Some(install_id))
        .map_err(|message| TxError::Refuse { message })?;
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
    if is_pre_swap_phase(&journal.phase) {
        cleanup_pre_swap(root, &journal).map_err(TxError::Other)?;
    } else {
        restore_live(root, &live, &journal).map_err(TxError::Other)?;
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

fn cleanup_outgoing(outgoing: &Path) -> Result<(), String> {
    if outgoing.exists() {
        fs::remove_dir_all(outgoing).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn cleanup_committed(root: &Path, journal: &JournalV2) -> Result<(), String> {
    let paths = tx_paths(root, &journal.install_id);
    cleanup_outgoing(&paths.outgoing)?;
    for path in [
        paths.staged,
        paths.dir.join("restore"),
        paths.dir.join("trash"),
    ] {
        if path.exists() {
            fs::remove_dir_all(path).map_err(|err| err.to_string())?;
        }
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
        if path.exists() {
            fs::remove_dir_all(path).map_err(|err| err.to_string())?;
        }
    }
    if matches!(journal.phase.as_str(), "DISCOVERED" | "INTENT") && paths.original.exists() {
        // +---------------------------------------------------------------+
        // | 备份尚未进入可用阶段；被中断的 ditto 产物只能当垃圾清掉，不能回写 live。 |
        // +---------------------------------------------------------------+
        fs::remove_dir_all(paths.original).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn restore_live(root: &Path, live: &Path, journal: &JournalV2) -> Result<(), String> {
    let paths = tx_paths(root, &journal.install_id);
    if paths.original.exists() {
        // +---------------------------------------------------------------+
        // | 原始快照是卸载所需的耐久备份；先复制再替换，回滚绝不消费它。          |
        // +---------------------------------------------------------------+
        let restore = paths.dir.join("restore").join("ChatGPT.app");
        if restore.exists() {
            fs::remove_dir_all(&restore).map_err(|err| err.to_string())?;
        }
        ditto(&paths.original, &restore)?;
        replace_live(&restore, live, &paths.dir)?;
    } else if paths.outgoing.exists() {
        replace_live(&paths.outgoing, live, &paths.dir)?;
    }
    if paths.outgoing.exists() {
        // +---------------------------------------------------------------+
        // | original 优先作为回滚源；outgoing 可能正处于被 commit 清理的半态。 |
        // +---------------------------------------------------------------+
        fs::remove_dir_all(&paths.outgoing).map_err(|err| err.to_string())?;
    }
    if paths.staged.exists() {
        fs::remove_dir_all(&paths.staged).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn replace_live(source: &Path, live: &Path, transaction_dir: &Path) -> Result<(), String> {
    let trash = transaction_dir.join("trash").join("ChatGPT.app");
    if let Some(parent) = trash.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    if trash.exists() {
        fs::remove_dir_all(&trash).map_err(|err| err.to_string())?;
    }
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
    if trash.exists() {
        fs::remove_dir_all(&trash).map_err(|err| err.to_string())?;
    }
    Ok(())
}
