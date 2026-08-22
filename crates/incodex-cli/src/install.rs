use std::fs;
use std::path::{Path, PathBuf};

use incodex_asar::{patch_asar, Archive, LOADER_NAME};
use incodex_core::canonical::{inspect_target, is_official_app};
use incodex_core::paths::{user_root, ASAR_REL, DEFAULT_APP};
use incodex_core::{format_kv, format_ok, format_step, format_warn};
use incodex_macos::{
    ditto, notify_launch_services, quit_official_app, read_asar_integrity, read_plist_info,
    sign_app, verify_app, verify_original_vendor_bundle, verify_patched_adhoc_bundle_deep_strict,
    write_asar_integrity, OFFICIAL_BUNDLE_IDENTIFIER,
};
use incodex_runtime_bundle::{loader_source, publish, runtime_version};
use incodex_transaction::{
    journal_v2, migrate_legacy_committed, recover_with, restore_committed,
    validate_backup_snapshot, validate_committed_live_snapshot, Engine, Recovery, TxError,
};

use crate::parse::ParsedCli;
use crate::spinner::Progress;

pub fn run_install(parsed: &ParsedCli) -> Result<(), String> {
    let root = user_root();
    let app = resolve_target(parsed, &root);
    if parsed.clone && parsed.app.is_none() {
        println!("{}", format_kv("Clone", &app.display().to_string(), None));
    }
    print_install_plan(&app, parsed.clone)?;
    if parsed.dry_run {
        println!("{}", format_warn("Dry run. No files changed.", None));
        return Ok(());
    }
    ensure_confirmed(parsed, "install")?;
    let mut progress = Progress::new();
    if parsed.clone && parsed.app.is_none() {
        progress.stage("Cloning official app");
        if !Path::new(DEFAULT_APP).exists() {
            return Err(format!("Codex app not found: {DEFAULT_APP}"));
        }
        ditto(Path::new(DEFAULT_APP), &app)?;
        progress.stop();
        println!("{}", format_ok("Cloned official app", None));
        println!("{}", format_kv("Target", &app.display().to_string(), None));
    }
    if parsed.live && parsed.app.is_none() {
        progress.stage("Closing ChatGPT");
        let _ = quit_official_app();
    }
    let result = install_app(&app, &root, &mut progress)?;
    progress.stop();
    print_command_result(&result);
    if parsed.live && parsed.app.is_none() {
        println!(
            "{}",
            format_ok("Done. Open ChatGPT.app when you want Incognito.", None)
        );
    } else {
        println!(
            "{}",
            format_ok("Restart that app copy to see the Incognito button.", None)
        );
    }
    println!();
    Ok(())
}

pub fn run_uninstall(parsed: &ParsedCli) -> Result<(), String> {
    let root = user_root();
    let app = resolve_target(parsed, &root);
    println!("{}", format_step("Uninstall", None));
    println!("{}", format_kv("App", &app.display().to_string(), None));
    if parsed.dry_run {
        println!("{}", format_warn("Dry run. No files changed.", None));
        return Ok(());
    }
    ensure_confirmed(parsed, "uninstall")?;
    let mut progress = Progress::new();
    let result = uninstall_app(&app, &root, &mut progress)?;
    progress.stop();
    println!(
        "{}",
        format_ok("Official app restored. Dock was refreshed.", None)
    );
    print_command_result(&result);
    println!();
    Ok(())
}

pub fn run_recover(parsed: &ParsedCli) -> Result<(), String> {
    let root = user_root();
    let id = parsed
        .transaction
        .as_deref()
        .ok_or("recover requires --transaction <id>\n  incodex recover --transaction <id>")?;
    let v2 = root.join("transactions").join(id).join("journal.json");
    let v1 = root.join("transactions").join(format!("{id}.json"));
    if !v2.exists() && !v1.exists() {
        return Err(format!("no journal for {id}"));
    }
    let mut progress = Progress::new();
    progress.stage("Recovering transaction");
    let result = recover_with(&root, id, verify_app).map_err(map_tx)?;
    progress.stop();
    println!("phase: {}", result.journal.phase);
    println!("action: {}", result.action.as_str());
    println!("target: {}", result.journal.target.real_path);
    println!(
        "target present: {}",
        Path::new(&result.journal.target.real_path).exists()
    );
    let original = root
        .join("transactions")
        .join(&result.journal.install_id)
        .join(&result.journal.paths.original);
    println!("backup intact: {}", original.exists());
    let staged = root
        .join("transactions")
        .join(&result.journal.install_id)
        .join(&result.journal.paths.staged);
    println!("staged removed: {}", !staged.exists());
    println!("outgoing restored: {}", result.action == Recovery::Rollback);
    let _ = result;
    Ok(())
}

