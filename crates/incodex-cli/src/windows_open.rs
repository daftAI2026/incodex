use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use incodex_core::windows_session::{
    burn_windows_session, copy_windows_settings, create_windows_session, WindowsCleanupResult,
    WindowsSessionHome,
};
use incodex_core::{format_kv, format_step, format_warn};

use crate::cdp::{allocate_debug_port, debug_launch_args, InjectionOptions};
use crate::profile_mask::ProfileMask;
use crate::windows_app::{discover_codex_package, WindowsCodexApp};
use crate::{parse::ParsedCli, CliFailure};

#[derive(Debug)]
pub struct WindowsOpenPlan {
    pub bin: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, PathBuf>,
    pub env_flags: BTreeMap<String, String>,
    pub session: WindowsSessionHome,
    pub debug_port: u16,
    pub injection: InjectionOptions,
}

pub fn run_open(parsed: &ParsedCli) -> Result<(), CliFailure> {
    if parsed.app.is_some() {
        return Err(CliFailure::new(
            "Windows open discovers the current user's official Microsoft Store package; --app is not supported",
        ));
    }
    let app = discover_codex_package().map_err(CliFailure::from)?;
    if parsed.dry_run {
        println!(
            "{}",
            format_step("Open incognito without patching Codex", None)
        );
        println!("{}", format_kv("Package", &app.package_full_name, None));
        println!(
            "{}",
            format_kv("Binary", &app.executable.display().to_string(), None)
        );
        println!("{}", format_warn("Dry run. No window opened.", None));
        return Ok(());
    }
    Err(CliFailure::new(
        "open is not supported on Windows without --dry-run yet",
    ))
}

pub fn prepare_windows_open(
    app: &WindowsCodexApp,
    user_root: &Path,
    source_home: &Path,
    profile_mask: Option<ProfileMask>,
) -> Result<WindowsOpenPlan, String> {
    let session = create_windows_session(user_root)?;
    let prepared = (|| {
        copy_windows_settings(&session, source_home)?;
        let debug_port = allocate_debug_port()?;
        let args = debug_launch_args(&session.chromium.display().to_string(), debug_port);
        let env = BTreeMap::from([
            ("CODEX_HOME".to_string(), session.home.clone()),
            (
                "CODEX_ELECTRON_USER_DATA_PATH".to_string(),
                session.chromium.clone(),
            ),
            ("INCODEX_SESSION_ROOT".to_string(), session.root.clone()),
            ("INCODEX_SOURCE_HOME".to_string(), source_home.to_path_buf()),
        ]);
        let env_flags = BTreeMap::from([
            ("INCODEX_INCOGNITO".to_string(), "1".to_string()),
            ("INCODEX_CLEANUP_OWNER".to_string(), "native".to_string()),
            ("INCODEX_SESSION_ID".to_string(), session.session_id.clone()),
        ]);
        Ok(WindowsOpenPlan {
            bin: app.executable.clone(),
            args,
            env,
            env_flags,
            session: session.clone(),
            debug_port,
            injection: InjectionOptions {
                locale: read_locale_override(source_home),
                profile_mask,
            },
        })
    })();

    match prepared {
        Ok(plan) => Ok(plan),
        Err(error) => match burn_windows_session(&session) {
            WindowsCleanupResult::Removed => Err(error),
            WindowsCleanupResult::Retained { reason } => Err(format!(
                "{error}; incomplete Windows session retained at {}: {reason}",
                session.root.display()
            )),
            WindowsCleanupResult::Unknown { reason } => Err(format!(
                "{error}; Windows session cleanup state is unknown at {}: {reason}",
                session.root.display()
            )),
        },
    }
}

fn read_locale_override(source_home: &Path) -> Option<String> {
    let content = std::fs::read_to_string(source_home.join("config.toml")).ok()?;
    content.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        if name.trim() != "localeOverride" {
            return None;
        }
        let value = value.trim();
        value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                value
                    .strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
            })
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn plan() -> (PathBuf, WindowsOpenPlan) {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "incodex-windows-open-lifecycle-{}-{sequence}",
            std::process::id()
        ));
        let profile = root.join("profile");
        let source = profile.join(".codex");
        fs::create_dir_all(&source).expect("create source");
        fs::write(source.join("auth.json"), b"fixture").expect("write auth");
        let executable = std::env::current_exe().expect("test executable");
        let app = WindowsCodexApp {
            package_full_name: "OpenAI.Codex_fixture_x64__2p2nqsd0c76g0".to_string(),
            install_location: root.join("package"),
            executable: executable.clone(),
            asar: root.join("package/app/resources/app.asar"),
            asar_unpacked: root.join("package/app/resources/app.asar.unpacked"),
            architecture: "X64".to_string(),
        };
        let mut plan = prepare_windows_open(&app, &profile.join(".incodex"), &source, None)
            .expect("prepare lifecycle plan");
        plan.args = vec![
            "windows_open::tests::open_process_fixture".to_string(),
            "--exact".to_string(),
            "--nocapture".to_string(),
        ];
        plan.env_flags
            .insert("INCODEX_WINDOWS_OPEN_FIXTURE".to_string(), "1".to_string());
        (root, plan)
    }

    #[test]
    fn open_process_fixture() {
        if std::env::var_os("INCODEX_WINDOWS_OPEN_FIXTURE").is_none() {
            return;
        }
        thread::sleep(Duration::from_millis(250));
    }

    #[test]
    fn successful_contained_process_exit_removes_the_session() {
        let (root, plan) = plan();
        let session_root = plan.session.root.clone();

        let outcome = execute_windows_open_with(plan, |_port, _options, alive| {
            assert!(alive.load(Ordering::Acquire));
            Ok(())
        });

        assert_eq!(outcome.process, WindowsOpenProcessResult::Exited(0));
        assert!(outcome.ui_ready);
        assert_eq!(outcome.cleanup, WindowsCleanupResult::Removed);
        assert!(!session_root.exists());
        fs::remove_dir_all(root).expect("remove lifecycle fixture");
    }

    #[test]
    fn injection_failure_terminates_the_job_before_removing_the_session() {
        let (root, plan) = plan();
        let session_root = plan.session.root.clone();

        let outcome =
            execute_windows_open_with(plan, |_port, _options, _alive: Arc<AtomicBool>| {
                Err("fixture injection refused".to_string())
            });

        assert!(matches!(
            outcome.process,
            WindowsOpenProcessResult::InjectionFailed(ref error)
                if error == "fixture injection refused"
        ));
        assert!(!outcome.ui_ready);
        assert_eq!(outcome.cleanup, WindowsCleanupResult::Removed);
        assert!(!session_root.exists());
        fs::remove_dir_all(root).expect("remove lifecycle fixture");
    }
}
