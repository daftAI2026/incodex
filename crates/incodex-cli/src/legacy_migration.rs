use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use incodex_asar::{Archive, MARKER_KEY};
use incodex_core::{canonical::canonical_path, is_official_app, target_id};
use incodex_macos::{ditto, read_plist_info, read_plist_json_file, verify_app};
use incodex_transaction::{
    acquire_target_lock, journal_v2, sync_tree_and_ancestors, tree_digest, tree_digest_excluding,
    validate_path_ancestors, JournalV2,
};

use crate::legacy_proof::LegacyProvenState;
use crate::legacy_typescript::{
    load_legacy_journal, load_legacy_ts_v1, write_legacy_journal_with_checkpoint, LegacyState,
    TransactionJournal,
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
            // A legacy record can outlive an official upgrade.  If the live
            // target is already the sealed original (or a verified vendor
            // bundle), the old record is historical rather than a patched
            // state that must be adopted.  A merely well-signed foreign app
            // still falls through to the proof gate and fails closed.
            if legacy_target_is_clean_or_vendor(target, &state)? {
                return Ok(None);
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
    validate_path_ancestors(
        root,
        &format!("legacy-recovery/{install_id}/outgoing-proof"),
    )?;
    let outgoing_proof = outgoing_proof_path(root, install_id);
    let needs_original = matches!(
        journal.phase.as_str(),
        "TARGET_MOVED_OUT" | "SWAPPED" | "TARGET_VERIFIED"
    );
    let mut outgoing_restored = false;
    let mut outgoing_already_restored = false;
    if journal.recovery_intent.as_deref() == Some("restore-outgoing") {
        let digest = journal
            .recovery_digest
            .as_deref()
            .ok_or("legacy outgoing restore intent has no digest")?;
        if outgoing_proof.is_dir() && tree_digest(&outgoing_proof)? != digest {
            return Err("legacy outgoing proof copy does not match its durable digest".into());
        }
        if target.exists() && tree_digest(&target)? == digest {
            verify_restore_candidate(root, &target, &target, &journal.install_id)?;
            finalize_bundle_replace(&root.join("legacy-recovery").join(install_id))?;
            outgoing_already_restored = true;
        } else if let Some(outgoing) = &outgoing {
            if outgoing_proof.is_dir() && (!outgoing.exists() || tree_digest(outgoing)? != digest) {
                remove_path(outgoing)?;
                fs::create_dir_all(outgoing.parent().ok_or("legacy outgoing has no parent")?)
                    .map_err(|error| error.to_string())?;
                ditto(&outgoing_proof, outgoing)?;
            }
            if outgoing.is_dir() {
                // Re-enter the normal source validation below.  The proof copy
                // is retained until that validation and the post-rename proof
                // have both completed.
            } else {
                return Err(
                    "legacy outgoing restore intent cannot prove the restored target".into(),
                );
            }
        } else {
            return Err("legacy outgoing restore intent cannot prove the restored target".into());
        }
    }
    if needs_original && original.is_dir() {
        if !already_restored(root, &target, &original, &journal.install_id)? {
            ensure_staged_target_or_absent(
                root,
                &target,
                &staged,
                &journal.install_id,
                Some(&original),
            )?;
            let restore_root = root.join("legacy-recovery").join(install_id);
            let restore = restore_root.join("ChatGPT.app");
            remove_path(&restore)?;
            fs::create_dir_all(&restore_root).map_err(|error| error.to_string())?;
            ditto(&original, &restore)?;
            verify_restore_candidate(root, &target, &restore, &journal.install_id)?;
            replace_bundle(&restore, &target, &restore_root)?;
            checkpoint("AFTER_RESTORE_RENAME");
            if let Err(error) =
                verify_restore_candidate(root, &target, &target, &journal.install_id)
            {
                undo_bundle_replace(&restore, &target, &restore_root)?;
                return Err(error);
            }
            finalize_bundle_replace(&restore_root)?;
        } else {
            finalize_bundle_replace(&root.join("legacy-recovery").join(install_id))?;
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
        prepare_outgoing_proof(root, install_id, outgoing, &outgoing_digest)?;
        intent.recovery_digest = Some(outgoing_digest.clone());
        write_legacy_journal_with_checkpoint(root, &intent, || {
            checkpoint("BEFORE_LEGACY_INTENT_JOURNAL_RENAME")
        })?;
        checkpoint("AFTER_RESTORE_INTENT");
        ensure_staged_target_or_absent(
            root,
            &target,
            &staged,
            &journal.install_id,
            Some(outgoing),
        )?;
        if tree_digest(outgoing)? != outgoing_digest {
            return Err("legacy outgoing source changed after restore intent".into());
        }
        replace_bundle(
            outgoing,
            &target,
            &root.join("legacy-recovery").join(install_id),
        )?;
        checkpoint("AFTER_RESTORE_RENAME");
        if let Err(error) = verify_restore_candidate(root, &target, &target, &journal.install_id)
            .and_then(|_| {
                if tree_digest(&target)? != outgoing_digest {
                    return Err("recovered legacy outgoing full-tree proof mismatch".into());
                }
                Ok(())
            })
        {
            undo_bundle_replace(
                outgoing,
                &target,
                &root.join("legacy-recovery").join(install_id),
            )?;
            return Err(error);
        }
        finalize_bundle_replace(&root.join("legacy-recovery").join(install_id))?;
        remove_path(&outgoing_proof)?;
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
                let outgoing_digest = tree_digest(outgoing)?;
                prepare_outgoing_proof(root, install_id, outgoing, &outgoing_digest)?;
                intent.recovery_digest = Some(outgoing_digest.clone());
                write_legacy_journal_with_checkpoint(root, &intent, || {
                    checkpoint("BEFORE_LEGACY_INTENT_JOURNAL_RENAME")
                })?;
                checkpoint("AFTER_RESTORE_INTENT");
                if tree_digest(outgoing)? != outgoing_digest {
                    return Err("legacy outgoing source changed after restore intent".into());
                }
                replace_bundle(
                    outgoing,
                    &target,
                    &root.join("legacy-recovery").join(install_id),
                )?;
                checkpoint("AFTER_RESTORE_RENAME");
                if let Err(error) = verify_restore_candidate(
                    root,
                    &target,
                    &target,
                    &journal.install_id,
                )
                .and_then(|_| {
                    if tree_digest(&target)? != outgoing_digest {
                        return Err("recovered legacy outgoing full-tree proof mismatch".into());
                    }
                    Ok(())
                })
                {
                    undo_bundle_replace(
                        outgoing,
                        &target,
                        &root.join("legacy-recovery").join(install_id),
                    )?;
                    return Err(error);
                }
                finalize_bundle_replace(&root.join("legacy-recovery").join(install_id))?;
                remove_path(&outgoing_proof)?;
                outgoing_restored = true;
            }
        }
    }
    // Make the restored bundle durable before deleting the alternate restore
    // sources.  The journal must never outlive both its target and its proof.
    if target.is_dir() {
        sync_tree_and_ancestors(&target, target.parent().ok_or("target has no parent")?)?;
    }
    remove_path(&staged)?;
    if let Some(outgoing) = &outgoing {
        remove_path(outgoing)?;
    }
    remove_path(&outgoing_proof)?;
    finalize_bundle_replace(&root.join("legacy-recovery").join(install_id))?;
    checkpoint("AFTER_RECOVERY_CLEANUP");
    sync_recovery_mutations(root, &target, &staged, outgoing.as_deref())?;
    let mut rolled_back = journal;
    rolled_back.phase = "ROLLED_BACK".into();
    checkpoint("BEFORE_ROLLED_BACK_JOURNAL");
    write_legacy_journal_with_checkpoint(root, &rolled_back, || {
        checkpoint("BEFORE_LEGACY_ROLLED_BACK_JOURNAL_RENAME")
    })?;
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
    Ok(())
}

