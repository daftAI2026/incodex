use std::fs;
use std::path::{Path, PathBuf};

use incodex_asar::{patch_asar, Archive};
use incodex_core::canonical::{inspect_target, is_official_app};
use incodex_core::paths::{user_root, ASAR_REL, DEFAULT_APP};
use incodex_core::{format_kv, format_ok, format_step, format_warn};
use incodex_macos::{
    ditto, notify_launch_services, quit_official_app, read_asar_integrity, read_plist_info,
    restore_original, sign_app, verify_app, write_asar_integrity,
};
use incodex_runtime_bundle::{loader_source, publish, runtime_version};
use incodex_transaction::{
    acquire_target_lock, journal_v2, recover_with, Engine, Recovery, TxError,
};

use crate::parse::ParsedCli;

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
    if parsed.clone && parsed.app.is_none() {
        if !Path::new(DEFAULT_APP).exists() {
            return Err(format!("Codex app not found: {DEFAULT_APP}"));
        }
        ditto(Path::new(DEFAULT_APP), &app)?;
        println!("{}", format_ok("Cloned official app", None));
        println!("{}", format_kv("Target", &app.display().to_string(), None));
    }
    if parsed.live && parsed.app.is_none() {
        let _ = quit_official_app();
    }
    let result = install_app(&app, &root)?;
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
    let result = uninstall_app(&app, &root)?;
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
    let result = recover_with(&root, id, verify_app).map_err(map_tx)?;
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

pub(crate) fn restore_default_for_self_uninstall() -> Result<(), String> {
    uninstall_app(Path::new(DEFAULT_APP), &user_root()).map(|_| ())
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

fn install_app(app: &Path, root: &Path) -> Result<CommandResult, String> {
    if !app.exists() {
        return Err(format!("Codex app not found: {}", app.display()));
    }
    let published = publish(root)?;
    let asar = app.join(ASAR_REL);
    if asar.exists() {
        if let Ok(archive) = Archive::open(&asar) {
            if let Some(install_id) = current_install_id(app, root, &archive) {
                return Ok(CommandResult {
                    skipped: true,
                    install_id: Some(install_id),
                    runtime_version: Some(published.version),
                    app: app.display().to_string(),
                });
            }
        }
    }
    let mut tx = Engine::begin(root, app, "install")?;
    let install_id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&install_id)
        .join("original")
        .join("ChatGPT.app");
    ditto(app, &original)?;
    let staged = root
        .join("scratch")
        .join(format!("ChatGPT.app.staged-{install_id}"));
    ditto(app, &staged)?;
    let (hash, _) = patch_asar(&staged.join(ASAR_REL), loader_source(), Some(&install_id))?;
    write_asar_integrity(&staged, &hash)?;
    if is_official_app(app, None) || verify_app(app) || app.join("Contents/MacOS").exists() {
        if let Err(err) = sign_app(&staged) {
            if is_official_app(app, None) {
                tx.rollback(&err)?;
            }
            return Err(err);
        }
    }
    tx.place_staging(&staged)?;
    if let Err(error) = tx.swap() {
        if matches!(tx.journal().phase.as_str(), "TARGET_MOVED_OUT" | "SWAPPED") {
            let _ = tx.rollback(&error);
        }
        return Err(error);
    }
    if !verify_app(app) {
        let error = "post-swap codesign verification failed".to_string();
        tx.rollback(&error)?;
        return Err(error);
    }
    if let Err(error) = tx.commit() {
        tx.rollback(&error)?;
        return Err(error);
    }
    let _ = notify_launch_services(app);
    Ok(CommandResult {
        skipped: false,
        install_id: Some(install_id),
        runtime_version: Some(runtime_version()),
        app: app.display().to_string(),
    })
}

fn uninstall_app(app: &Path, root: &Path) -> Result<CommandResult, String> {
    if !app.exists() {
        return Err(format!("Codex app not found: {}", app.display()));
    }
    if is_official_app(app, None) {
        let _ = quit_official_app();
    }
    let _lock = acquire_target_lock(root, app, "uninstall", None)?;
    let journal = find_committed(root, app)?;
    let original = root
        .join("transactions")
        .join(&journal.install_id)
        .join(&journal.paths.original);
    restore_original(&original, app)?;
    let _ = notify_launch_services(app);
    Ok(CommandResult {
        skipped: false,
        install_id: Some(journal.install_id),
        runtime_version: Some(runtime_version()),
        app: app.display().to_string(),
    })
}

fn current_install_id(app: &Path, root: &Path, archive: &Archive) -> Option<String> {
    if !archive.has_only_loader()
        || archive.extract("incodex-loader.cjs").ok()? != loader_source().as_bytes()
    {
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

fn find_committed(root: &Path, app: &Path) -> Result<incodex_transaction::JournalV2, String> {
    let real = inspect_target(app, None)
        .map(|t| t.real_path)
        .unwrap_or_else(|_| app.to_path_buf());
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
    if let Some(version) = &result.runtime_version {
        println!("{}", format_kv("Runtime", version, None));
    }
    println!("{}", format_kv("App", &result.app, None));
}
