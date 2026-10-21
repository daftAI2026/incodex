/*
 * [INPUT]: 依赖启动/移除参数、安装记录、标准profile owner与共享CDP动作。
 * [OUTPUT]: 先移除或发布自有入口；复用活profile宿主并保留有界失败回执。
 * [POS]: macOS薄启动入口；移除不解析宿主，启动不新建controller或覆盖既有IPC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use crate::macos_launch_log::{record, LaunchStage};
use crate::parse::ParsedCli;
use crate::CliFailure;
use incodex_core::{format_kv, format_ok, format_step, format_warn};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, PartialEq, Eq)]
enum LaunchRoute {
    Installed,
    Cdp,
}

fn select_launch_route(
    installed: Result<bool, String>,
    is_vendor: impl FnOnce() -> Result<bool, String>,
) -> Result<LaunchRoute, String> {
    if installed? {
        return Ok(LaunchRoute::Installed);
    }
    if is_vendor()? {
        return Ok(LaunchRoute::Cdp);
    }
    Err("launch requires official Codex or a recorded Incodex install; the current app was not changed".into())
}

fn installed_launch_command(app: &Path) -> Command {
    let mut command = Command::new("/usr/bin/open");
    command.arg("-a").arg(app);
    command
}

fn publish_before_start<T>(
    publish: impl FnOnce() -> Result<PathBuf, String>,
    start: impl FnOnce() -> Result<T, String>,
) -> Result<(PathBuf, T), String> {
    let launcher = publish()?;
    let started = start()?;
    Ok((launcher, started))
}

pub(crate) fn run_launch(parsed: &ParsedCli) -> Result<(), CliFailure> {
    if parsed.remove_launcher {
        return crate::macos_launch_remove::run_remove(parsed).map_err(CliFailure::from);
    }
    let app = selected_app(parsed);
    let root = incodex_core::paths::user_root();
    if !parsed.dry_run {
        record(&root, &app, LaunchStage::Requested, None);
    }
    let result = run_launch_inner(parsed);
    if !parsed.dry_run {
        if let Err(error) = &result {
            record(&root, &app, LaunchStage::Failed, Some(error.message()));
        }
    }
    result
}

fn selected_app(parsed: &ParsedCli) -> PathBuf {
    parsed
        .app
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(incodex_core::DEFAULT_APP))
}

fn run_launch_inner(parsed: &ParsedCli) -> Result<(), CliFailure> {
    let app = selected_app(parsed);
    let binary = crate::app_bundle::resolve_executable(&app)?;
    let home = incodex_core::paths::home_dir();
    let icon = crate::macos_launch_icon::launcher_path(&home);
    if parsed.dry_run {
        println!("{}", format_step("Launch normal Codex", None));
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
                "Dry run. No window opened or files changed. Reuses a recorded Incodex install; otherwise checks the official signature before CDP launch. No patching or re-signing.",
                None
            )
        );
        return Ok(());
    }
    let root = incodex_core::paths::user_root();
    let route = select_launch_route(crate::install::has_recorded_install(&root, &app), || {
        let signing = incodex_macos::inspect_outer_signing(&app)?;
        Ok(signing.verified
            && signing.kind == incodex_macos::SignatureKind::Vendor
            && signing.identifier.as_deref() == Some("com.openai.codex"))
    })?;
    // Chromium按普通profile而非App包路径转交启动。只唤回已有确切owner，目标路径不变。
    let profile = home.join("Library/Application Support/Codex");
    if let Some((owned_app, host)) =
        crate::macos_launch_owner::active_profile_host(&root, &profile)?
    {
        let current = std::env::current_exe().map_err(|error| error.to_string())?;
        let cli = crate::macos_launch_cli::stable_entry(&current, &home)?;
        let (path, ()) = publish_before_start(
            || crate::macos_launch_icon::publish_launcher(&home, &cli, &app),
            || host.activate(&owned_app),
        )?;
        record(&root, &app, LaunchStage::Ready, None);
        println!(
            "{}",
            format_ok(
                "Activated the existing Codex window that owns the normal profile.",
                None
            )
        );
        println!(
            "{}",
            format_kv("Active app", &owned_app.display().to_string(), None)
        );
        println!(
            "{}",
            format_kv("Launcher", &path.display().to_string(), None)
        );
        return Ok(());
    }
    if route == LaunchRoute::Installed {
        // 已安装main独占会话、IPC及热更新；薄入口不叠加CDP controller。
        let current = std::env::current_exe().map_err(|e| e.to_string())?;
        let cli = crate::macos_launch_cli::stable_entry(&current, &home)?;
        let (path, output) = publish_before_start(
            || crate::macos_launch_icon::publish_launcher(&home, &cli, &app),
            || {
                installed_launch_command(&app)
                    .output()
                    .map_err(|e| format!("cannot request installed Codex launch: {e}"))
            },
        )?;
        if !output.status.success() {
            return Err(CliFailure::new(format!(
                "LaunchServices could not open installed Codex: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        record(&root, &app, LaunchStage::InstalledRequested, None);
        println!(
            "{}",
            format_ok(
                "Requested launch of installed Codex; reusing its existing Runtime.",
                None
            )
        );
        println!(
            "{}",
            format_kv("Launcher", &path.display().to_string(), None)
        );
        return Ok(());
    }
    let mut owner = match crate::macos_launch_owner::claim(&root, &app)? {
        crate::macos_launch_owner::LaunchClaim::Existing(host) => {
            let current = std::env::current_exe().map_err(|e| e.to_string())?;
            let cli = crate::macos_launch_cli::stable_entry(&current, &home)?;
            let (_, ()) = publish_before_start(
                || crate::macos_launch_icon::publish_launcher(&home, &cli, &app),
                || host.activate(&app),
            )?;
            record(&root, &app, LaunchStage::Ready, None);
            println!(
                "{}",
                format_ok(
                    "Requested activation of the existing Codex launcher window.",
                    None
                )
            );
            return Ok(());
        }
        crate::macos_launch_owner::LaunchClaim::Primary(owner) => owner,
    };
    if incodex_runtime_bundle::inspect_deployed(&root)?.is_none() {
        incodex_runtime_bundle::ensure_current(&root)?;
    }
    let mut runtime = crate::macos_launch_runtime::LaunchRuntime::new(&root)?;
    let port = crate::cdp::allocate_debug_port()?;
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let cli = crate::macos_launch_cli::stable_entry(&current, &home)?;
    let (path, host) = publish_before_start(
        || crate::macos_launch_icon::publish_launcher(&home, &cli, &app),
        || crate::macos_launch_process::launch_normal(&app, port),
    )?;
    owner.publish(&host)?;
    record(&root, &app, LaunchStage::CdpRequested, None);
    println!(
        "{}",
        format_step("Connecting normal Codex CDP actions", None)
    );
    println!("{}", format_kv("Host", &host.pid().to_string(), None));
    println!("{}", format_kv("Runtime", &runtime.active().release, None));
    crate::macos_launch_bridge::supervise(&host, port, &app, &cli, &mut runtime, || {
        record(&root, &app, LaunchStage::Ready, None);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_launch_recognition_does_not_require_signing_or_a_restore_snapshot() {
        use std::fs;
        struct Scratch(PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let scratch = Scratch(std::env::temp_dir().join(format!(
            "incodex-launch-record-{}",
            incodex_transaction::new_install_id()
        )));
        let root = scratch.0.join("state");
        let app = scratch.0.join("Host.app");
        let source = scratch.0.join("source");
        fs::create_dir_all(app.join("Contents/Resources")).unwrap();
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("package.json"), r#"{"main":"official.js"}"#).unwrap();
        fs::write(source.join("official.js"), "module.exports = {};").unwrap();
        let asar = app.join(incodex_core::paths::ASAR_REL);
        incodex_asar::pack_dir(&source, &asar).unwrap();
        let transaction = incodex_transaction::Engine::begin(&root, &app, "install").unwrap();
        // 只造有校验和的归属记录；没有签名和恢复快照，不能授权安装或卸载。
        let mut journal = transaction.journal().clone();
        journal.phase = "COMMITTED".into();
        let path = root
            .join("transactions")
            .join(&journal.install_id)
            .join("journal.json");
        let save = |journal: &mut incodex_transaction::JournalV2| {
            journal.checksum.clear();
            journal.checksum =
                crate::macos_update_assets::sha256_hex(&serde_json::to_vec(&*journal).unwrap());
            fs::write(&path, serde_json::to_vec(journal).unwrap()).unwrap();
        };
        incodex_asar::patch_asar(&asar, "old installed loader", Some(&journal.install_id)).unwrap();
        save(&mut journal);
        assert!(crate::install::has_recorded_install(&root, &app).unwrap());
        journal.phase = "DISCOVERED".into();
        save(&mut journal);
        assert!(!crate::install::has_recorded_install(&root, &app).unwrap_or(false));
        journal.phase = "COMMITTED".into();
        journal.target.real_path = source.to_str().unwrap().into();
        save(&mut journal);
        assert!(!crate::install::has_recorded_install(&root, &app).unwrap_or(false));
    }

    #[test]
    fn recorded_install_reuses_its_runtime_without_requiring_vendor_signature() {
        let route = select_launch_route(Ok(true), || {
            panic!("installed route must not require a vendor signature")
        });
        assert_eq!(route.unwrap(), LaunchRoute::Installed);
    }

    #[test]
    fn uninstalled_vendor_app_uses_cdp() {
        assert_eq!(
            select_launch_route(Ok(false), || Ok(true)).unwrap(),
            LaunchRoute::Cdp
        );
    }

    #[test]
    fn unverified_modified_app_does_not_enter_either_injection_path() {
        assert!(select_launch_route(Ok(false), || Ok(false)).is_err());
    }

    #[test]
    fn broken_install_proof_is_not_reinterpreted_as_a_clean_app() {
        let result = select_launch_route(Err("install proof failed".into()), || {
            panic!("do not bypass failed install proof")
        });
        assert_eq!(result.unwrap_err(), "install proof failed");
    }

    #[test]
    fn installed_launch_only_requests_normal_launchservices_activation() {
        let app = std::path::Path::new("/Applications/Official App.app");
        let command = installed_launch_command(app);
        assert_eq!(command.get_program(), "/usr/bin/open");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [std::ffi::OsStr::new("-a"), app.as_os_str()]
        );
        assert_eq!(command.get_envs().count(), 0);
    }

    #[test]
    fn launcher_publication_precedes_host_start() {
        let steps = std::cell::RefCell::new(Vec::new());
        let (launcher, ()) = publish_before_start(
            || {
                steps.borrow_mut().push("publish");
                Ok(PathBuf::from("/tmp/Incodex Launcher.app"))
            },
            || {
                steps.borrow_mut().push("start");
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(launcher, PathBuf::from("/tmp/Incodex Launcher.app"));
        assert_eq!(*steps.borrow(), ["publish", "start"]);
    }

    #[test]
    fn launcher_publication_failure_prevents_host_start() {
        let mut started = false;
        let result = publish_before_start(
            || Err("foreign launcher destination".into()),
            || {
                started = true;
                Ok(())
            },
        );

        assert_eq!(result.unwrap_err(), "foreign launcher destination");
        assert!(!started);
    }

    #[test]
    fn existing_owner_publishes_launcher_before_activation() {
        let source = include_str!("macos_launch.rs");
        let existing = source
            .split_once("LaunchClaim::Existing(host) => {")
            .expect("existing-owner route must remain explicit")
            .1
            .split_once("LaunchClaim::Primary(owner)")
            .expect("primary-owner route must remain explicit")
            .0;
        let publication = existing
            .find("publish_before_start(")
            .expect("existing owner must publish before activation");
        let publisher = existing
            .find("macos_launch_icon::publish_launcher(&home, &cli, &app)")
            .expect("existing owner must publish the owned launcher");
        let activation = existing
            .find("host.activate(&app)")
            .expect("existing owner must activate the current Host");

        assert!(publication < publisher && publisher < activation);
    }
}