fn finalize_bundle_replace(work_root: &Path) -> Result<(), String> {
    remove_path(&work_root.join("trash/ChatGPT.app"))
}

fn undo_bundle_replace(source: &Path, target: &Path, work_root: &Path) -> Result<(), String> {
    if source.exists() {
        return Err("cannot undo legacy replacement: source path already exists".into());
    }
    if target.exists() {
        fs::rename(target, source).map_err(|error| error.to_string())?;
    }
    let trash = work_root.join("trash/ChatGPT.app");
    if trash.exists() {
        fs::rename(&trash, target).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn outgoing_proof_path(root: &Path, install_id: &str) -> PathBuf {
    root.join("legacy-recovery")
        .join(install_id)
        .join("outgoing-proof/ChatGPT.app")
}

fn prepare_outgoing_proof(
    root: &Path,
    install_id: &str,
    outgoing: &Path,
    expected_digest: &str,
) -> Result<(), String> {
    let proof = outgoing_proof_path(root, install_id);
    remove_path(&proof)?;
    fs::create_dir_all(
        proof
            .parent()
            .ok_or("legacy outgoing proof has no parent")?,
    )
    .map_err(|error| error.to_string())?;
    ditto(outgoing, &proof)?;
    if tree_digest(&proof)? != expected_digest {
        return Err("legacy outgoing proof copy does not match source".into());
    }
    Ok(())
}

fn ensure_staged_target_or_absent(
    root: &Path,
    target: &Path,
    staged: &Path,
    install_id: &str,
    proof_source: Option<&Path>,
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
        verify_patched_target_seal(root, target, install_id, proof_source)?;
        if staged.is_dir() && tree_digest(target)? != tree_digest(staged)? {
            return Err("legacy recovery target does not match staged tree".into());
        }
        verify_manifest_identity(root, target, install_id)
    } else {
        Err("legacy recovery target no longer belongs to this transaction".into())
    }
}

fn verify_patched_target_seal(
    root: &Path,
    target: &Path,
    install_id: &str,
    proof_source: Option<&Path>,
) -> Result<(), String> {
    let manifest = legacy_manifest(root, target, install_id)?;
    let asar = target.join("Contents/Resources/app.asar");
    let archive = Archive::open(&asar)?;
    if archive.header_hash() != manifest.patched_asar_header_hash
        || sha256_file(&asar)? != manifest.patched_asar_file_hash
    {
        return Err("legacy recovery target does not match the sealed patched ASAR".into());
    }
    let original = proof_source.map(Path::to_path_buf).unwrap_or_else(|| {
        root.join("installations")
            .join(target_id(target))
            .join(install_id)
            .join("original/ChatGPT.app")
    });
    if !original.is_dir() {
        return Err("legacy recovery has no sealed original for patched tree proof".into());
    }
    // The ASAR and code signature are the only bundle members the retired
    // installer is allowed to rewrite.  Comparing the remaining tree against
    // the sealed original catches a different non-ASAR app even when it was
    // re-signed and still passes a shallow signature check.
    let info = read_plist_info(target).ok_or("legacy recovery target Info.plist is unreadable")?;
    let executable_member = format!("Contents/MacOS/{}", info.executable);
    let patched_members = vec![
        "Contents/Resources/app.asar",
        "Contents/_CodeSignature",
        "Contents/Info.plist",
        executable_member.as_str(),
    ];
    if tree_digest_excluding(target, &patched_members)?
        != tree_digest_excluding(&original, &patched_members)?
    {
        return Err("legacy recovery target full-tree proof mismatch".into());
    }
    let target_plist = target.join("Contents/Info.plist");
    let original_plist = original.join("Contents/Info.plist");
    if normalized_plist(&target_plist, Some(&manifest.patched_asar_header_hash))?
        != normalized_plist(&original_plist, None)?
    {
        return Err("legacy recovery target Info.plist proof mismatch".into());
    }
    let target_mode = fs::symlink_metadata(&target_plist)
        .map_err(|error| error.to_string())?
        .mode()
        & 0o7777;
    let original_mode = fs::symlink_metadata(&original_plist)
        .map_err(|error| error.to_string())?
        .mode()
        & 0o7777;
    if target_mode != original_mode {
        return Err("legacy recovery target Info.plist mode mismatch".into());
    }
    Ok(())
}

fn normalized_plist(
    path: &Path,
    expected_patched_hash: Option<&str>,
) -> Result<serde_json::Value, String> {
    let mut plist = read_plist_json_file(path)?;
    let Some(root) = plist.as_object_mut() else {
        return Err(format!("Info.plist {} is not a dictionary", path.display()));
    };
    let Some(integrity) = root.get_mut("ElectronAsarIntegrity") else {
        if expected_patched_hash.is_some() {
            return Err(format!(
                "Info.plist {} has no ElectronAsarIntegrity",
                path.display()
            ));
        }
        return Ok(plist);
    };
    let Some(entries) = integrity.as_object_mut() else {
        return Err(format!(
            "Info.plist {} has invalid ElectronAsarIntegrity",
            path.display()
        ));
    };
    let app_entry = "Resources/app.asar";
    let Some(entry) = entries.get_mut(app_entry) else {
        if expected_patched_hash.is_some() {
            return Err(format!(
                "Info.plist {} has no Resources/app.asar integrity entry",
                path.display()
            ));
        }
        return Ok(plist);
    };
    let Some(entry) = entry.as_object_mut() else {
        return Err(format!(
            "Info.plist {} has invalid Resources/app.asar integrity entry",
            path.display()
        ));
    };
    if let Some(expected_hash) = expected_patched_hash {
        if entry.get("algorithm").and_then(serde_json::Value::as_str) != Some("SHA256")
            || entry.get("hash").and_then(serde_json::Value::as_str) != Some(expected_hash)
        {
            return Err(format!(
                "Info.plist {} has an invalid Resources/app.asar integrity entry",
                path.display()
            ));
        }
    }
    // TS v1 changes only these two fields. Preserve every other integrity
    // entry and field so unrelated plist drift remains a hard failure.
    entry.remove("algorithm");
    entry.remove("hash");
    if entry.is_empty() {
        entries.remove(app_entry);
    }
    if entries.is_empty() {
        root.remove("ElectronAsarIntegrity");
    }
    Ok(plist)
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
