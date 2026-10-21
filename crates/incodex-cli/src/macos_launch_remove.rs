/**
 * [INPUT]: 依赖解析后的 launch --remove 参数、Finder 入口库存与统一确认提示。
 * [OUTPUT]: 预览并删除经过marker、plist、脚本和图标校验的自有薄入口。
 * [POS]: Launch 移除子命令；不解析宿主、不退出宿主、不触碰 Runtime 或会话。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use crate::macos_launch_icon::{
    inspect_launchers, remove_confirmed_launchers, unknown_launcher_error,
};
use crate::parse::ParsedCli;
use incodex_core::{format_kv, format_step, format_warn};
use std::path::PathBuf;

pub(crate) struct SelfUninstallLauncherPlan {
    home: PathBuf,
    owned: Vec<PathBuf>,
    unknown: Vec<PathBuf>,
}

pub(crate) fn print_self_uninstall_plan() -> SelfUninstallLauncherPlan {
    let home = incodex_core::paths::home_dir();
    let inventory = inspect_launchers(&home);
    for path in &inventory.owned {
        println!("  {}", path.display());
    }
    for (path, reason) in &inventory.unknown {
        println!(
            "{}",
            format_warn(
                &format!("Leaving unrecognized launcher {}: {reason}", path.display()),
                None
            )
        );
    }
    SelfUninstallLauncherPlan {
        home,
        owned: inventory.owned,
        unknown: inventory
            .unknown
            .into_iter()
            .map(|(path, _)| path)
            .collect(),
    }
}

pub(crate) fn remove_self_uninstall_entries(
    plan: &SelfUninstallLauncherPlan,
) -> Result<(), String> {
    if plan.owned.is_empty() {
        return Ok(());
    }
    let removal = remove_confirmed_launchers(&plan.home, &plan.owned, false)?;
    for path in removal.removed {
        println!("removed {}", path.display());
    }
    for (path, reason) in removal.warnings {
        if !plan.unknown.contains(&path) {
            println!(
                "{}",
                format_warn(
                    &format!("Leaving unrecognized launcher {}: {reason}", path.display()),
                    None
                )
            );
        }
    }
    Ok(())
}

pub(crate) fn run_remove(parsed: &ParsedCli) -> Result<(), String> {
    let home = incodex_core::paths::home_dir();
    let inventory = inspect_launchers(&home);
    println!("{}", format_step("Remove Incodex Finder launcher", None));
    for path in &inventory.owned {
        println!(
            "{}",
            format_kv("Owned entry", &path.display().to_string(), None)
        );
    }
    for (path, reason) in &inventory.unknown {
        println!(
            "{}",
            format_warn(
                &format!("Leaving unrecognized entry {}: {reason}", path.display()),
                None
            )
        );
    }

    if !inventory.unknown.is_empty() {
        return Err(unknown_launcher_error(&inventory.unknown));
    }
    if inventory.owned.is_empty() {
        println!("No Incodex Finder launcher entry exists; no changes made.");
        return Ok(());
    }
    if parsed.dry_run {
        println!(
            "{}",
            format_warn(
                "Dry run; no changes made. Use launch --remove --yes to remove the listed entries.",
                None
            )
        );
        return Ok(());
    }

    crate::confirm::require("launch --remove", parsed.yes)?;
    let removal = remove_confirmed_launchers(&home, &inventory.owned, true)?;
    for path in &removal.removed {
        println!(
            "{}",
            format_kv("Removed", &path.display().to_string(), None)
        );
    }
    for (path, reason) in removal.warnings {
        println!(
            "{}",
            format_warn(
                &format!("Leaving unrecognized entry {}: {reason}", path.display()),
                None
            )
        );
    }
    println!("Launcher removal complete. The host and its controller were left running.");
    Ok(())
}
