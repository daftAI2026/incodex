/*
 * [INPUT]: 依赖 install 的 marker/备份证明、quiescence、事务引擎与明确的签名上下文。
 * [OUTPUT]: 提供安装/恢复/卸载的事务变更流程，长备份后再解锁、签名前后复验，失败回滚。
 * [POS]: install 命令的危险变更实现，展示/确认与只读证明保留在父模块。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;
use crate::macos_signing::{context_for_install, log_signing_phase};
use incodex_macos::{sign_staged_app_with_context, verify_patched_bundle_with_context};

pub(super) fn install_app_with_quiescence<G>(
    app: &Path,
    root: &Path,
    progress: &mut Progress,
    quiescence: G,
) -> Result<CommandResult, String>
where
    G: QuiescenceGuard + Clone,
{
    install_app_for_expected_build(app, root, progress, quiescence, None)
}

fn install_app_for_expected_build<G>(
    app: &Path,
    root: &Path,
    progress: &mut Progress,
    quiescence: G,
    expected_build: Option<u64>,
) -> Result<CommandResult, String>
where
    G: QuiescenceGuard + Clone,
{
    if !app.exists() {
        return Err(format!("Codex app not found: {}", app.display()));
    }
    quiescence.ensure_quiescent(app)?;
    ensure_expected_build(app, expected_build)?;
    let asar = app.join(ASAR_REL);
    let existing = inspect_existing_install(app, root, &asar)?;
    if existing.is_none() {
        ensure_official_target_is_verified(app)?;
    }
    progress.stage("Publishing Runtime");
    let published = ensure_current(root)?;
    let official_app = is_official_app(app, None);
    let mut keychain_warning = None;
    let keychain_registration = if official_app {
        let mut registration =
            crate::macos_keychain_assets::ensure_bundled_registration(root, app)?;
        if expected_build.is_none() {
            progress.stage("Authorizing Keychain continuity");
            if crate::macos_keychain_assets::authorize_registration(root, &registration)?
                == crate::macos_keychain_assets::KeychainAuthorization::ItemMissing
            {
                keychain_warning = Some(
                    "Codex has not created its Storage Key yet; sign in to Codex, then run `incodex install` once more to enable update-safe Keychain continuity."
                        .to_string(),
                );
            }
            registration = crate::macos_keychain_assets::read_registration(root)?
                .ok_or("macOS Keychain registration disappeared after authorization")?;
        }
        Some(registration)
    } else {
        None
    };
    if let Some(install_id) = inspect_existing_install(app, root, &asar)? {
        let mut warnings = keychain_warning.into_iter().collect::<Vec<_>>();
        if official_app
            && incodex_macos::inspect_outer_signing(app)?.kind
                == incodex_macos::SignatureKind::Adhoc
        {
            warnings.push("Existing ad-hoc install was not migrated to stable signing. Explicitly uninstall then install once before testing Accessibility across official updates; the new identity may require an initial user grant.".into());
        }
        if let Some(warning) = prune_warning(root, app, &install_id) {
            warnings.push(warning);
        }
        let warning = (!warnings.is_empty()).then(|| warnings.join(" "));
        return Ok(CommandResult {
            skipped: true,
            install_id: Some(install_id),
            runtime_version: Some(published.version),
            app: app.display().to_string(),
            warning,
        });
    }
    if let Ok(archive) = Archive::open(&asar) {
        let has_loader = archive.extract(LOADER_NAME).is_ok();
        if let Ok(package) = archive.read_package_main() {
            if has_loader || package.already_patched || package.install_id.is_some() {
                return Err(unbound_patch_error());
            }
        } else if has_loader {
            return Err(unbound_patch_error());
        }
    }
    let signing_context = context_for_install(root, official_app, expected_build.is_some())?;
    let expected_plist = read_plist_info(app);
    let signing_build = expected_plist
        .as_ref()
        .map(|info| info.app_build.as_str())
        .unwrap_or("unknown");
    let transaction_quiescence = quiescence.clone();
    let mut tx = begin_verified_transaction_with_quiescence(
        root,
        app,
        transaction_quiescence,
        |locked_app| {
            ensure_expected_build(locked_app, expected_build)?;
            let locked_asar = locked_app.join(ASAR_REL);
            if inspect_existing_install(locked_app, root, &locked_asar)?.is_some() {
                return Err(
                "live app changed into an existing Incodex installation after preflight; refusing to snapshot it"
                    .into(),
            );
            }
            ensure_official_target_is_verified(locked_app)
        },
    )?;
    let install_id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&install_id)
        .join("original")
        .join("ChatGPT.app");
    log_signing_phase(
        root,
        "signing-identity",
        signing_build,
        &install_id,
        &signing_context,
    );
    progress.stage("Backing up original app");
    if let Err(error) = quiescence.ensure_quiescent(app) {
        return Err(rollback_install(&mut tx, None, error));
    }
    snapshot_original(&mut tx, app, &original)?;
    let staged = root
        .join("scratch")
        .join(format!("ChatGPT.app.staged-{install_id}"));
    progress.stage("Patching and signing app");
    if let Err(error) = quiescence.ensure_quiescent(app) {
        return Err(rollback_install(&mut tx, Some(&staged), error));
    }
    if let Err(error) = ditto(app, &staged) {
        return Err(rollback_install(&mut tx, Some(&staged), error));
    }
    if let Err(error) = quiescence.ensure_quiescent(app) {
        return Err(rollback_install(&mut tx, Some(&staged), error));
    }
    let (hash, _) = match patch_asar(&staged.join(ASAR_REL), loader_source(), Some(&install_id)) {
        Ok(result) => result,
        Err(error) => return Err(rollback_install(&mut tx, Some(&staged), error)),
    };
    let authorization_ready = crate::macos_keychain_assets::should_install_keychain_provider(
        keychain_registration.as_ref(),
    );
    if authorization_ready {
        let registration = keychain_registration
            .as_ref()
            .expect("provider readiness requires a registration");
        let helper_sha256 = &registration.helper_sha256;
        if let Err(error) =
            crate::macos_keychain_assets::install_keychain_provider(&staged, helper_sha256)
        {
            return Err(rollback_install(&mut tx, Some(&staged), error));
        }
    }
    if let Err(error) = quiescence.ensure_quiescent(app) {
        return Err(rollback_install(&mut tx, Some(&staged), error));
    }
    if is_official_app(app, None) || verify_app(app) || app.join("Contents/MacOS").exists() {
        // 私有 Keychain 会自动锁定；备份/补丁可能耗时，不能依赖预检时的解锁。
        if let incodex_macos::SigningContext::Local(identity) = &signing_context {
            if let Err(error) = crate::macos_signing_assets::unlock_signing_identity(root, identity)
            {
                return Err(rollback_install(&mut tx, Some(&staged), error));
            }
        }
        log_signing_phase(
            root,
            "signing-start",
            signing_build,
            &install_id,
            &signing_context,
        );
        if let Err(err) = sign_staged_app_with_context(&staged, app, &hash, &signing_context) {
            return Err(rollback_install(&mut tx, Some(&staged), err));
        }
    } else if let Err(error) = write_asar_integrity(&staged, &hash) {
        return Err(rollback_install(&mut tx, Some(&staged), error));
    }
    log_signing_phase(
        root,
        "signing-finished",
        signing_build,
        &install_id,
        &signing_context,
    );
    progress.stage("Replacing the app");
    if let Err(error) = quiescence.ensure_quiescent(app) {
        return Err(rollback_install(&mut tx, Some(&staged), error));
    }
    if let Err(error) = tx.place_staging(&staged) {
        return Err(rollback_install(&mut tx, Some(&staged), error));
    }
    if let Err(error) = quiescence.ensure_quiescent(app) {
        return Err(rollback_install(&mut tx, Some(&staged), error));
    }
    if let Err(error) = tx.swap() {
        return Err(rollback_install(&mut tx, Some(&staged), error));
    }
    progress.stage("Verifying installation");
    if let Err(error) =
        verify_patched_bundle_with_context(app, expected_plist.as_ref(), &signing_context)
    {
        let error = format!("post-swap codesign verification failed: {error}");
        return Err(rollback_install(&mut tx, Some(&staged), error));
    }
    let commit = match tx.commit() {
        Ok(result) => result,
        Err(error) => {
            return Err(rollback_install(&mut tx, Some(&staged), error));
        }
    };
    drop(tx);
    log_signing_phase(
        root,
        "recovery-committed",
        signing_build,
        &install_id,
        &signing_context,
    );
    let mut warnings = commit
        .cleanup_warning
        .map(|error| {
            format!(
                "Install committed, but transaction cleanup failed: {error}. Run `incodex recover --transaction {install_id}` to retry cleanup."
            )
        })
        .into_iter()
        .collect::<Vec<_>>();
    warnings.extend(keychain_warning);
    if let Some(warning) = prune_warning(root, app, &install_id) {
        warnings.push(warning);
    }
    let warning = (!warnings.is_empty()).then(|| warnings.join(" "));
    let _ = notify_launch_services(app);
    Ok(CommandResult {
        skipped: false,
        install_id: Some(install_id),
        runtime_version: Some(runtime_version()),
        app: app.display().to_string(),
        warning,
    })
}

pub(crate) fn reinstall_after_official_update(
    root: &Path,
    app: &Path,
    helper_source: &Path,
    expected_build: u64,
    expected_install_id: &str,
    expected_helper_sha256: &str,
    expected_signing_certificate_sha256: Option<&str>,
) -> Result<String, String> {
    let guard = RestoreGenerationGuard {
        app: AppGuard::for_app(app)?,
        root: root.to_path_buf(),
        install_id: expected_install_id.into(),
        helper_sha256: expected_helper_sha256.into(),
        certificate_sha256: expected_signing_certificate_sha256.map(str::to_string),
    };
    guard.ensure_quiescent(app)?;
    let mut progress = Progress::new();
    let result =
        install_app_for_expected_build(app, root, &mut progress, guard, Some(expected_build));
    progress.stop();
    let result = result?;
    register_update_restore_if_generation(
        root,
        app,
        helper_source,
        &result,
        expected_install_id,
        expected_helper_sha256,
        expected_signing_certificate_sha256,
    )?;
    result
        .install_id
        .ok_or_else(|| "restored app has no install epoch".into())
}

fn ensure_expected_build(app: &Path, expected_build: Option<u64>) -> Result<(), String> {
    let Some(expected_build) = expected_build else {
        return Ok(());
    };
    let observed = read_plist_info(app)
        .and_then(|info| info.app_build.parse::<u64>().ok())
        .ok_or_else(|| format!("cannot read Codex build from {}", app.display()))?;
    if observed == expected_build {
        Ok(())
    } else {
        Err(format!(
            "Codex build changed before update recovery: expected {expected_build}, found {observed}"
        ))
    }
}

pub(super) fn rollback_install(tx: &mut Engine, scratch: Option<&Path>, error: String) -> String {
    let rollback_error = match tx.journal().phase.as_str() {
        "COMMITTED" | "ROLLED_BACK" => None,
        _ => tx.rollback(&error).err(),
    };
    finish_rollback(tx, scratch, error, rollback_error)
}

pub(super) fn finish_rollback(
    tx: &Engine,
    scratch: Option<&Path>,
    error: String,
    rollback_error: Option<String>,
) -> String {
    let rollback_is_durable = tx.journal().phase == "ROLLED_BACK";
    let scratch_error = if rollback_error.is_none() || rollback_is_durable {
        scratch.and_then(|path| remove_install_scratch(path).err())
    } else {
        None
    };
    let mut details = Vec::new();
    if let Some(rollback_error) = rollback_error {
        if rollback_is_durable {
            details.push(format!(
                "rollback reached ROLLED_BACK, but durability confirmation reported an error: {rollback_error}"
            ));
        } else {
            details.push(format!(
                "transaction rollback failed; recover the retained journal: {rollback_error}"
            ));
        }
    }
    if let Some(scratch_error) = scratch_error {
        details.push(format!("install scratch cleanup failed: {scratch_error}"));
    }
    if details.is_empty() {
        error
    } else {
        format!("{error}; {}", details.join("; "))
    }
}

pub(super) fn remove_install_scratch(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_dir() {
        fs::remove_dir_all(path).map_err(|error| error.to_string())
    } else {
        fs::remove_file(path).map_err(|error| error.to_string())
    }
}

pub(super) fn uninstall_app_with_quiescence<Q>(
    app: &Path,
    root: &Path,
    progress: &mut Progress,
    quiescence: Q,
    official_target: bool,
) -> Result<(), String>
where
    Q: QuiescenceGuard + Clone,
{
    incodex_transaction::validate_storage_root(root)?;
    if !app.exists() {
        return Err(format!("Codex app not found: {}", app.display()));
    }
    quiescence.ensure_quiescent(app)?;
    progress.stage("Locating verified backup");
    let journal = find_committed(root, app)?;
    progress.stage("Restoring original app");
    if journal.target.parent_device.is_empty() {
        let install_id = journal.install_id.clone();
        migrate_legacy_committed_with_quiescence(
            root,
            &install_id,
            app,
            quiescence.clone(),
            |target| crate::macos_signing::verify_for_root(root, target),
            |live| verified_live_install_id(root, live).as_deref() == Some(install_id.as_str()),
        )?;
    } else {
        restore_committed_with_quiescence(
            root,
            &journal.install_id,
            app,
            quiescence.clone(),
            |_| {},
        )?;
    }
    verify_restored_app(app, official_target)?;
    finalize_restored_transaction(root, &journal.install_id, app).map_err(|error| {
        format!(
            "ChatGPT.app was restored, but transaction {0} could not be removed: {error}. Run `incodex recover --transaction {0}` to retry cleanup.",
            journal.install_id,
        )
    })?;
    progress.stage("Refreshing app registration");
    let _ = notify_launch_services(app);
    Ok(())
}

pub(super) fn verify_restored_app(app: &Path, official_target: bool) -> Result<(), String> {
    let archive = Archive::open(app.join(ASAR_REL))
        .map_err(|error| format!("restored app ASAR could not be inspected: {error}"))?;
    let package = archive.read_package_main().map_err(|error| {
        format!("restored app package metadata could not be inspected: {error}")
    })?;
    if package.already_patched || package.install_id.is_some() {
        return Err("restored app still contains an Incodex marker".into());
    }
    if archive.extract(LOADER_NAME).is_ok() {
        return Err("restored app still contains the Incodex loader".into());
    }
    if official_target {
        verify_original_vendor_bundle(app, Some(OFFICIAL_BUNDLE_IDENTIFIER), None, None)
            .map_err(|error| format!("restored official app failed vendor acceptance: {error}"))?;
    }
    Ok(())
}

#[derive(Clone)]
struct RestoreGenerationGuard {
    app: AppGuard,
    root: PathBuf,
    install_id: String,
    helper_sha256: String,
    certificate_sha256: Option<String>,
}

impl QuiescenceGuard for RestoreGenerationGuard {
    fn ensure_quiescent(&self, target: &Path) -> Result<(), String> {
        self.app.ensure_quiescent(target)?;
        crate::macos_signing::validate_restore_generation(
            &self.root,
            target,
            &self.install_id,
            &self.helper_sha256,
            self.certificate_sha256.as_deref(),
        )
    }
}
