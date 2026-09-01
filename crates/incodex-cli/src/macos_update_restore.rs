use std::fs;

use incodex_core::canonical::is_official_app;
use incodex_macos::read_plist_info;

pub(crate) use crate::macos_update_assets::{ensure_private_dir, set_file_mode};
pub use crate::macos_update_assets::{
    publish_registration, read_registration, refresh_registered_helper, remove_registration,
    UpdateRegistration,
};
use crate::macos_update_assets::{read_regular_file, sha256_hex};

pub fn parse_relaunch_request(
    marker: Option<&str>,
    install_id: Option<&str>,
) -> Option<Result<String, String>> {
    if marker != Some("1") {
        return None;
    }
    Some(
        install_id
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| "macOS update relaunch has no install epoch".into()),
    )
}

pub fn try_run_relaunch() -> Option<Result<(), String>> {
    let marker = std::env::var("INCODEX_MACOS_UPDATE_RELAUNCH").ok();
    if marker.as_deref() != Some("1") {
        return None;
    }
    let request = parse_relaunch_request(
        marker.as_deref(),
        std::env::var("INCODEX_MACOS_UPDATE_INSTALL_ID")
            .ok()
            .as_deref(),
    )?;
    Some(request.and_then(run_relaunch_recovery))
}

fn run_relaunch_recovery(install_id: String) -> Result<(), String> {
    let root = incodex_core::paths::user_root();
    let registration = read_registration(&root)?
        .ok_or("macOS update registration disappeared before relaunch recovery")?;
    if registration.install_id != install_id {
        return Err("macOS update relaunch install epoch is stale".into());
    }
    if !is_official_app(&registration.app_path, None) {
        return Err("macOS update relaunch is not bound to the official Codex app".into());
    }
    verify_running_helper(&registration)?;
    let build = current_build(&registration.app_path)
        .ok_or("macOS update relaunch cannot read the replacement Codex build")?;
    crate::macos_update_log::log_coordinator_event(
        &root,
        &format!("relaunch recovery started build={build}"),
    );
    let restored_install_id = crate::install::reinstall_after_official_update(
        &root,
        &registration.app_path,
        &registration.helper_path,
        build,
    )?;
    crate::macos_update_log::log_coordinator_event(
        &root,
        &format!("relaunch recovery finished installId={restored_install_id}"),
    );
    Ok(())
}

fn current_build(app: &std::path::Path) -> Option<u64> {
    read_plist_info(app)?.app_build.parse::<u64>().ok()
}

fn verify_running_helper(registration: &UpdateRegistration) -> Result<(), String> {
    let current = std::env::current_exe()
        .map_err(|error| format!("cannot locate the running macOS update helper: {error}"))?;
    let current = fs::canonicalize(&current)
        .map_err(|error| format!("cannot resolve the running macOS update helper: {error}"))?;
    let expected = fs::canonicalize(&registration.helper_path)
        .map_err(|error| format!("cannot resolve the registered macOS update helper: {error}"))?;
    if current != expected {
        return Err("macOS update helper executable does not match its registration".into());
    }
    let digest = sha256_hex(&read_regular_file(&current, "running macOS update helper")?);
    if digest != registration.helper_sha256 {
        return Err("running macOS update helper failed its content hash".into());
    }
    Ok(())
}