pub(crate) fn restore_default_for_self_uninstall(progress: &mut Progress) -> Result<(), String> {
    uninstall_app(Path::new(DEFAULT_APP), &user_root(), progress).map(|_| ())
}

fn map_tx(err: TxError) -> String {
    match err {
        TxError::Refuse { message } | TxError::Other(message) => {
            if message.contains("No such file") || message.contains("not found") {
                message
            } else {
                message
            }
        }
    }
}

#[derive(Debug)]
struct CommandResult {
    skipped: bool,
    install_id: Option<String>,
    runtime_version: Option<String>,
    app: String,
    warning: Option<String>,
}

fn resolve_target(parsed: &ParsedCli, root: &Path) -> PathBuf {
    if let Some(app) = &parsed.app {
        return PathBuf::from(app);
    }
    if parsed.clone {
        return root.join("scratch").join("ChatGPT.app");
    }
    PathBuf::from(DEFAULT_APP)
}

fn print_install_plan(app: &Path, clone: bool) -> Result<(), String> {
    let source = if clone {
        PathBuf::from(DEFAULT_APP)
    } else {
        app.to_path_buf()
    };
    println!(
        "{}",
        format_step(if clone { "Clone install" } else { "Install" }, None)
    );
    println!(
        "{}",
        format_kv(
            "App",
            &if clone { app } else { &source }.display().to_string(),
            None
        )
    );
    if clone {
        println!(
            "{}",
            format_kv("Source", &source.display().to_string(), None)
        );
    }
    let plist = read_plist_info(&source);
    let version = match &plist {
        Some(info) if !info.app_version.is_empty() => {
            format!("{} {}", info.app_version, info.app_build)
                .trim()
                .to_string()
        }
        _ => "unknown".to_string(),
    };
    println!("{}", format_kv("Version", &version, None));
    println!(
        "{}",
        format_kv(
            "Signed",
            if verify_app(&source) { "yes" } else { "no" },
            None
        )
    );
    if !clone {
        println!(
            "{}",
            format_warn("Replaces the app in place and resigns it ad hoc.", None)
        );
        println!(
            "{}",
            format_warn(
                "Official Appshot (smart snapshot) stops until uninstall.",
                None
            )
        );
        println!("{}", format_kv("Backup", "~/.incodex/installations/", None));
    }
    Ok(())
}

fn ensure_confirmed(parsed: &ParsedCli, command: &str) -> Result<(), String> {
    if parsed.clone || parsed.dry_run || parsed.yes {
        return Ok(());
    }
    if crate::terminal::is_tty() {
        return if crate::confirm::ask_to_continue()? {
            Ok(())
        } else {
            Err("aborted".into())
        };
    }
    Err(format!(
        "non-interactive {command} requires --yes\n  incodex {command} --yes"
    ))
}

