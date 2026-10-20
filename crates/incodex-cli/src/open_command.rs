/**
 * [INPUT]: 依赖CLI参数、WindowServer能力、Runtime代际与open生命周期。
 * [OUTPUT]: 提供run_open命令入口与结果展示。
 * [POS]: 隔离open的CLI适配；会话与进程所有权留在open模块。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::path::{Path, PathBuf};

use incodex_core::{format_kv, format_step, format_warn};
use incodex_runtime_bundle::ensure_current;

use crate::open_presentation::{DRY_RUN_COMPLETE, DRY_RUN_HEADING};
use crate::parse::ParsedCli;
use crate::profile_mask::resolve_profile_mask;
use crate::CliFailure;

const LAUNCH_RUNTIME_RELEASE_ENV: &str = "INCODEX_LAUNCH_RUNTIME_RELEASE";

use super::{
    default_source_home, describe_incognito_open, format_open_completion,
    prepare_incognito_open_with_profile_mask, user_root, wait_and_burn_native_close, OpenExitCode,
    OPENING_MESSAGE,
};

pub fn run_open(parsed: &ParsedCli) -> Result<(), CliFailure> {
    let app_path = parsed
        .app
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(incodex_core::DEFAULT_APP));
    let profile_mask = resolve_profile_mask(
        parsed.mask,
        parsed.name.as_deref(),
        parsed.avatar.as_deref().map(Path::new),
    )
    .map_err(CliFailure::from)?;

    if parsed.dry_run {
        let (bin, _) = describe_incognito_open(&app_path).map_err(CliFailure::from)?;
        println!("{}", format_step(DRY_RUN_HEADING, None));
        println!(
            "{}",
            format_kv("App", &app_path.display().to_string(), None)
        );
        println!("{}", format_kv("Binary", &bin.display().to_string(), None));
        if let Some(mask) = &profile_mask {
            println!("{}", format_kv("Profile", &mask.name, None));
        }
        println!("{}", format_warn(DRY_RUN_COMPLETE, None));
        return Ok(());
    }

    // Validate the executable before touching Runtime or creating a session.
    describe_incognito_open(&app_path).map_err(CliFailure::from)?;
    let root = user_root();
    let launch_release = launch_runtime_release().map_err(CliFailure::from)?;
    if let Some(expected) = launch_release.as_deref() {
        verify_launch_runtime(&root, expected).map_err(CliFailure::from)?;
    }
    let native_close = super::native_close::NativeCloseMonitor::new().map_err(CliFailure::from)?;
    prepare_runtime_for_open(&root, launch_release.as_deref(), |root| {
        ensure_current(root).map(|_| ())
    })
    .map_err(CliFailure::from)?;
    let source = default_source_home();
    let plan = prepare_incognito_open_with_profile_mask(
        &app_path,
        &root,
        &source,
        std::process::id() as i32,
        profile_mask,
    )?;
    println!("{}", format_step(OPENING_MESSAGE, None));
    println!(
        "{}",
        format_kv("Binary", &plan.bin.display().to_string(), None)
    );
    println!(
        "{}",
        format_kv("Home", &plan.home.display().to_string(), None)
    );
    println!("{}", format_kv("Session", &plan.session_id, None));
    let (process, cleanup) = wait_and_burn_native_close(&plan, &root, 250, native_close)?;
    let result = format_open_completion(&process, &cleanup);
    crate::terminal_presentation::print_terminal_result(&result);
    let code = process.exit_code(&cleanup);
    if code == OpenExitCode::Success {
        Ok(())
    } else {
        Err(CliFailure::with_code(
            code.as_i32(),
            process.failure_message(code),
        ))
    }
}

fn launch_runtime_release() -> Result<Option<String>, String> {
    std::env::var_os(LAUNCH_RUNTIME_RELEASE_ENV)
        .map(|value| {
            value.into_string().map_err(|_| {
                format!("{LAUNCH_RUNTIME_RELEASE_ENV} must contain a UTF-8 Runtime release")
            })
        })
        .transpose()
}

fn verify_launch_runtime(root: &Path, expected_release: &str) -> Result<(), String> {
    let identity = incodex_runtime_bundle::runtime_identity()?;
    let deployed = incodex_runtime_bundle::inspect_deployed(root)?
        .ok_or("selected Runtime is not published; refusing to publish from this CLI")?;
    validate_pinned_runtime(expected_release, &identity, &deployed)
}

fn validate_pinned_runtime(
    expected_release: &str,
    identity: &incodex_runtime_bundle::RuntimeIdentity,
    deployed: &incodex_runtime_bundle::DeployedRuntime,
) -> Result<(), String> {
    if deployed.release != expected_release {
        return Err(format!(
            "selected Runtime changed: expected {expected_release}, found {}; refusing to start a stale open process",
            deployed.release
        ));
    }
    if !identity.matches(deployed) {
        return Err(
            "running Incodex CLI does not contain the selected Runtime; refusing to publish an older Runtime or start private open".into(),
        );
    }
    Ok(())
}

fn prepare_runtime_for_open(
    root: &Path,
    launch_release: Option<&str>,
    ensure_current: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    match launch_release {
        Some(expected) => verify_launch_runtime(root, expected),
        None => ensure_current(root),
    }
}

#[cfg(test)]
mod tests {
    use super::{prepare_runtime_for_open, validate_pinned_runtime};
    use std::cell::Cell;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct RuntimeRoot(PathBuf);

    impl RuntimeRoot {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "incodex-launch-pinned-runtime-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for RuntimeRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn pinned_release_mismatch_does_not_publish_over_current_runtime() {
        let root = RuntimeRoot::new();
        incodex_runtime_bundle::ensure_current(&root.0).unwrap();
        let current = root.0.join("runtime/current.json");
        let before = fs::read(&current).unwrap();
        let pinned_release =
            "releases/next-0000000000000000000000000000000000000000000000000000000000000000";
        let publisher_called = Cell::new(false);

        let result = prepare_runtime_for_open(&root.0, Some(pinned_release), |_| {
            publisher_called.set(true);
            incodex_runtime_bundle::ensure_current(&root.0).map(|_| ())
        });

        assert!(result.is_err());
        assert!(!publisher_called.get());
        assert_eq!(fs::read(&current).unwrap(), before);
    }

    #[test]
    fn pinned_runtime_requires_the_running_clis_complete_identity() {
        let root = RuntimeRoot::new();
        incodex_runtime_bundle::ensure_current(&root.0).unwrap();
        let deployed = incodex_runtime_bundle::inspect_deployed(&root.0)
            .unwrap()
            .unwrap();
        let mut stale_cli = incodex_runtime_bundle::runtime_identity().unwrap();
        stale_cli.version.push_str("-older");

        let result = validate_pinned_runtime(&deployed.release, &stale_cli, &deployed);

        assert!(result
            .unwrap_err()
            .contains("does not contain the selected Runtime"));
    }
}
