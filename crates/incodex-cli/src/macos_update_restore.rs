use std::fs;

use incodex_core::canonical::is_official_app;
use incodex_macos::read_plist_info;

pub(crate) use crate::macos_update_assets::{ensure_private_dir, set_file_mode};
pub use crate::macos_update_assets::{
    publish_registration, publish_registration_if_generation, read_registration,
    refresh_registered_helper, remove_registration, UpdateRegistration,
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
    let expected_helper_sha256 = std::env::var("INCODEX_MACOS_UPDATE_HELPER_SHA256").ok();
    let result = request.and_then(|install_id| {
        run_relaunch_recovery(install_id, expected_helper_sha256.as_deref())
    });
    if let Err(error) = &result {
        crate::macos_update_log::log_coordinator_event(
            &incodex_core::paths::user_root(),
            &format!("relaunch recovery failed: {error}"),
        );
    }
    Some(result)
}

fn run_relaunch_recovery(
    install_id: String,
    expected_helper_sha256: Option<&str>,
) -> Result<(), String> {
    let root = incodex_core::paths::user_root();
    let registration = read_registration(&root)?
        .ok_or("macOS update registration disappeared before relaunch recovery")?;
    if registration.install_id != install_id {
        return Err("macOS update relaunch install epoch is stale".into());
    }
    if let Some(expected_helper_sha256) = expected_helper_sha256 {
        if registration.helper_sha256 != expected_helper_sha256 {
            return Err("macOS update helper generation changed before recovery".into());
        }
    }
    if !is_official_app(&registration.app_path, None) {
        return Err("macOS update relaunch is not bound to the official Codex app".into());
    }
    verify_running_helper(&root, &registration, expected_helper_sha256)?;
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
        &registration.install_id,
        expected_helper_sha256.unwrap_or(&registration.helper_sha256),
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

fn verify_running_helper(
    root: &std::path::Path,
    registration: &UpdateRegistration,
    expected_helper_sha256: Option<&str>,
) -> Result<(), String> {
    let expected_helper_sha256 = expected_helper_sha256.unwrap_or(&registration.helper_sha256);
    if expected_helper_sha256.len() != 64
        || !expected_helper_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("macOS update helper received an invalid content hash".into());
    }
    let current = std::env::current_exe()
        .map_err(|error| format!("cannot locate the running macOS update helper: {error}"))?;
    let current = fs::canonicalize(&current)
        .map_err(|error| format!("cannot resolve the running macOS update helper: {error}"))?;
    let expected = root
        .join("helpers")
        .join("macos-update")
        .join(expected_helper_sha256)
        .join("incodex");
    let expected = fs::canonicalize(&expected)
        .map_err(|error| format!("cannot resolve the registered macOS update helper: {error}"))?;
    if current != expected {
        return Err("macOS update helper executable does not match its registration".into());
    }
    let digest = sha256_hex(&read_regular_file(&current, "running macOS update helper")?);
    if digest != expected_helper_sha256 {
        return Err("running macOS update helper failed its content hash".into());
    }
    Ok(())
}