fn install_app(app: &Path, root: &Path, progress: &mut Progress) -> Result<CommandResult, String> {
    if !app.exists() {
        return Err(format!("Codex app not found: {}", app.display()));
    }
    let asar = app.join(ASAR_REL);
    let existing = inspect_existing_install(app, root, &asar)?;
    if existing.is_none() {
        ensure_official_target_is_verified(app)?;
    }
    progress.stage("Publishing Runtime");
    let published = publish(root)?;
    if let Some(install_id) = inspect_existing_install(app, root, &asar)? {
        return Ok(CommandResult {
            skipped: true,
            install_id: Some(install_id),
            runtime_version: Some(published.version),
            app: app.display().to_string(),
            warning: None,
        });
    }
    if let Some(archive) = Archive::open(&asar).ok() {
        let has_loader = archive.extract(LOADER_NAME).is_ok();
        if let Ok(package) = archive.read_package_main() {
            if has_loader || package.already_patched || package.install_id.is_some() {
                return Err(unbound_patch_error());
            }
        } else if has_loader {
            return Err(unbound_patch_error());
        }
    }
    let expected_plist = read_plist_info(app);
    let mut tx = begin_verified_transaction(root, app, |locked_app| {
        let locked_asar = locked_app.join(ASAR_REL);
        if inspect_existing_install(locked_app, root, &locked_asar)?.is_some() {
            return Err(
                "live app changed into an existing Incodex installation after preflight; refusing to snapshot it"
                    .into(),
            );
        }
        ensure_official_target_is_verified(locked_app)
    })?;
    let install_id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&install_id)
        .join("original")
        .join("ChatGPT.app");
    progress.stage("Backing up original app");
    snapshot_original(&mut tx, app, &original)?;
    let staged = root
        .join("scratch")
        .join(format!("ChatGPT.app.staged-{install_id}"));
    progress.stage("Patching and signing app");
    if let Err(error) = ditto(app, &staged) {
        return Err(rollback_install(&mut tx, Some(&staged), error));
    }
    let (hash, _) = match patch_asar(&staged.join(ASAR_REL), loader_source(), Some(&install_id)) {
        Ok(result) => result,
        Err(error) => return Err(rollback_install(&mut tx, Some(&staged), error)),
    };
    if let Err(error) = write_asar_integrity(&staged, &hash) {
        return Err(rollback_install(&mut tx, Some(&staged), error));
    }
    if is_official_app(app, None) || verify_app(app) || app.join("Contents/MacOS").exists() {
        if let Err(err) = sign_app(&staged) {
            return Err(rollback_install(&mut tx, Some(&staged), err));
        }
    }
    progress.stage("Replacing application");
    if let Err(error) = tx.place_staging(&staged) {
        return Err(rollback_install(&mut tx, Some(&staged), error));
    }
    if let Err(error) = tx.swap() {
        return Err(rollback_install(&mut tx, Some(&staged), error));
    }
    progress.stage("Verifying installation");
    if let Err(error) = verify_patched_adhoc_bundle_deep_strict(app, expected_plist.as_ref()) {
        let error = format!("post-swap codesign verification failed: {error}");
        return Err(rollback_install(&mut tx, Some(&staged), error));
    }
    let commit = match tx.commit() {
        Ok(result) => result,
        Err(error) => {
            return Err(rollback_install(&mut tx, Some(&staged), error));
        }
    };
    let warning = commit.cleanup_warning.map(|error| {
        format!(
            "Install committed, but transaction cleanup failed: {error}. Run `incodex recover --transaction {install_id}` to retry cleanup."
        )
    });
    let _ = notify_launch_services(app);
    Ok(CommandResult {
        skipped: false,
        install_id: Some(install_id),
        runtime_version: Some(runtime_version()),
        app: app.display().to_string(),
        warning,
    })
}

fn rollback_install(tx: &mut Engine, scratch: Option<&Path>, error: String) -> String {
    let rollback_error = match tx.journal().phase.as_str() {
        "COMMITTED" | "ROLLED_BACK" => None,
        _ => tx.rollback(&error).err(),
    };
    finish_rollback(tx, scratch, error, rollback_error)
}

