use incodex_core::{format_kv, format_step, format_warn};

use crate::parse::ParsedCli;
use crate::windows_app::{discover_codex_package, WindowsCodexApp};

pub fn run_install(parsed: &ParsedCli) -> Result<(), String> {
    let app = discover_codex_package()?;
    print_plan("Install", &app);
    if parsed.dry_run {
        println!("{}", format_warn("Dry run. No files changed.", None));
        println!();
        return Ok(());
    }
    crate::confirm::require("install", parsed.yes)?;
    Err("Windows install is not implemented yet; no files changed".to_string())
}

pub fn run_uninstall(parsed: &ParsedCli) -> Result<(), String> {
    let app = discover_codex_package()?;
    print_plan("Uninstall", &app);
    if parsed.dry_run {
        println!("{}", format_warn("Dry run. No files changed.", None));
        println!();
        return Ok(());
    }
    crate::confirm::require("uninstall", parsed.yes)?;
    Err("Windows uninstall is not implemented yet; no files changed".to_string())
}

fn print_plan(action: &str, app: &WindowsCodexApp) {
    println!("{}", format_step(action, None));
    println!("{}", format_kv("Package", &app.package_full_name, None));
    println!(
        "{}",
        format_kv("App", &app.executable.display().to_string(), None)
    );
    println!(
        "{}",
        format_warn("The Microsoft Store package is not modified.", None)
    );
}
