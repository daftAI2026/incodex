// 显式 Runtime 发布交接同一安装授权下的更新记录；旧 helper 的严格读端保持不变。
use super::*;
use crate::windows_registration::{
    read_windows_debug_registration, registration_matches_install_state,
};
use crate::windows_runtime::verify_installed_windows_runtime;

pub(super) fn synchronize(
    user_root: &Path,
    runtime_release: &str,
) -> Result<Option<WindowsInstallState>, String> {
    synchronize_with(user_root, runtime_release, write_update_repair_intent)
}

pub(super) fn synchronize_with(
    user_root: &Path,
    runtime_release: &str,
    mut write_intent: impl FnMut(&Path, &WindowsUpdateRepairIntent) -> Result<(), String>,
) -> Result<Option<WindowsInstallState>, String> {
    let _lock = InstallStateLock::acquire()?;
    validate_runtime_release(runtime_release)?;
    let user_root = ensure_private_windows_dir(user_root)?;
    let Some(source) = read_state_from_root(&user_root, HelperValidation::Required)? else {
        return Ok(None);
    };
    verify_installed_windows_runtime(&user_root, runtime_release)?;
    if !source.desired_enabled()
        || !matches!(
            source.phase,
            WindowsInstallPhase::EnabledUnobserved | WindowsInstallPhase::EnabledObserved
        )
    {
        return Err("Windows Runtime integration is not in an enabled state".into());
    }
    if runtime_release_version(&source.runtime_release)? > runtime_release_version(runtime_release)?
    {
        return Err("a newer installed Windows Runtime is already active".into());
    }
    let mut selected = source.clone();
    // epoch 撤销安装授权，不表示内容版本；必须保留活 App 协调器捕获的授权。
    selected.runtime_release = runtime_release.to_string();

    let prearm = read_windows_update_prearm_intent(&user_root)?;
    let repair = read_windows_update_repair_intent(&user_root)?;
    if prearm.is_some() || repair.is_some() {
        let registration = read_windows_debug_registration(&user_root)?
            .ok_or("Windows Runtime handoff needs the installed registration evidence")?;
        if !registration_matches_install_state(&registration, &source) {
            return Err(
                "Windows Runtime handoff registration changed; pending update retained".into(),
            );
        }
    }
    if let (Some(prearm), Some(repair)) = (&prearm, &repair) {
        if prearm.operation_id != repair.operation_id
            || prearm.target_package_full_name != repair.target_package_full_name
        {
            return Err("Windows Runtime handoff has conflicting pending updates".into());
        }
    }
    // 全部预检后才落盘。目标 ID 与系统 debugger 环境不变，不触碰运行中的官方 App。
    // 升级中断时留下旧 prearm 作为唯一可消费路线；旧 promotion 会重建 repair。
    // 同版本修复先恢复 prearm，使已运行的新来源也能保留正常首次启动路线。
    let ordered = if source == selected {
        [prearm, repair]
    } else {
        [repair, prearm]
    };
    let intents = ordered
        .into_iter()
        .flatten()
        .map(|intent| prepare_intent(&user_root, &source, &selected, intent))
        .collect::<Result<Vec<_>, _>>()?;
    for intent in &intents {
        write_intent(&user_root, intent)?;
    }
    // 先预备 pending，再提交来源状态；中断时旧 helper 拒绝，重跑同一同步可完成交接。
    // 同版本也必须经过以上修复，不能因版本号相等提前成功。
    if source != selected {
        write_state(&user_root, &selected)?;
    }
    Ok(Some(selected))
}

fn prepare_intent(
    root: &Path,
    source: &WindowsInstallState,
    selected: &WindowsInstallState,
    mut intent: WindowsUpdateRepairIntent,
) -> Result<WindowsUpdateRepairIntent, String> {
    crate::windows_app::validate_codex_package_full_name(&intent.target_package_full_name)?;
    // 同一 registration 的状态只能从 enabled 走向退休，重新安装必换 ID。
    // 只允许显式发布器续接此身份；后台读端仍须精确匹配 epoch 与 Runtime。
    let same_owner = intent.source_registration_id == source.registration_id
        && intent.source_package_full_name == source.package_full_name
        && intent.helper_path == source.helper_path
        && intent.helper_sha256 == source.helper_sha256;
    let retained = intent.source_epoch <= source.epoch
        && runtime_release_version(&intent.runtime_release)?
            <= runtime_release_version(&source.runtime_release)?;
    let prepared = source.runtime_release != selected.runtime_release
        && intent.source_epoch == selected.epoch
        && intent.runtime_release == selected.runtime_release;
    if !same_owner || (!retained && !prepared) {
        return Err(
            "Windows Runtime handoff pending authorization changed; update retained".into(),
        );
    }
    verify_installed_windows_runtime(root, &intent.runtime_release)?;
    intent.source_epoch = selected.epoch;
    intent.runtime_release = selected.runtime_release.clone();
    if !intent.authorizes_source(selected, &intent.target_package_full_name) {
        return Err("Windows Runtime handoff cannot authorize the pending target".into());
    }
    Ok(intent)
}