fn finish_rollback(
    tx: &Engine,
    scratch: Option<&Path>,
    error: String,
    rollback_error: Option<String>,
) -> String {
    let _ = tx;
    let scratch_error = if rollback_error.is_none() {
        scratch.and_then(|path| remove_install_scratch(path).err())
    } else {
        None
    };
    let mut details = Vec::new();
    if let Some(rollback_error) = rollback_error {
        details.push(format!(
            "transaction rollback failed; recover the retained journal: {rollback_error}"
        ));
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

fn remove_install_scratch(path: &Path) -> Result<(), String> {
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

fn uninstall_app(
    app: &Path,
    root: &Path,
    progress: &mut Progress,
) -> Result<CommandResult, String> {
    if !app.exists() {
        return Err(format!("Codex app not found: {}", app.display()));
    }
    let official_target = is_official_app(app, None);
    if official_target {
        progress.stage("Closing ChatGPT");
        let _ = quit_official_app();
    }
    progress.stage("Locating verified backup");
    let journal = find_committed(root, app)?;
    progress.stage("Restoring original app");
    if journal.target.parent_device.is_empty() {
        let install_id = journal.install_id.clone();
        migrate_legacy_committed(root, &install_id, app, verify_app, |live| {
            verified_live_install_id(root, live).as_deref() == Some(install_id.as_str())
        })?;
    } else {
        restore_committed(root, &journal.install_id, app)?;
    }
    verify_restored_app(app, official_target)?;
    progress.stage("Refreshing Dock registration");
    let _ = notify_launch_services(app);
    Ok(CommandResult {
        skipped: false,
        install_id: Some(journal.install_id),
        runtime_version: Some(runtime_version()),
        app: app.display().to_string(),
        warning: None,
    })
}

fn verify_restored_app(app: &Path, official_target: bool) -> Result<(), String> {
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

fn current_install_id(app: &Path, root: &Path, archive: &Archive) -> Option<String> {
    if archive.extract("incodex-loader.cjs").ok()? != loader_source().as_bytes() {
        return None;
    }
    installed_install_id(app, root, archive)
}

fn begin_verified_transaction<F>(
    root: &Path,
    app: &Path,
    validate_locked_target: F,
) -> Result<Engine, String>
where
    F: FnOnce(&Path) -> Result<(), String>,
{
    let mut tx = Engine::begin(root, app, "install")?;
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

fn snapshot_original(tx: &mut Engine, app: &Path, original: &Path) -> Result<(), String> {
    if let Err(error) = ditto(app, original) {
        return Err(rollback_snapshot_failure(tx, error));
    }
    if let Err(error) = tx.mark_backup_committed() {
        return Err(rollback_snapshot_failure(tx, error));
    }
    Ok(())
}

fn rollback_snapshot_failure(tx: &mut Engine, error: String) -> String {
    let rollback = if tx.journal().phase == "DISCOVERED" {
        tx.abort_discovered_snapshot()
    } else {
        tx.rollback(&error)
    };
    match rollback {
        Ok(()) => error,
        Err(rollback) => format!(
            "{error}; failed to roll back rejected snapshot transaction: {rollback}"
        ),
    }
}

fn inspect_existing_install(
    app: &Path,
    root: &Path,
    asar: &Path,
) -> Result<Option<String>, String> {
    let Ok(archive) = Archive::open(asar) else {
        return Ok(None);
    };
    let has_loader = archive.extract(LOADER_NAME).is_ok();
    let package = match archive.read_package_main() {
        Ok(package) => package,
        Err(_) if has_loader => return Err(unbound_patch_error()),
        Err(_) => return Ok(None),
    };
    if !has_loader && !package.already_patched && package.install_id.is_none() {
        return Ok(None);
    }
    current_install_id(app, root, &archive)
        .map(Some)
        .ok_or_else(unbound_patch_error)
}

fn unbound_patch_error() -> String {
    "live app contains an Incodex marker or loader without a trusted committed installation record; refusing to create a new original snapshot".into()
}

fn ensure_official_target_is_verified(app: &Path) -> Result<(), String> {
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

fn ensure_official_bundle_identifier(info: &incodex_macos::PlistInfo) -> Result<(), String> {
    if info.bundle_identifier == OFFICIAL_BUNDLE_IDENTIFIER {
        return Ok(());
    }
    Err(format!(
        "default target bundle identifier is not {OFFICIAL_BUNDLE_IDENTIFIER}; refusing to snapshot a foreign bundle"
    ))
}

fn installed_install_id(app: &Path, root: &Path, archive: &Archive) -> Option<String> {
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
fn installed_marker_id(app: &Path, root: &Path, archive: &Archive) -> Option<String> {
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
    if target != journal_target || !verify_app(app) {
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

fn verified_live_install_id(root: &Path, app: &Path) -> Option<String> {
    Archive::open(&app.join(ASAR_REL))
        .ok()
        .and_then(|archive| installed_marker_id(app, root, &archive))
}

fn find_committed(root: &Path, app: &Path) -> Result<incodex_transaction::JournalV2, String> {
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

fn print_command_result(result: &CommandResult) {
    if result.skipped {
        println!(
            "{}",
            format_ok("Already current. Codex was not re-signed.", None)
        );
    }
    if let Some(id) = &result.install_id {
        println!("{}", format_kv("Install id", id, None));
    }
    if let Some(warning) = &result.warning {
        println!("{}", format_warn(warning, None));
    }
    if let Some(version) = &result.runtime_version {
        println!("{}", format_kv("Runtime", version, None));
    }
    println!("{}", format_kv("App", &result.app, None));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_path_foreign_bundle_is_rejected_before_snapshot() {
        let info = incodex_macos::PlistInfo {
            bundle_identifier: "com.example.foreign".into(),
            ..Default::default()
        };
        let error = ensure_official_bundle_identifier(&info).unwrap_err();
        assert!(error.contains("foreign bundle"), "{error}");
    }

    #[test]
    fn locked_target_validation_failure_rolls_back_before_original_snapshot() {
        let sandbox = std::env::temp_dir().join(format!(
            "incodex-locked-install-validation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = sandbox.join("state");
        let app = sandbox.join("ChatGPT.app");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("marker"), "replacement\n").unwrap();
        let canonical = fs::canonicalize(&app).unwrap();

        let result = begin_verified_transaction(&root, &app, |locked_target| {
            assert_eq!(locked_target, canonical);
            Err("locked target validation failed".to_string())
        });

        assert!(result.is_err());
        let transaction = fs::read_dir(root.join("transactions"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .file_name()
            .to_string_lossy()
            .into_owned();
        let journal = journal_v2(&root, &transaction).unwrap();
        assert_eq!(journal.phase, "ROLLED_BACK");
        assert!(!root
            .join("transactions")
            .join(transaction)
            .join("original/ChatGPT.app")
            .exists());
        fs::remove_dir_all(sandbox).unwrap();
    }

    #[test]
    fn target_replacement_after_locked_validation_does_not_leave_a_snapshot() {
        let sandbox = std::env::temp_dir().join(format!(
            "incodex-target-replacement-before-snapshot-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = sandbox.join("state");
        let app = sandbox.join("ChatGPT.app");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("marker"), "original\n").unwrap();

        let mut tx = begin_verified_transaction(&root, &app, |_locked_target| {
            let moved = sandbox.join("ChatGPT-updater-old.app");
            fs::rename(&app, &moved).unwrap();
            fs::create_dir_all(&app).unwrap();
            fs::write(app.join("marker"), "updater replacement\n").unwrap();
            Ok(())
        })
        .unwrap();
        let install_id = tx.install_id().to_string();
        let original = root
            .join("transactions")
            .join(&install_id)
            .join("original/ChatGPT.app");

        let error = snapshot_original(&mut tx, &app, &original).unwrap_err();

        assert!(error.contains("changed"), "{error}");
        assert_eq!(
            journal_v2(&root, &install_id).unwrap().phase,
            "ROLLED_BACK"
        );
        assert!(!original.exists());
        fs::remove_dir_all(sandbox).unwrap();
    }

    #[test]
    fn snapshot_failure_after_backup_commit_rolls_back_the_transaction() {
        let sandbox = std::env::temp_dir().join(format!(
            "incodex-snapshot-failure-after-backup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = sandbox.join("state");
        let app = sandbox.join("ChatGPT.app");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("marker"), "original\n").unwrap();

        let mut tx = Engine::begin(&root, &app, "snapshot-failure-test").unwrap();
        let install_id = tx.install_id().to_string();
        let original = root
            .join("transactions")
            .join(&install_id)
            .join("original/ChatGPT.app");
        fs::create_dir_all(&original).unwrap();
        fs::copy(app.join("marker"), original.join("marker")).unwrap();
        tx.mark_backup_committed().unwrap();

        // mark_backup_committed() may return after BACKUP_COMMITTED is durable.
        let error = rollback_snapshot_failure(&mut tx, "injected snapshot failure".into());

        assert!(error.contains("injected snapshot failure"), "{error}");
        assert_eq!(journal_v2(&root, &install_id).unwrap().phase, "ROLLED_BACK");
        assert!(original.exists());
        fs::remove_dir_all(sandbox).unwrap();
    }

    #[test]
    fn durable_rollback_error_cleans_scratch_without_claiming_recover_completed() {
        let sandbox = std::env::temp_dir().join(format!(
            "incodex-durable-rollback-error-{}",
            std::process::id()
        ));
        let root = sandbox.join("state");
        let app = sandbox.join("ChatGPT.app");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("marker"), "original\n").unwrap();

        let mut tx = Engine::begin(&root, &app, "durable-rollback-error-test").unwrap();
        tx.rollback("test rollback").unwrap();
        assert_eq!(tx.journal().phase, "ROLLED_BACK");
        let scratch = root
            .join("scratch")
            .join(format!("ChatGPT.app.staged-{}", tx.install_id()));
        fs::create_dir_all(&scratch).unwrap();

        let output = finish_rollback(
            &tx,
            Some(&scratch),
            "install failed".into(),
            Some("journal readback failed".into()),
        );

        assert!(!scratch.exists(), "durable rollback must still clean scratch");
        assert!(output.contains("rollback reached ROLLED_BACK"), "{output}");
        assert!(!output.contains("recover"), "{output}");
        fs::remove_dir_all(sandbox).unwrap();
    }
}
