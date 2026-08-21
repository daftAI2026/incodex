use std::fs;
use std::path::{Path, PathBuf};

use incodex_asar::{Archive, MARKER_KEY};
use incodex_core::{canonical::canonical_path, is_official_app, target_id};
use incodex_macos::{ditto, read_plist_info, verify_app};
use incodex_transaction::{
    acquire_target_lock, journal_v2, sync_tree_and_ancestors, tree_digest, validate_path_ancestors,
    JournalV2,
};

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
    match &state.state {
        LegacyState::Committed { .. } => {
            // Native uninstall preserves the legacy record for rollback/audit, but
            // leaves a rolled-back v2 journal.  Do not re-prove the now-clean app
            // as a legacy patched target on a later invocation.
            if let Ok(native) = journal_v2(root, &state.install_id) {
                if native.phase == "COMMITTED" || native.phase == "ROLLED_BACK" {
                    if native.phase == "COMMITTED" {
                        if legacy_marker_matches_for_install(target, &state.install_id) {
                            return Ok(Some(native));
                        }
                        // A native committed record can outlive an official
                        // app upgrade.  Only a clean vendor bundle (or the
                        // exact sealed legacy original) is historical; a
                        // merely well-signed different bundle is foreign.
                        return if legacy_target_is_clean_or_vendor(target, &state)? {
                            Ok(None)
                        } else {
                            Err("legacy record points to a foreign modified app; refusing migration".into())
                        };
                    }
                    return Ok(None);
                }
                return Err(format!(
                    "adopted native transaction {} is not settled (phase {})",
                    state.install_id, native.phase
                ));
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
    recover_legacy_ts_v1_with_checkpoint(root, install_id, |_| {})
}

/// Test-only recovery entrypoint.  The callback is invoked only after durable
/// boundaries, so subprocess SIGKILL tests exercise the same restart points as
/// the product path without adding a production kill switch.
#[doc(hidden)]
pub fn recover_legacy_ts_v1_with_checkpoint<F>(
    root: &Path,
    install_id: &str,
    mut checkpoint: F,
) -> Result<LegacyRecoveryResult, String>
where
    F: FnMut(&str),
{
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
    validate_path_ancestors(root, &format!("legacy-recovery/{install_id}/trash"))?;
    let needs_original = matches!(
        journal.phase.as_str(),
        "TARGET_MOVED_OUT" | "SWAPPED" | "TARGET_VERIFIED"
    );
    let mut outgoing_restored = false;
    let mut outgoing_already_restored = false;
    if journal.recovery_intent.as_deref() == Some("restore-outgoing")
        && outgoing.as_ref().is_none_or(|path| !path.exists())
    {
        let digest = journal
            .recovery_digest
            .as_deref()
            .ok_or("legacy outgoing restore intent has no digest")?;
        if target.exists() && tree_digest(&target)? == digest {
            verify_restore_candidate(root, &target, &target, &journal.install_id)?;
            outgoing_already_restored = true;
        } else {
            return Err("legacy outgoing restore intent cannot prove the restored target".into());
        }
    }
    if needs_original && original.is_dir() {
        if !already_restored(root, &target, &original, &journal.install_id)? {
            ensure_staged_target_or_absent(root, &target, &staged, &journal.install_id)?;
            let restore_root = root.join("legacy-recovery").join(install_id);
            let restore = restore_root.join("ChatGPT.app");
            remove_path(&restore)?;
            fs::create_dir_all(&restore_root).map_err(|error| error.to_string())?;
            ditto(&original, &restore)?;
            verify_restore_candidate(root, &target, &restore, &journal.install_id)?;
            replace_bundle(&restore, &target, &restore_root)?;
            checkpoint("AFTER_RESTORE_RENAME");
            verify_restore_candidate(root, &target, &target, &journal.install_id)?;
        }
    } else if needs_original && !outgoing_already_restored {
        let outgoing = outgoing
            .as_ref()
            .filter(|path| path.is_dir())
            .ok_or("legacy recovery has no intact original or outgoing restore source")?;
        verify_outgoing_candidate(root, &target, outgoing, &journal.install_id)?;
        let mut intent = journal.clone();
        intent.recovery_intent = Some("restore-outgoing".into());
        let outgoing_digest = tree_digest(outgoing)?;
        intent.recovery_digest = Some(outgoing_digest.clone());
        write_legacy_journal(root, &intent)?;
        checkpoint("AFTER_RESTORE_INTENT");
        ensure_staged_target_or_absent(root, &target, &staged, &journal.install_id)?;
        replace_bundle(
            outgoing,
            &target,
            &root.join("legacy-recovery").join(install_id),
        )?;
        checkpoint("AFTER_RESTORE_RENAME");
        verify_restore_candidate(root, &target, &target, &journal.install_id)?;
        if tree_digest(&target)? != outgoing_digest {
            return Err("recovered legacy outgoing full-tree proof mismatch".into());
        }
        outgoing_restored = true;
    } else if let Some(outgoing) = &outgoing {
        if outgoing.is_dir() {
            verify_outgoing_candidate(root, &target, outgoing, &journal.install_id)?;
            if target.exists() {
                if tree_digest(&target)? != tree_digest(outgoing)? {
                    return Err("legacy pre-swap target reappeared and is not the original".into());
                }
                verify_restore_candidate(root, &target, &target, &journal.install_id)?;
            } else {
                let mut intent = journal.clone();
                intent.recovery_intent = Some("restore-outgoing".into());
                intent.recovery_digest = Some(tree_digest(outgoing)?);
                write_legacy_journal(root, &intent)?;
                checkpoint("AFTER_RESTORE_INTENT");
                fs::rename(outgoing, &target).map_err(|error| error.to_string())?;
                checkpoint("AFTER_RESTORE_RENAME");
                verify_restore_candidate(root, &target, &target, &journal.install_id)?;
                outgoing_restored = true;
            }
        }
    }
    remove_path(&staged)?;
    if let Some(outgoing) = &outgoing {
        remove_path(outgoing)?;
    }
    checkpoint("AFTER_RECOVERY_CLEANUP");
    if target.is_dir() {
        sync_tree_and_ancestors(&target, target.parent().ok_or("target has no parent")?)?;
    }
    sync_recovery_mutations(root, &target, &staged, outgoing.as_deref())?;
    let mut rolled_back = journal;
    rolled_back.phase = "ROLLED_BACK".into();
    checkpoint("BEFORE_ROLLED_BACK_JOURNAL");
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

fn ensure_staged_target_or_absent(
    root: &Path,
    target: &Path,
    staged: &Path,
    install_id: &str,
) -> Result<(), String> {
    if !target.exists() {
        return Ok(());
    }
    let archive = Archive::open(target.join("Contents/Resources/app.asar"))?;
    let package = archive.read_package_main()?;
    if package.already_patched && package.install_id.as_deref() == Some(install_id) {
        if !verify_app(target) {
            return Err("legacy recovery target failed signature verification".into());
        }
        if staged.is_dir() && tree_digest(target)? != tree_digest(staged)? {
            return Err("legacy recovery target does not match staged tree".into());
        }
        verify_manifest_identity(root, target, install_id)
    } else {
        Err("legacy recovery target no longer belongs to this transaction".into())
    }
}

fn already_restored(
    root: &Path,
    target: &Path,
    original: &Path,
    install_id: &str,
) -> Result<bool, String> {
    if !target.exists() {
        return Ok(false);
    }
    if tree_digest(target)? != tree_digest(original)? {
        return Ok(false);
    }
    Ok(verify_restore_candidate(root, target, target, install_id).is_ok())
}

fn verify_outgoing_candidate(
    root: &Path,
    target: &Path,
    outgoing: &Path,
    install_id: &str,
) -> Result<(), String> {
    verify_restore_candidate(root, target, outgoing, install_id)
}

fn verify_restore_candidate(
    root: &Path,
    target: &Path,
    candidate: &Path,
    install_id: &str,
) -> Result<(), String> {
    if !candidate.is_dir() {
        return Err("legacy restore candidate is not a directory".into());
    }
    ensure_real_path(candidate, "legacy restore candidate")?;
    let manifest = legacy_manifest(root, target, install_id)?;
    let info =
        read_plist_info(candidate).ok_or("legacy restore candidate Info.plist is unreadable")?;
    if info.bundle_identifier != manifest.bundle_identifier
        || info.app_version != manifest.app_version
        || info.app_build != manifest.app_build
    {
        return Err("legacy restore candidate bundle identity mismatch".into());
    }
    ensure_real_path(
        &candidate.join("Contents/Info.plist"),
        "legacy restore candidate Info.plist",
    )?;
    ensure_real_path(
        &candidate.join("Contents/Resources/app.asar"),
        "legacy restore candidate ASAR",
    )?;
    ensure_real_path(
        &candidate.join("Contents/MacOS").join(&info.executable),
        "legacy restore candidate executable",
    )?;
    verify_bundle_hashes(root, target, candidate, install_id)?;
    let original = root
        .join("installations")
        .join(target_id(target))
        .join(install_id)
        .join("original/ChatGPT.app");
    if original.is_dir() && tree_digest(candidate)? != tree_digest(&original)? {
        return Err("legacy restore candidate full-tree proof mismatch".into());
    }
    if is_official_app(target, None) {
        crate::legacy_proof::verify_official_vendor_bundle(candidate, &manifest.bundle_identifier)?;
    } else if !verify_app(candidate) {
        return Err("legacy restore candidate signature verification failed".into());
    }
    Ok(())
}

fn verify_manifest_identity(root: &Path, target: &Path, install_id: &str) -> Result<(), String> {
    let manifest = legacy_manifest(root, target, install_id)?;
    let info = read_plist_info(target).ok_or("legacy recovery Info.plist is unreadable")?;
    if info.bundle_identifier != manifest.bundle_identifier
        || info.app_version != manifest.app_version
        || info.app_build != manifest.app_build
    {
        return Err("legacy recovery bundle identity mismatch".into());
    }
    Ok(())
}

fn ensure_real_path(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| format!("{label}: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{label} is a symlink"));
    }
    Ok(())
}

fn verify_bundle_hashes(
    root: &Path,
    target: &Path,
    candidate: &Path,
    install_id: &str,
) -> Result<(), String> {
    let manifest = legacy_manifest(root, target, install_id)?;
    let asar = candidate.join("Contents/Resources/app.asar");
    let plist = candidate.join("Contents/Info.plist");
    if sha256_file(&asar)? != manifest.original_asar_file_hash
        || sha256_file(&plist)? != manifest.original_plist_file_hash
    {
        return Err("legacy restore candidate does not match the sealed original".into());
    }
    Ok(())
}

fn legacy_manifest(
    root: &Path,
    target: &Path,
    install_id: &str,
) -> Result<crate::legacy_typescript::InstallManifest, String> {
    let path = root
        .join("installations")
        .join(target_id(target))
        .join(install_id)
        .join("manifest.json");
    let bytes =
        fs::read(path).map_err(|error| format!("legacy manifest is unreadable: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("legacy manifest is invalid: {error}"))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    Ok(
        Sha256::digest(fs::read(path).map_err(|error| error.to_string())?)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}

pub(crate) fn legacy_marker_matches_for_install(target: &Path, install_id: &str) -> bool {
    Archive::open(target.join("Contents/Resources/app.asar"))
        .ok()
        .and_then(|archive| archive.read_package_main().ok())
        .is_some_and(|package| {
            package.already_patched && package.install_id.as_deref() == Some(install_id)
        })
}

fn legacy_target_is_clean_or_vendor(
    target: &Path,
    state: &crate::legacy_typescript::LegacyTsV1State,
) -> Result<bool, String> {
    let LegacyState::Committed {
        original_app,
        manifest,
        ..
    } = &state.state
    else {
        return Ok(false);
    };
    let archive = Archive::open(target.join("Contents/Resources/app.asar"))?;
    let package = archive.read_package_main()?;
    let package_json: serde_json::Value = serde_json::from_slice(&archive.extract("package.json")?)
        .map_err(|error| format!("legacy target package metadata is invalid: {error}"))?;
    if package.already_patched
        || package.install_id.is_some()
        || package_json.get(MARKER_KEY).is_some()
    {
        return Ok(false);
    }
    if original_app.is_dir() && tree_digest(target)? == tree_digest(original_app)? {
        return Ok(true);
    }
    if is_official_app(target, None) {
        return Ok(crate::legacy_proof::verify_official_vendor_bundle(
            target,
            &manifest.bundle_identifier,
        )
        .is_ok());
    }
    Ok(false)
}

fn sync_recovery_mutations(
    root: &Path,
    target: &Path,
    staged: &Path,
    outgoing: Option<&Path>,
) -> Result<(), String> {
    if let Some(directory) = target.parent() {
        sync_directory(directory)?;
    }
    if let Some(directory) = staged.parent() {
        sync_directory(directory)?;
    }
    if let Some(directory) = outgoing.and_then(Path::parent) {
        sync_directory(directory)?;
    }
    sync_directory(&root.join("transactions"))?;
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW);
    options
        .open(path)
        .map_err(|error| error.to_string())?
        .sync_all()
        .map_err(|error| error.to_string())
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
