use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use incodex_core::paths::{home_dir, user_root};
use incodex_core::session::{
    burn_session_home, copy_settings, create_session_home, sweep_orphan_sessions, target_id_from_exec,
    BurnExpected, SessionHome,
};
use incodex_core::{format_kv, format_ok, format_step, format_warn};

use crate::parse::ParsedCli;

#[derive(Debug, Clone)]
pub struct OpenPlan {
    pub bin: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub home: PathBuf,
    pub chromium: PathBuf,
    pub session_id: String,
    pub session_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupResult {
    Removed { attempts: u8 },
    Retained {
        attempts: u8,
        retained_path: PathBuf,
        reason: String,
    },
}

impl CleanupResult {
    pub fn removed(&self) -> bool {
        matches!(self, CleanupResult::Removed { .. })
    }
}

pub fn chat_gpt_binary(app_path: &Path) -> PathBuf {
    app_path.join("Contents/MacOS/ChatGPT")
}

pub fn describe_incognito_open(app_path: &Path) -> (PathBuf, Vec<String>) {
    (
        chat_gpt_binary(app_path),
        vec!["--user-data-dir=<isolated-chromium>".to_string()],
    )
}

pub fn default_source_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".codex"))
}

pub fn prepare_incognito_open(
    app_path: &Path,
    user_root: &Path,
    source_home: &Path,
    pid: i32,
) -> Result<OpenPlan, String> {
    let bin = chat_gpt_binary(app_path);
    if !bin.exists() {
        return Err(format!("Codex binary not found: {}", bin.display()));
    }
    let target_id = target_id_from_exec(&bin.to_string_lossy());
    let _ = sweep_orphan_sessions(user_root, Some(&target_id));
    let session = create_session_home(user_root, Some(&target_id), pid, &source_home.to_string_lossy())?;
    if let Err(error) = copy_settings(&session.home, source_home, user_root) {
        let _ = burn_session_home(
            &session.root,
            &BurnExpected {
                user_root,
                session_id: Some(&session.session_id),
                ino: Some(session.ino),
                dev: Some(session.dev),
            },
        );
        return Err(error);
    }
    Ok(plan_from_session(bin, session, source_home))
}

fn plan_from_session(bin: PathBuf, session: SessionHome, source_home: &Path) -> OpenPlan {
    OpenPlan {
        args: vec![format!("--user-data-dir={}", session.chromium.display())],
        env: vec![
            ("CODEX_HOME".into(), session.home.display().to_string()),
            ("INCODEX_INCOGNITO".into(), "1".into()),
            ("INCODEX_SESSION_ID".into(), session.session_id.clone()),
            (
                "INCODEX_SESSION_ROOT".into(),
                session.root.display().to_string(),
            ),
            (
                "CODEX_ELECTRON_USER_DATA_PATH".into(),
                session.chromium.display().to_string(),
            ),
            (
                "INCODEX_SOURCE_HOME".into(),
                source_home.display().to_string(),
            ),
        ],
        home: session.home,
        chromium: session.chromium,
        session_id: session.session_id,
        session_root: session.root,
        bin,
    }
}

pub fn format_session_cleanup(cleanup: &CleanupResult) -> (bool, String) {
    match cleanup {
        CleanupResult::Removed { .. } => (true, "Closed. Isolated session removed.".into()),
        CleanupResult::Retained {
            retained_path,
            reason,
            ..
        } => (
            false,
            format!(
                "Closed. Isolated session kept at {} ({reason})",
                retained_path.display()
            ),
        ),
    }
}

pub fn wait_and_burn(
    plan: &OpenPlan,
    user_root: &Path,
    retry_delay_ms: u64,
) -> Result<(i32, CleanupResult), String> {
    wait_and_burn_with(
        plan,
        user_root,
        retry_delay_ms,
        spawn_plan,
        |root, expected| burn_session_home(root, expected).map_err(|err| err),
    )
}

