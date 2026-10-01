/*
 * [INPUT]: 依赖绑定的事务快照、ASAR marker、官方原件政策与 root 级签名验收。
 * [OUTPUT]: 提供安装 skip/卸载/恢复共用的 original 与 live 证明，不进行签名或授权。
 * [POS]: install 的只读证明层，危险变更在 install_mutation，确认与展示在父模块。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;

pub(super) fn prune_warning(root: &Path, app: &Path, install_id: &str) -> Option<String> {
    prune_superseded_terminal(root, install_id, app)
        .err()
        .map(|error| {
            format!("Current install is valid, but old transaction cleanup failed: {error}")
        })
}

#[cfg(test)]
pub(super) fn begin_verified_transaction<F>(
    root: &Path,
    app: &Path,
    validate_locked_target: F,
) -> Result<Engine, String>
where
    F: FnOnce(&Path) -> Result<(), String>,
{
    begin_verified_transaction_with_quiescence(
        root,
        app,
        NoopQuiescenceGuard,
        validate_locked_target,
    )
}

pub(super) fn begin_verified_transaction_with_quiescence<Q, F>(
    root: &Path,
    app: &Path,
    quiescence: Q,
    validate_locked_target: F,
) -> Result<Engine, String>
where
    Q: QuiescenceGuard + Clone,
    F: FnOnce(&Path) -> Result<(), String>,
{
    let mut tx = Engine::begin_with_quiescence(root, app, "install", quiescence.clone())?;
    if let Err(error) = validate_locked_target(tx.target_path()) {
        return match tx.rollback(&error) {
            Ok(()) => Err(error),
            Err(rollback) => Err(format!(
                "{error}; failed to roll back rejected transaction: {rollback}"
            )),
        };
    }
    Ok(tx)
}

pub(super) fn snapshot_original(
    tx: &mut Engine,
    app: &Path,
    original: &Path,
) -> Result<(), String> {
    if let Err(error) = ditto(app, original) {
        return Err(rollback_snapshot_failure(tx, error));
    }
    if let Err(error) = tx.mark_backup_committed() {
        return Err(rollback_snapshot_failure(tx, error));
    }
    Ok(())
}

pub(super) fn rollback_snapshot_failure(tx: &mut Engine, error: String) -> String {
    let rollback = if tx.journal().phase == "DISCOVERED" {
        tx.abort_discovered_snapshot()
    } else {
        tx.rollback(&error)
    };
    match rollback {
        Ok(()) => error,
        Err(rollback) => {
            format!("{error}; failed to roll back rejected snapshot transaction: {rollback}")
        }
    }
}

pub(super) fn inspect_existing_install(
    app: &Path,
    root: &Path,
    asar: &Path,
) -> Result<Option<String>, String> {
    let Ok(archive) = Archive::open(asar) else {
        return Ok(None);
    };
    let loader = archive.extract(LOADER_NAME).ok();
    let has_loader = loader.is_some();
    let package = match archive.read_package_main() {
        Ok(package) => package,
        Err(_) if has_loader => return Err(unbound_patch_error()),
        Err(_) => return Ok(None),
    };
    if !has_loader && !package.already_patched && package.install_id.is_none() {
        return Ok(None);
    }
    let install_id = installed_install_id(app, root, &archive).ok_or_else(unbound_patch_error)?;
    if loader
        .as_deref()
        .is_some_and(|bytes| !loader_is_compatible(bytes))
    {
        return Err(
            "live app contains an Incodex loader that is not compatible with this CLI; refusing to synchronize Runtime"
                .into(),
        );
    }
    Ok(Some(install_id))
}

pub(super) fn loader_is_compatible(loader: &[u8]) -> bool {
    let digest: [u8; 32] = Sha256::digest(loader).into();
    loader == loader_source().as_bytes() || COMPATIBLE_HISTORICAL_LOADER_SHA256.contains(&digest)
}

pub(super) fn unbound_patch_error() -> String {
    "live app contains an Incodex marker or loader without a trusted committed installation record; refusing to create a new original snapshot".into()
}

pub(super) fn ensure_official_target_is_verified(app: &Path) -> Result<(), String> {
    if !is_official_app(app, None) {
        return Ok(());
    }
    let info = read_plist_info(app).ok_or_else(|| {
        "default target has no readable Info.plist; refusing to snapshot it".to_string()
    })?;
    ensure_official_bundle_identifier(&info)?;
    verify_original_vendor_bundle(
        app,
        Some(OFFICIAL_BUNDLE_IDENTIFIER),
        Some(&info.app_version),
        Some(&info.app_build),
    )
    .map(|_| ())
    .map_err(|error| format!("default target is not a verified official Codex app: {error}"))
}

pub(super) fn ensure_official_bundle_identifier(
    info: &incodex_macos::PlistInfo,
) -> Result<(), String> {
    if info.bundle_identifier == OFFICIAL_BUNDLE_IDENTIFIER {
        return Ok(());
    }
    Err(format!(
        "default target bundle identifier is not {OFFICIAL_BUNDLE_IDENTIFIER}; refusing to snapshot a foreign bundle"
    ))
}

pub(super) fn installed_install_id(app: &Path, root: &Path, archive: &Archive) -> Option<String> {
    let install_id = installed_marker_id(app, root, archive)?;
    if validate_committed_live_snapshot(root, &install_id, app).is_err()
        || validate_backup_snapshot(root, &install_id).is_err()
    {
        return None;
    }
    Some(install_id)
}

/// Read the live marker for uninstall's legacy migration path. The migration
/// proof performs the stronger backup/live validation before any restore.
pub(super) fn installed_marker_id(app: &Path, root: &Path, archive: &Archive) -> Option<String> {
    if !archive.has_only_loader() {
        return None;
    }
    let package = archive.read_package_main().ok()?;
    if !package.already_patched {
        return None;
    }
    let install_id = package.install_id?;
    let journal = journal_v2(root, &install_id).ok()?;
    if journal.phase != "COMMITTED" {
        return None;
    }
    let target = fs::canonicalize(app).ok()?;
    let journal_target = fs::canonicalize(&journal.target.real_path).ok()?;
    if target != journal_target || !crate::macos_signing::verify_for_root(root, app) {
        return None;
    }
    let original = root
        .join("transactions")
        .join(&install_id)
        .join(&journal.paths.original);
    if !original.exists() || read_asar_integrity(app) != Some(archive.header_hash()) {
        return None;
    }
    Some(install_id)
}

pub(super) fn verified_live_install_id(root: &Path, app: &Path) -> Option<String> {
    Archive::open(app.join(ASAR_REL))
        .ok()
        .and_then(|archive| installed_marker_id(app, root, &archive))
}

pub(super) fn find_committed(
    root: &Path,
    app: &Path,
) -> Result<incodex_transaction::JournalV2, String> {
    let real = inspect_target(app, None)
        .map(|t| t.real_path)
        .unwrap_or_else(|_| app.to_path_buf());
    let live_install_id = verified_live_install_id(root, app);
    let dir = root.join("transactions");
    let entries = fs::read_dir(&dir).map_err(|_| {
        "no installation record for this target. refusing to use ~/.incodex/backup because it is not bound to this app"
            .to_string()
    })?;
    let mut best: Option<incodex_transaction::JournalV2> = None;
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let Ok(journal) = incodex_transaction::journal_v2(root, &id) else {
            continue;
        };
        if journal.phase != "COMMITTED" {
            continue;
        }
        if Path::new(&journal.target.real_path) != real {
            continue;
        }
        if live_install_id.as_deref() != Some(journal.install_id.as_str()) {
            continue;
        }
        if best
            .as_ref()
            .map(|cur| journal.sequence > cur.sequence)
            .unwrap_or(true)
        {
            best = Some(journal);
        }
    }
    best.ok_or_else(|| {
        "no installation record for this target. refusing to use ~/.incodex/backup because it is not bound to this app"
            .to_string()
    })
}
