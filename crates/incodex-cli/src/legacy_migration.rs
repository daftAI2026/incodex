use std::fs;
use std::path::{Path, PathBuf};

use incodex_asar::Archive;
use incodex_core::canonical::canonical_path;
use incodex_macos::{ditto, verify_app};
use incodex_transaction::{acquire_target_lock, journal_v2, tree_digest, JournalV2};

use crate::legacy_proof::LegacyProvenState;
use crate::legacy_typescript::{
    load_legacy_journal, load_legacy_ts_v1, write_legacy_journal, LegacyState, TransactionJournal,
};

/// Migrate one already-proven TS v1 state without invoking the retired CLI.
pub fn migrate_legacy_ts_v1(root: &Path, proven: LegacyProvenState) -> Result<JournalV2, String> {
    proven.migrate(root)
}

/// Find and migrate a committed TS v1 installation for a target.
pub fn migrate_legacy_if_needed(root: &Path, target: &Path) -> Result<Option<JournalV2>, String> {
    let Some(state) = load_legacy_ts_v1(root, target)? else {
        return Ok(None);
    };
    match state.state {
        LegacyState::Committed { .. } => {
            // Native uninstall preserves the legacy record for rollback/audit, but
            // leaves a rolled-back v2 journal.  Do not re-prove the now-clean app
            // as a legacy patched target on a later invocation.
            if let Ok(native) = journal_v2(root, &state.install_id) {
                if native.phase == "ROLLED_BACK" {
                    return Ok(None);
                }
            }
            let proven = crate::legacy_proof::prove_legacy_ts_v1(root, state)?;
            migrate_legacy_ts_v1(root, proven).map(Some)
        }
        LegacyState::Interrupted => Err(format!(
            "legacy transaction {} is interrupted; run `incodex recover --transaction {}` first",
            state.install_id, state.install_id
        )),
        LegacyState::RolledBack => Ok(None),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyRecoveryResult {
    pub install_id: String,
    pub phase: String,
    pub action: &'static str,
    pub target: PathBuf,
    pub backup_intact: bool,
    pub staged_removed: bool,
    pub outgoing_restored: bool,
}

/// Recover one flat TS v1 journal using its exact emitted paths.
pub fn recover_legacy_ts_v1(root: &Path, install_id: &str) -> Result<LegacyRecoveryResult, String> {
    let initial = load_legacy_journal(root, install_id)?;
    let target = PathBuf::from(&initial.target_real_path);
    let _lock = acquire_target_lock(root, &target, "recover", Some(install_id))?;
    let journal = load_legacy_journal(root, install_id)?;
    if journal.phase == "COMMITTED" || journal.phase == "ROLLED_BACK" {
        return Ok(done_result(&journal));
    }

    let original = PathBuf::from(&journal.original_snapshot);
    let staged = PathBuf::from(&journal.staged_app);
    let outgoing = journal.outgoing_app.as_deref().map(PathBuf::from);
    let needs_original = matches!(
        journal.phase.as_str(),
        "TARGET_MOVED_OUT" | "SWAPPED" | "TARGET_VERIFIED"
    );
    let mut outgoing_restored = false;
    if needs_original && original.is_dir() {
        if !already_restored(&target, &original)? {
            ensure_staged_target_or_absent(&target, &journal.install_id)?;
            let restore_root = root.join("legacy-recovery").join(install_id);
            let restore = restore_root.join("ChatGPT.app");
            remove_path(&restore)?;
            fs::create_dir_all(&restore_root).map_err(|error| error.to_string())?;
            ditto(&original, &restore)?;
            if !verify_app(&restore) {
                return Err(
                    "legacy original restore candidate failed codesign verification".into(),
                );
            }
            replace_bundle(&restore, &target, &restore_root)?;
            if !verify_app(&target) {
                return Err("recovered legacy original failed codesign verification".into());
            }
        }
    } else if needs_original {
        let outgoing = outgoing
            .as_ref()
            .filter(|path| path.is_dir())
            .ok_or("legacy recovery has no intact original or outgoing restore source")?;
        ensure_staged_target_or_absent(&target, &journal.install_id)?;
        replace_bundle(
            outgoing,
            &target,
            &root.join("legacy-recovery").join(install_id),
        )?;
        outgoing_restored = true;
    } else if let Some(outgoing) = &outgoing {
        if outgoing.is_dir() && !target.exists() {
            fs::rename(outgoing, &target).map_err(|error| error.to_string())?;
            outgoing_restored = true;
        }
    }
    remove_path(&staged)?;
    if let Some(outgoing) = &outgoing {
        remove_path(outgoing)?;
    }
    let mut rolled_back = journal;
    rolled_back.phase = "ROLLED_BACK".into();
    write_legacy_journal(root, &rolled_back)?;
    Ok(LegacyRecoveryResult {
        install_id: rolled_back.install_id,
        phase: rolled_back.phase,
        action: "rollback",
        target,
        backup_intact: original.is_dir(),
        staged_removed: !staged.exists(),
        outgoing_restored,
    })
}

fn done_result(journal: &TransactionJournal) -> LegacyRecoveryResult {
    LegacyRecoveryResult {
        install_id: journal.install_id.clone(),
        phase: journal.phase.clone(),
        action: "done",
        target: canonical_path(&journal.target_real_path),
        backup_intact: Path::new(&journal.original_snapshot).is_dir(),
        staged_removed: !Path::new(&journal.staged_app).exists(),
        outgoing_restored: false,
    }
}

fn replace_bundle(source: &Path, target: &Path, work_root: &Path) -> Result<(), String> {
    let trash = work_root.join("trash/ChatGPT.app");
    remove_path(&trash)?;
    if target.exists() {
        fs::create_dir_all(trash.parent().unwrap()).map_err(|error| error.to_string())?;
        fs::rename(target, &trash).map_err(|error| error.to_string())?;
    }
    if let Err(error) = fs::rename(source, target) {
        if trash.exists() {
            let _ = fs::rename(&trash, target);
        }
        return Err(error.to_string());
    }
    remove_path(&trash)
}

fn ensure_staged_target_or_absent(target: &Path, install_id: &str) -> Result<(), String> {
    if !target.exists() {
        return Ok(());
    }
    let archive = Archive::open(target.join("Contents/Resources/app.asar"))?;
    let package = archive.read_package_main()?;
    if package.already_patched && package.install_id.as_deref() == Some(install_id) {
        Ok(())
    } else {
        Err("legacy recovery target no longer belongs to this transaction".into())
    }
}

fn already_restored(target: &Path, original: &Path) -> Result<bool, String> {
    if !target.exists() {
        return Ok(false);
    }
    if tree_digest(target)? != tree_digest(original)? {
        return Ok(false);
    }
    Ok(verify_app(target))
}

fn remove_path(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            fs::remove_dir_all(path).map_err(|error| error.to_string())
        }
        Ok(_) => fs::remove_file(path).map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
