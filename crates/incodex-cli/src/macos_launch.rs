/*
 * [INPUT]: 依赖官方包解析、LaunchServices进程证据、已验证Runtime与共享CDP动作。
 * [OUTPUT]: 提供可选launch命令；预览不写盘，正式启动不改包或重签。
 * [POS]: 新启动入口编排；install旧入口、原生open及会话实现保持独立原职责。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use crate::parse::ParsedCli;
use crate::CliFailure;
use incodex_core::{format_kv, format_ok, format_step, format_warn};
use std::path::PathBuf;

pub(crate) fn run_launch(parsed: &ParsedCli) -> Result<(), CliFailure> {
    let app = parsed
        .app
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(incodex_core::DEFAULT_APP));
    let binary = crate::app_bundle::resolve_executable(&app)?;
    let home = incodex_core::paths::home_dir();
    let icon = crate::macos_launch_icon::launcher_path(&home);
    if parsed.dry_run {
        println!("{}", format_step("Launch normal Codex through CDP", None));
        println!("{}", format_kv("App", &app.display().to_string(), None));
        println!(
            "{}",
            format_kv("Binary", &binary.display().to_string(), None)
        );
        println!(
            "{}",
            format_kv("Launcher", &icon.display().to_string(), None)
        );
        println!(
            "{}",
            format_warn(
                "Dry run. No window opened or files changed. Official signature is checked before launch; no patching or re-signing.",
                None
            )
        );
        return Ok(());
    }
    // 只接入原厂包，不让旧ASAR入口与新CDP适配竞争同一普通窗口。
    let signing = incodex_macos::inspect_outer_signing(&app)?;
    if !signing.verified
        || signing.kind != incodex_macos::SignatureKind::Vendor
        || signing.identifier.as_deref() != Some("com.openai.codex")
    {
        return Err(CliFailure::new("launch requires the unmodified official Codex app; the current install was not changed"));
    }
    let root = incodex_core::paths::user_root();
    if incodex_runtime_bundle::inspect_deployed(&root)?.is_none() {
        incodex_runtime_bundle::ensure_current(&root)?;
    }
    let mut runtime = crate::macos_launch_runtime::LaunchRuntime::new(&root)?;
    let port = crate::cdp::allocate_debug_port()?;
    let host = crate::macos_launch_process::launch_normal(&app, port)?;
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let cli = crate::macos_launch_cli::stable_entry(&current, &home)?;
    println!(
        "{}",
        format_step("Connecting normal Codex CDP actions", None)
    );
    println!("{}", format_kv("Host", &host.pid().to_string(), None));
    println!("{}", format_kv("Runtime", &runtime.active().release, None));
    crate::macos_launch_bridge::supervise(&host, port, &app, &cli, &mut runtime, || {
        let path = crate::macos_launch_icon::publish_launcher(&home, &cli, &app)?;
        println!(
            "{}",
            format_ok("Opened. Normal Codex CDP actions are ready.", None)
        );
        println!(
            "{}",
            format_kv("Launcher", &path.display().to_string(), None)
        );
        Ok(())
    })
    .map_err(CliFailure::from)
}
