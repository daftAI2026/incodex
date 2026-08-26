use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use incodex_core::windows_session::{
    burn_windows_session, copy_windows_settings, create_windows_session, WindowsCleanupResult,
    WindowsSessionHome,
};

use crate::cdp::{allocate_debug_port, debug_launch_args, InjectionOptions};
use crate::profile_mask::ProfileMask;
use crate::windows_app::WindowsCodexApp;

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
