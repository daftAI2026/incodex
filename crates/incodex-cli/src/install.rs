/*
 * [INPUT]: 依赖事务引擎的备份/回滚、macOS 签名与外部 Runtime 发布，并复用实验的 Keychain/更新登记。
 * [OUTPUT]: 提供 install/uninstall/accessibility 路径及按更新代恢复安装的入口。
 * [POS]: 原生 CLI 的危险变更编排器；仅在 quiescence 与代际证明成立时交给底层事务。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::fs;
use std::path::{Path, PathBuf};

use incodex_asar::{patch_asar, Archive, LOADER_NAME};
use incodex_core::canonical::{inspect_target, is_official_app};
use incodex_core::paths::{user_root, ASAR_REL, DEFAULT_APP};
use incodex_core::{format_kv, format_ok, format_step, format_warn};
#[cfg(test)]
use incodex_macos::AppQuiescence;
use incodex_macos::{
    ditto, notify_launch_services, read_asar_integrity, read_plist_info, verify_app,
    verify_original_vendor_bundle, write_asar_integrity, OFFICIAL_BUNDLE_IDENTIFIER,
};
use incodex_runtime_bundle::{ensure_current, loader_source, runtime_version};
#[cfg(test)]
use incodex_transaction::NoopQuiescenceGuard;
use incodex_transaction::{
    finalize_restored_transaction, journal_for_recovery, journal_v2,
    migrate_legacy_committed_with_quiescence, prune_superseded_terminal, recover_with_quiescence,
    restore_committed_with_quiescence, terminal_cleanup_pending, validate_backup_snapshot,
    validate_committed_live_snapshot, Engine, QuiescenceGuard, Recovery, TxError,
};
use sha2::{Digest, Sha256};

use crate::app_quiescence::AppGuard;
use crate::parse::ParsedCli;
use crate::spinner::Progress;

// 仅放行已验证能加载本代 external Runtime 协议的已发布 Loader。
const COMPATIBLE_HISTORICAL_LOADER_SHA256: &[[u8; 32]] = &[
    [
        0x6d, 0x03, 0x7a, 0x91, 0xc1, 0xec, 0x7a, 0x1f, 0xce, 0x92, 0x80, 0xfd, 0xfd, 0xba, 0x78,
        0x71, 0x6f, 0x99, 0xc6, 0xf9, 0x68, 0x32, 0x26, 0x18, 0x0f, 0x02, 0x4f, 0x2f, 0x9f, 0x97,
        0x3c, 0x23,
    ],
    [
        0x07, 0x1f, 0x0b, 0xf5, 0x52, 0xb1, 0x28, 0x9e, 0xec, 0x93, 0x25, 0x0e, 0x23, 0xdc, 0x11,
        0x62, 0x22, 0x52, 0x54, 0xb8, 0xd1, 0xd2, 0xd5, 0x8d, 0xe6, 0xc7, 0x5c, 0xb9, 0x60, 0xcb,
        0x0f, 0x52,
    ],
    [
        0x70, 0x90, 0xd7, 0x47, 0xb8, 0xcf, 0x21, 0x06, 0x2d, 0x42, 0x67, 0xa1, 0x8e, 0xff, 0x6d,
        0x92, 0x45, 0x88, 0xd7, 0xb1, 0x10, 0x35, 0xba, 0xf9, 0xd7, 0x42, 0x0f, 0x44, 0x06, 0x19,
        0xb3, 0x4b,
    ],
];

#[cfg(test)]
fn close_official_app_with<P, Q, C>(
    app: &AppQuiescence,
    probe: &P,
    requester: &mut Q,
    clock: &mut C,
) -> Result<(), String>
where
    P: incodex_macos::ProcessProbe,
    Q: incodex_macos::QuitRequester,
    C: incodex_macos::QuiescenceClock,
{
    app.quit_official_app_and_wait_with(probe, requester, clock)
}

pub fn run_install(parsed: &ParsedCli) -> Result<(), String> {
    let root = user_root();
    let app = resolve_target(parsed, &root);
    if parsed.clone && parsed.app.is_none() {
        println!("{}", format_kv("Clone", &app.display().to_string(), None));
    }
    let mut progress = Progress::new();
    print_install_plan(
        &app,
        parsed.clone,
        parsed.live && parsed.app.is_none(),
        &mut progress,
    )?;
    if parsed.dry_run {
        println!("{}", format_warn("Dry run. No files changed.", None));
        return Ok(());
    }
    if !parsed.clone {
        crate::confirm::require("install", parsed.yes)?;
    }
    incodex_transaction::validate_storage_root(&root)?;
    let official_default = is_official_app(&app, None);
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
    if !app.exists() {
        return Err(format!("Codex app not found: {}", app.display()));
    }
    let guard = AppGuard::for_app(&app)?;
    if official_default {
        progress.stage("Closing ChatGPT");
        guard.close_official()?;
    } else {
        guard.ensure()?;
    }
    let mut result = install_app_with_quiescence(&app, &root, &mut progress, guard)?;
    if official_default {
        let registration = std::env::current_exe()
            .map_err(|error| format!("cannot locate the active Incodex CLI: {error}"))
            .and_then(|helper| register_update_restore(&root, &app, &helper, &result));
        if let Err(error) = registration {
            append_warning(
                &mut result,
                format!("Automatic recovery after a Codex update is unavailable: {error}"),
            );
        }
    }
    progress.stop();
    print_command_result(&result);
    if parsed.live && parsed.app.is_none() {
        let install_id = result.install_id.as_deref().ok_or(
            "App installation finished, but its install identity is missing; permission setup was not started.",
        )?;
        let _permission_lock = incodex_transaction::acquire_target_lock(
            &root,
            &app,
            "install-accessibility",
            Some(install_id),
        )?;
        let request_id = crate::accessibility_setup::request_setup(&root, &app, install_id).map_err(|error| {
            format!("App installation finished, but permission setup could not be prepared: {error}. Run incodex install to retry.")
        })?;
        println!(
            "{}",
            format_kv(
                "Accessibility",
                "Checking ChatGPT access; the shared native guide opens only if needed.",
                None
            )
        );
        let permission = crate::accessibility_guide_host::run_permission_guide(
            &root,
            &app,
            crate::accessibility_guide_host::GuideCopyContext::Installed,
            || {
                validate_committed_live_snapshot(&root, install_id, &app)
                    .map_err(|error| error.to_string())?;
                crate::macos_signing::verify_patched(&root, &app, None).map(|_| ())
            },
        );
        let state = match &permission {
            Ok(crate::accessibility_guide_host::Outcome::Granted) => "granted",
            Ok(crate::accessibility_guide_host::Outcome::Pending) => "deferred",
            Err(_) => "error",
        };
        if let Err(error) = crate::accessibility_setup::finish_cli_setup(
            &root,
            &app,
            install_id,
            &request_id,
            state,
        ) {
            println!("{}", format_warn(&format!("Installation is complete, but the permission result could not be recorded: {error}"), None));
        }
        match permission {
            Ok(crate::accessibility_guide_host::Outcome::Granted) => println!("{}", format_ok("Installed. ChatGPT Accessibility access verified.", None)),
            Ok(crate::accessibility_guide_host::Outcome::Pending) => println!("{}", format_warn("Installed. Accessibility setup is unfinished; run `incodex accessibility` when ready. `incodex doctor` only checks access.", None)),
            Err(error) => println!("{}", format_warn(&format!("Installed, but Accessibility setup could not finish: {error}. Run `incodex accessibility` to retry."), None)),
        }
    } else {
        println!(
            "{}",
            format_ok("Restart that app copy to see the Incognito button.", None)
        );
    }
    crate::install_keychain_advice::print_if_applicable(
        &app,
        !result.skipped,
        parsed.app.is_some(),
    );
    println!();
    Ok(())
}

/// User-initiated re-entry after Skip, close, or an interrupted guide. This
/// never repeats the install/uninstall transaction; both identities use the
/// same verified native guide and only an explicit Allow may reset TCC.
pub fn run_accessibility(parsed: &ParsedCli) -> Result<(), String> {
    if parsed.app.is_some()
        || parsed.clone
        || parsed.dry_run
        || parsed.json
        || parsed.transaction.is_some()
        || parsed.restore_app
    {
        return Err(
            "accessibility only supports the default /Applications/ChatGPT.app without other flags"
                .into(),
        );
    }
    let root = user_root();
    let app = Path::new(DEFAULT_APP);
    let _lock =
        incodex_transaction::acquire_target_lock(&root, app, "accessibility-reentry", None)?;
    let installed = inspect_existing_install(app, &root, &app.join(ASAR_REL))?;
    if let Some(install_id) = installed {
        let request_id = crate::accessibility_setup::request_setup(&root, app, &install_id)?;
        let outcome = crate::accessibility_guide_host::run_permission_guide(
            &root,
            app,
            crate::accessibility_guide_host::GuideCopyContext::Installed,
            || {
                validate_committed_live_snapshot(&root, &install_id, app)
                    .map_err(|error| error.to_string())?;
                crate::macos_signing::verify_patched(&root, app, None).map(|_| ())
            },
        );
        let state = match &outcome {
            Ok(crate::accessibility_guide_host::Outcome::Granted) => "granted",
            Ok(crate::accessibility_guide_host::Outcome::Pending) => "deferred",
            Err(_) => "error",
        };
        crate::accessibility_setup::finish_cli_setup(&root, app, &install_id, &request_id, state)?;
        print_accessibility_reentry(outcome, "installed")
    } else {
        verify_original_vendor_bundle(app, Some(OFFICIAL_BUNDLE_IDENTIFIER), None, None)
            .map(|_| ())?;
        let outcome = crate::accessibility_guide_host::run_permission_guide(
            &root,
            app,
            crate::accessibility_guide_host::GuideCopyContext::Official,
            || {
                verify_original_vendor_bundle(app, Some(OFFICIAL_BUNDLE_IDENTIFIER), None, None)
                    .map(|_| ())
            },
        );
        print_accessibility_reentry(outcome, "official")
    }
}

fn print_accessibility_reentry(
    outcome: Result<crate::accessibility_guide_host::Outcome, String>,
    identity: &str,
) -> Result<(), String> {
    match outcome {
        Ok(crate::accessibility_guide_host::Outcome::Granted) => {
            println!("{}", format_ok(&format!("{identity} ChatGPT Accessibility access verified."), None));
            Ok(())
        }
        Ok(crate::accessibility_guide_host::Outcome::Pending) => {
            println!("{}", format_warn("Accessibility setup is unfinished. Run `incodex accessibility` when you want to continue; `incodex doctor` only checks the result.", None));
            Ok(())
        }
        Err(error) => Err(format!("Accessibility setup could not finish: {error}. Run `incodex accessibility` to retry when ready.")),
    }
}

pub fn run_uninstall(parsed: &ParsedCli) -> Result<(), String> {
    let root = user_root();
    let app = resolve_target(parsed, &root);
    println!("{}", format_step("Uninstall", None));
    println!("{}", format_kv("App", &app.display().to_string(), None));
    let renew_official_access =
        parsed.app.is_none() && !parsed.clone && is_official_app(&app, None);
    if renew_official_access {
        println!("{}", format_kv("Accessibility", "Reopens the restored official app and checks access. If invalid, opens the shared native guide; only Allow resets ChatGPT's Accessibility registration and opens System Settings.", None));
    }
    if parsed.dry_run {
        println!("{}", format_warn("Dry run. No files changed.", None));
        return Ok(());
    }
    if !parsed.clone {
        crate::confirm::require("uninstall", parsed.yes)?;
    }
    incodex_transaction::validate_storage_root(&root)?;
    if !app.exists() {
        return Err(format!("Codex app not found: {}", app.display()));
    }
    let mut progress = Progress::new();
    let official_default = is_official_app(&app, None);
    let guard = AppGuard::for_app(&app)?;
    if official_default {
        cancel_update_restore(&root, &app)?;
        progress.stage("Closing ChatGPT");
        guard.close_official()?;
    } else {
        guard.ensure()?;
    }
    uninstall_app_with_quiescence(&app, &root, &mut progress, guard, official_default)?;
    progress.stop();
    let app_name = app
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("App");
    println!(
        "{}",
        format_ok(&format!("Uninstalled. {app_name} restored."), None)
    );
    if renew_official_access {
        crate::accessibility_restore::finish_uninstall(&root, &app);
    }
    println!();
    Ok(())
}

pub fn run_recover(parsed: &ParsedCli) -> Result<(), String> {
    let root = user_root();
    let id = parsed
        .transaction
        .as_deref()
        .ok_or("recover requires --transaction <id>\n  incodex recover --transaction <id>")?;
    incodex_transaction::validate_storage_root(&root)?;
    let v2 = root.join("transactions").join(id).join("journal.json");
    let v1 = root.join("transactions").join(format!("{id}.json"));
    let cleanup_pending = terminal_cleanup_pending(&root, id);
    if !v2.exists() && !v1.exists() && !cleanup_pending {
        return Err(format!("no journal for {id}"));
    }
    if !v2.exists() && !cleanup_pending {
        return Err(format!(
            "legacy transaction {id} is not supported by native recover; no files changed"
        ));
    }
    if parsed.dry_run {
        return Err("recover --dry-run is not supported; no files changed".into());
    }
    let journal = journal_for_recovery(&root, id)?;
    let terminal_cleanup = matches!(journal.phase.as_str(), "COMMITTED" | "ROLLED_BACK");
    let target = PathBuf::from(&journal.target.real_path);
    let guard = if terminal_cleanup {
        AppGuard::noop()
    } else {
        let guard = if target.exists() {
            AppGuard::for_app(&target)?
        } else {
            let original = root
                .join("transactions")
                .join(id)
                .join(&journal.paths.original);
            AppGuard::for_bundle_at(&original, &target)?
        };
        if is_official_app(&target, None) {
            guard.close_official()?;
        } else {
            guard.ensure()?;
        }
        guard
    };
    let mut progress = Progress::new();
    progress.stage("Recovering transaction");
    let result = recover_with_quiescence(&root, id, guard, |target| {
        crate::macos_signing::verify_for_root(&root, target)
    })
    .map_err(map_tx)?;
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
    let root = user_root();
    incodex_transaction::validate_storage_root(&root)?;
    let app = Path::new(DEFAULT_APP);
    let guard = AppGuard::for_app(app)?;
    progress.stage("Closing ChatGPT");
    guard.close_official()?;
    uninstall_app_with_quiescence(app, &root, progress, guard, true)
}

fn map_tx(err: TxError) -> String {
    match err {
        TxError::Refuse { message } | TxError::Other(message) => message,
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

fn register_update_restore(
    root: &Path,
    app: &Path,
    helper_source: &Path,
    result: &CommandResult,
) -> Result<(), String> {
    let install_id = result
        .install_id
        .as_deref()
        .ok_or("installed app has no install epoch for update recovery")?;
    crate::macos_update_restore::publish_registration(root, helper_source, app, install_id)?;
    Ok(())
}

fn register_update_restore_if_generation(
    root: &Path,
    app: &Path,
    helper_source: &Path,
    result: &CommandResult,
    expected_install_id: &str,
    expected_helper_sha256: &str,
    expected_signing_certificate_sha256: Option<&str>,
) -> Result<(), String> {
    let install_id = result
        .install_id
        .as_deref()
        .ok_or("restored app has no install epoch for generation commit")?;
    crate::macos_update_restore::publish_registration_if_signing_generation(
        root,
        helper_source,
        app,
        install_id,
        expected_install_id,
        expected_helper_sha256,
        expected_signing_certificate_sha256,
    )?;
    Ok(())
}

fn cancel_update_restore(root: &Path, app: &Path) -> Result<(), String> {
    let Some(registration) = crate::macos_update_restore::read_registration(root)? else {
        return Ok(());
    };
    if registration.app_path != app {
        return Ok(());
    }
    crate::macos_update_restore::remove_registration(root, &registration.install_id)
}

fn append_warning(result: &mut CommandResult, warning: String) {
    result.warning = Some(match result.warning.take() {
        Some(existing) => format!("{existing} {warning}"),
        None => warning,
    });
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

fn print_install_plan(
    app: &Path,
    clone: bool,
    setup_accessibility: bool,
    progress: &mut Progress,
) -> Result<(), String> {
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
    progress.stage("Checking app signature");
    let signed = verify_app(&source);
    progress.stop();
    println!(
        "{}",
        format_kv("Signed", if signed { "yes" } else { "no" }, None)
    );
    if !clone {
        println!(
            "{}",
            format_warn(
                if is_official_app(app, None) {
                    "New installs re-sign the app with a reusable local certificate. Verified existing installs skip re-signing; migrating an ad-hoc install requires uninstall, then install."
                } else {
                    "Replaces the app in place and resigns it ad hoc."
                },
                None,
            )
        );
        if setup_accessibility {
            println!("{}", format_kv("Accessibility", "Reopens ChatGPT for permission setup after installation. macOS may require your approval.", None));
        }
        println!(
            "{}",
            format_warn(
                "Official Appshot (smart snapshot) stops until uninstall.",
                None
            )
        );
        println!(
            "{}",
            format_kv(
                "Backup",
                "~/.incodex/transactions/<install-id>/original/ChatGPT.app",
                None,
            )
        );
    }
    Ok(())
}

#[path = "install_mutation.rs"]
mod mutation;
#[cfg(test)]
use mutation::finish_rollback;
pub(crate) use mutation::reinstall_after_official_update;
use mutation::{install_app_with_quiescence, uninstall_app_with_quiescence};

#[path = "install_proof.rs"]
mod proof;
use proof::*;

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
#[path = "install_tests.rs"]
mod tests;
