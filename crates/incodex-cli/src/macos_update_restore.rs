/*
 * [INPUT]: 依赖已注册更新 helper/epoch 的证明与 install 的有界恢复事务。
 * [OUTPUT]: 提供官方更新 relaunch 恢复，成功/失败仍写原 coordinator.log。
 * [POS]: 后台恢复完成后提交有界权限检查；不自动授予或绕过代际 CAS。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use incodex_core::canonical::is_official_app;
use incodex_macos::read_plist_info;

use crate::macos_update_assets::verify_running_helper;
pub use crate::macos_update_assets::{
    publish_registration, publish_registration_if_generation,
    publish_registration_if_signing_generation, read_registration, refresh_registered_helper,
    remove_registration, UpdateRegistration,
};

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
    let install_id = match request {
        Ok(install_id) => install_id,
        Err(error) => return Some(Err(error)),
    };
    let expected_helper_sha256 = std::env::var("INCODEX_MACOS_UPDATE_HELPER_SHA256").ok();
    let result = run_relaunch_recovery(install_id, expected_helper_sha256.as_deref());
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
        registration.signing_certificate_sha256.as_deref(),
    )?;
    crate::macos_update_log::log_coordinator_event(
        &root,
        &format!("relaunch recovery finished installId={restored_install_id}"),
    );
    crate::permission_update::schedule(crate::permission_update::Source::OfficialUpdate);
    Ok(())
}

fn current_build(app: &std::path::Path) -> Option<u64> {
    read_plist_info(app)?.app_build.parse::<u64>().ok()
}