fn spawn_plan(plan: &OpenPlan) -> Result<i32, String> {
    let mut command = Command::new(&plan.bin);
    command.args(&plan.args);
    for (key, value) in &plan.env {
        command.env(key, value);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match command.spawn() {
        Ok(mut child) => child
            .wait()
            .map(|status| status.code().unwrap_or(1))
            .map_err(|err| err.to_string()),
        Err(err) => Err(err.to_string()),
    }
}

pub fn wait_and_burn_with<S, B>(
    plan: &OpenPlan,
    user_root: &Path,
    retry_delay_ms: u64,
    spawn: S,
    mut burn: B,
) -> Result<(i32, CleanupResult), String>
where
    S: FnOnce(&OpenPlan) -> Result<i32, String>,
    B: FnMut(&Path, &BurnExpected<'_>) -> Result<(), String>,
{
    let code = match spawn(plan) {
        Ok(code) => code,
        Err(_) => 1,
    };
    let expected = BurnExpected {
        user_root,
        session_id: Some(&plan.session_id),
        ino: None,
        dev: None,
    };
    let cleanup = burn_with_retries(&plan.session_root, &expected, retry_delay_ms, &mut burn);
    Ok((code, cleanup))
}

fn burn_with_retries<B>(
    session_root: &Path,
    expected: &BurnExpected<'_>,
    retry_delay_ms: u64,
    burn: &mut B,
) -> CleanupResult
where
    B: FnMut(&Path, &BurnExpected<'_>) -> Result<(), String>,
{
    let mut reason = "session directory still present".to_string();
    for attempt in 1u8..=5 {
        if let Err(error) = burn(session_root, expected) {
            reason = error;
            if attempt == 5 {
                return if session_root.exists() {
                    CleanupResult::Retained {
                        attempts: attempt,
                        retained_path: session_root.to_path_buf(),
                        reason,
                    }
                } else {
                    CleanupResult::Removed { attempts: attempt }
                };
            }
        }
        if !session_root.exists() {
            return CleanupResult::Removed { attempts: attempt };
        }
        if attempt < 5 && retry_delay_ms > 0 {
            thread::sleep(Duration::from_millis(retry_delay_ms * u64::from(attempt)));
        }
    }
    if session_root.exists() {
        CleanupResult::Retained {
            attempts: 5,
            retained_path: session_root.to_path_buf(),
            reason,
        }
    } else {
        CleanupResult::Removed { attempts: 5 }
    }
}

pub fn run_open(parsed: &ParsedCli) -> Result<(), String> {
    let app_path = parsed
        .app
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(incodex_core::DEFAULT_APP));
    if parsed.dry_run {
        let (bin, _) = describe_incognito_open(&app_path);
        println!("{}", format_step("Open incognito without patching Codex", None));
        println!("{}", format_kv("App", &app_path.display().to_string(), None));
        println!("{}", format_kv("Binary", &bin.display().to_string(), None));
        println!("{}", format_warn("Dry run. No window opened.", None));
        return Ok(());
    }
    let root = user_root();
    let source = default_source_home();
    let plan = prepare_incognito_open(&app_path, &root, &source, std::process::id() as i32)?;
    println!("{}", format_step("Opening incognito window", None));
    println!("{}", format_kv("Binary", &plan.bin.display().to_string(), None));
    println!("{}", format_kv("Home", &plan.home.display().to_string(), None));
    let (_code, cleanup) = wait_and_burn(&plan, &root, 250)?;
    let (ok, message) = format_session_cleanup(&cleanup);
    if ok {
        println!("{}", format_ok(&message, None));
    } else {
        println!("{}", format_warn(&message, None));
    }
    println!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_root() -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("incodex-open-unit-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fake_app(root: &Path) -> PathBuf {
        let app = root.join("ChatGPT.app");
        let mac = app.join("Contents/MacOS");
        fs::create_dir_all(&mac).unwrap();
        fs::write(mac.join("ChatGPT"), "#!/bin/sh\nexit 0\n").unwrap();
        app
    }

    #[test]
    fn copy_failure_burns_the_session() {
        let root = temp_root();
        let app = fake_app(&root);
        let user = root.join("home");
        let source = root.join("codex");
        fs::create_dir_all(&source).unwrap();
        let bin = chat_gpt_binary(&app);
        let target_id = target_id_from_exec(&bin.to_string_lossy());
        let session = create_session_home(&user, Some(&target_id), 1, "").unwrap();
        fs::remove_dir_all(&session.home).unwrap();
        assert!(copy_settings(&session.home, &source, &user).is_err());
        burn_session_home(
            &session.root,
            &BurnExpected {
                user_root: &user,
                session_id: Some(&session.session_id),
                ino: Some(session.ino),
                dev: Some(session.dev),
            },
        )
        .unwrap();
        assert!(!session.root.exists());
    }

    #[test]
    fn burn_failure_does_not_claim_removed() {
        let root = temp_root();
        let app = fake_app(&root);
        let user = root.join("home");
        let source = root.join("codex");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("auth.json"), "{}\n").unwrap();
        let plan = prepare_incognito_open(&app, &user, &source, 1).unwrap();
        let (_code, cleanup) = wait_and_burn_with(
            &plan,
            &user,
            0,
            |_| Ok(0),
            |_, _| Err("EPERM".into()),
        )
        .unwrap();
        assert!(plan.session_root.exists());
        assert_eq!(
            cleanup,
            CleanupResult::Retained {
                attempts: 5,
                retained_path: plan.session_root.clone(),
                reason: "EPERM".into(),
            }
        );
        let (ok, message) = format_session_cleanup(&cleanup);
        assert!(!ok);
        assert!(!message.to_lowercase().contains("removed"));
    }

    #[test]
    fn spawn_error_still_burns() {
        let root = temp_root();
        let app = fake_app(&root);
        let user = root.join("home");
        let source = root.join("codex");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("auth.json"), "{}\n").unwrap();
        let plan = prepare_incognito_open(&app, &user, &source, 1).unwrap();
        let (_code, cleanup) = wait_and_burn_with(
            &plan,
            &user,
            0,
            |_| Err("ENOENT".into()),
            |root, expected| burn_session_home(root, expected),
        )
        .unwrap();
        assert!(!plan.session_root.exists());
        assert!(cleanup.removed());
    }

}
