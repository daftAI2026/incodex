use std::path::{Path, PathBuf};
use std::process::Command;

use incodex_core::paths::{user_root, DEFAULT_APP};
use incodex_core::{format_kv, format_ok, format_step};

use crate::parse::ParsedCli;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallChannel {
    Source,
    Script,
    Homebrew,
}

pub fn run_runtime(parsed: &ParsedCli) -> Result<(), String> {
    if parsed.dry_run {
        println!("would update ~/.incodex/runtime/ without modifying Codex");
        return Ok(());
    }
    let published = incodex_runtime_bundle::publish(&user_root())?;
    println!("{}", format_step("Runtime", None));
    println!(
        "{}",
        format_ok(
            "Runtime updated. Codex was not modified. Reopen it to load the new logic.",
            None,
        )
    );
    println!("{}", format_kv("Runtime", &published.version, None));
    Ok(())
}

pub fn run_update(parsed: &ParsedCli) -> Result<(), String> {
    let exe = current_exe()?;
    match install_channel(&exe) {
        InstallChannel::Source => {
            return Err(
                "this copy is running from source\n  git pull && bun install --frozen-lockfile && bun link"
                    .into(),
            )
        }
        InstallChannel::Homebrew => {
            return Err("this copy was installed with Homebrew\n  brew upgrade incodex".into())
        }
        InstallChannel::Script => {}
    }
    let prefix = install_prefix(&exe);
    println!("update channel: script");
    println!("  prefix: {}", prefix.display());
    if parsed.dry_run {
        println!("would re-run install.sh for this prefix");
        println!("no changes made.");
        return Ok(());
    }
    let status = Command::new("bash")
        .args([
            "-lc",
            "curl -fsSL https://raw.githubusercontent.com/daftAI2026/incodex/main/install.sh | bash",
        ])
        .env("INCODEX_PREFIX", &prefix)
        .status()
        .map_err(|err| err.to_string())?;
    status.success().then_some(()).ok_or("update failed".into())
}

pub fn run_self_uninstall(parsed: &ParsedCli) -> Result<(), String> {
    let exe = current_exe()?;
    match install_channel(&exe) {
        InstallChannel::Source => {
            return Err("this copy is running from source\n  bun unlink".into())
        }
        InstallChannel::Homebrew => {
            return Err("this copy was installed with Homebrew\n  brew uninstall incodex".into())
        }
        InstallChannel::Script => {}
    }
    let paths = self_uninstall_paths(&exe);
    println!("remove:");
    for path in &paths {
        println!("  {}", path.display());
    }
    if parsed.restore_app {
        println!("also restore: {DEFAULT_APP}");
    }
    if parsed.dry_run {
        println!("no changes made.");
        return Ok(());
    }
    ensure_confirmed(parsed, "self-uninstall")?;
    if parsed.restore_app {
        crate::install::restore_default_for_self_uninstall()?;
        println!("restored {DEFAULT_APP}");
    }
    for path in paths {
        if path.exists() {
            std::fs::remove_file(path).map_err(|err| err.to_string())?;
        }
    }
    println!("done");
    Ok(())
}

fn current_exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|err| err.to_string())
}

fn install_channel(exe: &Path) -> InstallChannel {
    let text = exe.to_string_lossy();
    if text.contains("/Cellar/incodex/")
        || text.contains("/opt/homebrew/opt/incodex/")
        || text.ends_with("/opt/homebrew/bin/incodex")
        || text.ends_with("/opt/homebrew/bin/inc")
    {
        return InstallChannel::Homebrew;
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if exe.starts_with(repo) || text.contains("/target/debug/") || text.contains("/target/release/")
    {
        return InstallChannel::Source;
    }
    InstallChannel::Script
}

fn install_prefix(exe: &Path) -> PathBuf {
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    if dir.file_name().is_some_and(|name| name == "bin") {
        dir.parent().unwrap_or(dir).to_path_buf()
    } else {
        dir.to_path_buf()
    }
}

fn self_uninstall_paths(exe: &Path) -> [PathBuf; 2] {
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    [dir.join("incodex"), dir.join("inc")]
}

fn ensure_confirmed(parsed: &ParsedCli, command: &str) -> Result<(), String> {
    if parsed.yes {
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
