/*
 * [INPUT]: 依赖既有安装锁、登记/更新凭据、内容寻址 helper、观察器两阶段回执。
 * [OUTPUT]: 提供 Runtime 发布后的 helper 交接；登录入口可续接精确未提交事务，不退出官方宿主。
 * [POS]: 安装状态的原生内容迁移；写前凭据只引用既有授权，不拥有窗口、会话或第二套 IPC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;
use crate::windows_activation::WindowsInstalledRuntimeRegistration;
use crate::windows_registration::{
    read_transient_windows_debug_registration, read_windows_debug_registration,
    registration_matches_install_state, replace_installed_registration_for_handoff,
};
use crate::windows_runtime::verify_installed_windows_runtime;

const JOURNAL: &str = "windows-helper-handoff.json";
const PROTOCOL_MODE: &str = "--incodex-windows-update-helper-protocol";
// 此修订声明原生恢复能力；只有所需协议变化才轮换已兼容的固定 helper。
const PROTOCOL: &str = "incodex-windows-update-helper/4";
const OWNER_MODE: &str = "--incodex-windows-owner-handoff-protocol";
const OWNER_PROTOCOL: &str = "incodex-windows-owner-handoff/1";

pub(crate) fn try_run_helper_protocol(args: &[String]) -> Option<Result<(), String>> {
    let protocol = match args.first().map(String::as_str) {
        Some(PROTOCOL_MODE) => PROTOCOL,
        Some(OWNER_MODE) => OWNER_PROTOCOL,
        _ => return None,
    };
    Some(if args.len() == 1 {
        println!("{protocol}");
        Ok(())
    } else {
        Err("Windows helper protocol probe takes no arguments".into())
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Handoff {
    schema_version: u32,
    source: WindowsInstallState,
    target: WindowsInstallState,
    prearm: Option<WindowsUpdateRepairIntent>,
    repair: Option<WindowsUpdateRepairIntent>,
}

trait Backend {
    type Observer;
    fn current_package(&mut self) -> Result<String, String>;
    fn source_is_idle(&mut self, package: &str) -> Result<bool, String>;
    fn registered(&mut self, helper: &Path) -> Result<bool, String>;
    fn compatible(&mut self, helper: &Path) -> Result<bool, String>;
    fn supports_live_owner(&mut self, helper: &Path) -> Result<bool, String>;
    fn stop(&mut self, root: &Path) -> Result<(), String>;
    fn login(
        &mut self,
        source: &WindowsInstallState,
        target: &WindowsInstallState,
    ) -> Result<(), String>;
    fn enable(&mut self, state: &WindowsInstallState) -> Result<(), String>;
    fn prepare(&mut self, state: &WindowsInstallState) -> Result<Self::Observer, String>;
    fn preserve(&mut self, observer: &mut Self::Observer);
    fn active(&mut self, observer: Self::Observer) -> Result<u32, String>;
    fn restart_source(&mut self, state: &WindowsInstallState) -> Result<(), String>;
}

struct Native;
impl Backend for Native {
    type Observer = crate::windows_update_observer::PreparedObserver;
    fn current_package(&mut self) -> Result<String, String> {
        crate::windows_app::discover_codex_package().map(|app| app.package_full_name)
    }
    fn source_is_idle(&mut self, package: &str) -> Result<bool, String> {
        crate::windows_process::strict_running_codex_package_process_ids(package)
            .map(|pids| pids.is_empty())
            .map_err(|e| e.to_string())
    }
    fn registered(&mut self, helper: &Path) -> Result<bool, String> {
        crate::windows_update_startup::is_registered(helper)
    }
    fn compatible(&mut self, helper: &Path) -> Result<bool, String> {
        probe_protocol(helper, PROTOCOL_MODE, PROTOCOL)
    }
    fn supports_live_owner(&mut self, helper: &Path) -> Result<bool, String> {
        probe_protocol(helper, OWNER_MODE, OWNER_PROTOCOL)
    }
    fn stop(&mut self, root: &Path) -> Result<(), String> {
        crate::windows_update_observer::stop(root)
    }
    fn login(
        &mut self,
        source: &WindowsInstallState,
        target: &WindowsInstallState,
    ) -> Result<(), String> {
        crate::windows_update_startup::replace_for_handoff(source, target)
    }
    fn enable(&mut self, state: &WindowsInstallState) -> Result<(), String> {
        crate::windows_activation::enable_installed_runtime(
            &WindowsInstalledRuntimeRegistration::from_install_state(state)?,
        )
    }
    fn prepare(&mut self, state: &WindowsInstallState) -> Result<Self::Observer, String> {
        crate::windows_update_observer::prepare(state)
    }
    fn active(&mut self, observer: Self::Observer) -> Result<u32, String> {
        observer.wait_active()
    }
    fn preserve(&mut self, observer: &mut Self::Observer) {
        observer.preserve();
    }
    fn restart_source(&mut self, state: &WindowsInstallState) -> Result<(), String> {
        crate::windows_update_observer::start(state)
    }
}

fn probe_protocol(helper: &Path, mode: &str, protocol: &str) -> Result<bool, String> {
    use std::os::windows::process::CommandExt;
    let result = std::process::Command::new(helper)
        .arg(mode)
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    Ok(result.status.success()
        && result.stdout.len() <= 128
        && String::from_utf8_lossy(&result.stdout).trim() == protocol)
}

pub(crate) fn refresh_update_helper(root: &Path, executable: &Path) -> Result<Option<u32>, String> {
    refresh_with(root, executable, &mut Native)
}

pub(crate) fn resume_pending_helper(root: &Path, executable: &Path) -> Result<(), String> {
    if root.join(JOURNAL).try_exists().map_err(|e| e.to_string())? {
        refresh_update_helper(root, executable)?;
    }
    Ok(())
}

pub(crate) fn resume_observer_handoff(root: &Path, helper: &Path) -> Result<bool, String> {
    resume_observer_with(root, helper, &mut Native)
}

fn resume_observer_with<B: Backend>(
    root: &Path,
    helper: &Path,
    backend: &mut B,
) -> Result<bool, String> {
    if !observer_can_resume(root, helper, backend)? {
        return Ok(false);
    }
    // 接管前不持 observer owner；旧观察器回滚启动不等待此发布锁。
    let _publication = acquire_windows_runtime_update()?;
    if !observer_can_resume(root, helper, backend)? {
        return Ok(false);
    }
    refresh_with(root, helper, backend).map(|pid| pid.is_some())
}

fn observer_can_resume<B: Backend>(
    root: &Path,
    helper: &Path,
    backend: &mut B,
) -> Result<bool, String> {
    let _gate = acquire_windows_install_state()?;
    reseal_private_windows_root_if_present(root)?;
    let Some(h) = read_journal(root)? else {
        return Ok(false);
    };
    if h.target.helper_path != helper {
        return Ok(false);
    }
    validate_snapshot(root, &h)?;
    Ok(
        read_windows_install_state(root)?.as_ref() == Some(&h.source)
            && backend.registered(helper)?,
    )
}

fn refresh_with<B: Backend>(
    root: &Path,
    executable: &Path,
    backend: &mut B,
) -> Result<Option<u32>, String> {
    let _publication = acquire_windows_runtime_update()?;
    let canonical = {
        let _lock = acquire_windows_install_state()?;
        reseal_private_windows_root_if_present(root)?;
        fs::canonicalize(root).map_err(|error| error.to_string())?
    };
    let root = canonical.as_path();
    let handoff = {
        let _lock = acquire_windows_install_state()?;
        reseal_private_windows_root_if_present(root)?;
        let pending = read_journal(root)?;
        let Some(source) = read_windows_install_state(root)? else {
            return if pending.is_some() {
                Err("Windows helper handoff installation was removed".into())
            } else {
                Ok(None)
            };
        };
        if pending.is_none()
            && (!source.desired_enabled()
                || !backend.registered(&source.helper_path)?
                || backend.compatible(&source.helper_path)?)
        {
            return Ok(None);
        }
        if read_transient_windows_debug_registration(root)?.is_some() {
            return Err(
                "Windows helper handoff waits for the current private launch transaction".into(),
            );
        }
        let handoff = match pending {
            Some(pending) => pending,
            None => {
                let handoff = plan(root, &source, executable, backend)?;
                write_journal(root, &handoff)?;
                handoff
            }
        };
        validate_handoff(root, &handoff)?;
        validate_snapshot(root, &handoff)?;
        handoff
    };
    // 停止只属于 Incodex 的旧观察器；等待期间不能占安装锁，宿主和会话保持原样。
    backend.stop(root)?;
    let result = commit_with(root, &handoff, backend).and_then(|observer| backend.active(observer));
    match result {
        Ok(pid) => {
            let _lock = acquire_windows_install_state()?;
            validate_snapshot(root, &handoff)?;
            if read_windows_install_state(root)?.as_ref() != Some(&handoff.target) {
                return Err(
                    "Windows helper activation authorization changed; journal retained".into(),
                );
            }
            retire_journal(root)?;
            Ok(Some(pid))
        }
        Err(error) => {
            let rollback = restore_source(root, &handoff, backend);
            Err(format!("Runtime published; native helper handoff retained: {error}; source restoration: {}; rerun incodex runtime",
                rollback.map(|()| "ready".to_string()).unwrap_or_else(|error| error)))
        }
    }
}

fn plan<B: Backend>(
    root: &Path,
    source: &WindowsInstallState,
    executable: &Path,
    backend: &mut B,
) -> Result<Handoff, String> {
    let evidence =
        read_windows_debug_registration(root)?.ok_or("Windows helper registration is missing")?;
    if !source.desired_enabled() || !registration_matches_install_state(&evidence, source) {
        return Err("Windows helper source authorization changed".into());
    }
    let prearm = read_windows_update_prearm_intent(root)?;
    let repair = read_windows_update_repair_intent(root)?;
    for intent in prearm.iter().chain(repair.iter()) {
        if !intent.authorizes_source(source, &intent.target_package_full_name) {
            return Err("Windows helper pending authorization changed".into());
        }
    }
    if let (Some(a), Some(b)) = (&prearm, &repair) {
        if a.operation_id != b.operation_id
            || a.target_package_full_name != b.target_package_full_name
        {
            return Err("Windows helper pending operations conflict".into());
        }
    }
    let package = backend.current_package()?;
    let mut target = source.clone();
    if source.package_full_name == package
        && !backend.source_is_idle(&package)?
        && !backend.supports_live_owner(&source.helper_path)?
    {
        return Err("The running legacy Windows controller cannot migrate its fixed verifier; Runtime content is retained. Retry runtime after a normal host exit.".into());
    }
    if source.package_full_name != package {
        let intent = prearm
            .as_ref()
            .or(repair.as_ref())
            .filter(|intent| intent.target_package_full_name == package)
            .ok_or(
                "Windows helper has no captured authorization for the current Store generation",
            )?;
        if !backend.source_is_idle(&source.package_full_name)? {
            return Err("Windows helper handoff retains a running old Store generation".into());
        }
        target.package_full_name = package;
        target.registration_id = intent.operation_id.clone();
    }
    let helper = crate::windows_helper::publish_windows_helper(root, executable)?;
    if !backend.compatible(&helper.executable)? {
        return Err(
            "Windows helper candidate does not support the required recovery protocol".into(),
        );
    }
    target.helper_path = helper.executable;
    target.helper_sha256 = helper.sha256;
    target.phase = WindowsInstallPhase::EnabledUnobserved;
    let handoff = Handoff {
        schema_version: 1,
        source: source.clone(),
        target,
        prearm,
        repair,
    };
    validate_handoff(root, &handoff)?;
    Ok(handoff)
}

fn commit_with<B: Backend>(
    root: &Path,
    h: &Handoff,
    backend: &mut B,
) -> Result<B::Observer, String> {
    let _lock = acquire_windows_install_state()?;
    validate_handoff(root, h)?;
    validate_snapshot(root, h)?;
    if read_transient_windows_debug_registration(root)?.is_some() {
        return Err("Windows private launch transaction began before helper commit".into());
    }
    if backend.current_package()? != h.target.package_full_name {
        return Err("Windows Store generation changed before helper commit".into());
    }
    if h.source.package_full_name != h.target.package_full_name
        && !backend.source_is_idle(&h.source.package_full_name)?
    {
        return Err("Windows old Store generation restarted before helper commit".into());
    }
    if !backend.registered(&h.source.helper_path)? && !backend.registered(&h.target.helper_path)? {
        return Err("Windows helper handoff was disabled or superseded".into());
    }
    let already_committed = read_windows_install_state(root)?.as_ref() == Some(&h.target);
    let mut observer = backend.prepare(&h.target)?;
    if already_committed {
        backend.preserve(&mut observer);
    }
    // 已提交代际先保留监听，再写登录项；未提交候选准备失败时不改现有登录入口。
    backend.login(&h.source, &h.target)?;
    backend.enable(&h.target)?;
    let mut enabled = std::collections::BTreeSet::new();
    for intent in migrated(h).into_iter().flatten() {
        let mut prearmed = h.target.clone();
        prearmed.package_full_name = intent.target_package_full_name.clone();
        prearmed.registration_id = intent.operation_id.clone();
        if enabled.insert((
            prearmed.package_full_name.clone(),
            prearmed.registration_id.clone(),
        )) {
            backend.enable(&prearmed)?;
        }
        write_update_repair_intent(root, &intent)?;
    }
    replace_installed_registration_for_handoff(root, &h.source, &h.target)?;
    if backend.current_package()? != h.target.package_full_name {
        return Err("Windows Store generation changed during helper commit".into());
    }
    // 写后检查可能失败但替换已落盘；不再把此候选当作可强杀的未激活进程。
    backend.preserve(&mut observer);
    write_state(root, &h.target)?;
    // 只有已捕获的同 operation 被新来源接手，才退休旧包恢复凭据。
    if h.source.package_full_name != h.target.package_full_name {
        if let Some(intent) = &h.prearm {
            retire_windows_update_prearm_intent(root, Some(&intent.operation_id))?;
        }
        if let Some(intent) = &h.repair {
            retire_windows_update_repair_intent(root, Some(&intent.operation_id))?;
        }
    }
    Ok(observer)
}

fn restore_source<B: Backend>(root: &Path, h: &Handoff, backend: &mut B) -> Result<(), String> {
    {
        let _lock = acquire_windows_install_state()?;
        validate_snapshot(root, h)?;
        if read_windows_install_state(root)?.as_ref() != Some(&h.source) {
            return Err("new helper state committed or unknown; forward recovery retained".into());
        }
        let current = backend.current_package()?;
        if current != h.target.package_full_name {
            return Err("Store changed; previous authorization retained without rollback".into());
        }
        let mut enabled = std::collections::BTreeSet::new();
        if h.source.package_full_name == current {
            backend.enable(&h.source)?;
        }
        for intent in h.prearm.iter().chain(h.repair.iter()) {
            let mut state = h.source.clone();
            state.package_full_name = intent.target_package_full_name.clone();
            state.registration_id = intent.operation_id.clone();
            if enabled.insert((
                state.package_full_name.clone(),
                state.registration_id.clone(),
            )) {
                backend.enable(&state)?;
            }
            write_update_repair_intent(root, intent)?;
        }
        replace_installed_registration_for_handoff(root, &h.target, &h.source)?;
        write_state(root, &h.source)?;
        backend.login(&h.target, &h.source)?;
    }
    // 旧协议仅要求现有订阅回执；不要求旧程序理解新协议的第二阶段信号。
    backend.restart_source(&h.source)
}

fn migrated(h: &Handoff) -> [Option<WindowsUpdateRepairIntent>; 2] {
    if h.source.package_full_name != h.target.package_full_name {
        return [None, None];
    }
    [h.prearm.clone(), h.repair.clone()].map(|intent| {
        intent.map(|mut intent| {
            intent.helper_path = h.target.helper_path.clone();
            intent.helper_sha256 = h.target.helper_sha256.clone();
            intent.runtime_release = h.target.runtime_release.clone();
            intent
        })
    })
}

fn validate_handoff(root: &Path, h: &Handoff) -> Result<(), String> {
    if h.schema_version != 1
        || h.source.epoch != h.target.epoch
        || !h.source.desired_enabled()
        || !h.target.desired_enabled()
        || !matches!(
            h.source.phase,
            WindowsInstallPhase::EnabledObserved | WindowsInstallPhase::EnabledUnobserved
        )
        || h.target.phase != WindowsInstallPhase::EnabledUnobserved
        || h.source.runtime_release != h.target.runtime_release
        || h.source.state_path != root.join(STATE_NAME)
        || h.target.state_path != root.join(STATE_NAME)
    {
        return Err("Windows helper handoff identity is invalid".into());
    }
    validate_state(&h.source, HelperValidation::Required)?;
    validate_state(&h.target, HelperValidation::Required)?;
    verify_installed_windows_runtime(root, &h.target.runtime_release)?;
    for (intent, name) in [
        (&h.prearm, UPDATE_PREARM_INTENT_NAME),
        (&h.repair, UPDATE_REPAIR_INTENT_NAME),
    ] {
        if let Some(intent) = intent {
            validate_update_repair_intent(intent)?;
            if !intent.authorizes_source(&h.source, &intent.target_package_full_name)
                || intent.intent_path != root.join(name)
            {
                return Err("Windows helper journal pending owner is invalid".into());
            }
        }
    }
    if let (Some(a), Some(b)) = (&h.prearm, &h.repair) {
        if a.operation_id != b.operation_id
            || a.target_package_full_name != b.target_package_full_name
        {
            return Err("Windows helper journal operations conflict".into());
        }
    }
    if h.source.package_full_name == h.target.package_full_name {
        if h.source.registration_id != h.target.registration_id {
            return Err("Windows helper changed installation identity".into());
        }
    } else if !h.prearm.iter().chain(h.repair.iter()).any(|i| {
        i.target_package_full_name == h.target.package_full_name
            && i.operation_id == h.target.registration_id
    }) {
        return Err("Windows helper journal cannot adopt the target package".into());
    }
    Ok(())
}

fn validate_snapshot(root: &Path, h: &Handoff) -> Result<(), String> {
    let current =
        read_windows_install_state(root)?.ok_or("Windows helper installation disappeared")?;
    if current != h.source && current != h.target {
        return Err("Windows helper installation changed; journal retained".into());
    }
    let evidence =
        read_windows_debug_registration(root)?.ok_or("Windows helper registration disappeared")?;
    if !registration_matches_install_state(&evidence, &h.source)
        && !registration_matches_install_state(&evidence, &h.target)
    {
        return Err("Windows helper registration changed; journal retained".into());
    }
    let migrated = migrated(h);
    for (old, new, name) in [
        (&h.prearm, &migrated[0], UPDATE_PREARM_INTENT_NAME),
        (&h.repair, &migrated[1], UPDATE_REPAIR_INTENT_NAME),
    ] {
        let actual = read_update_intent(root, name)?;
        if actual != *old && actual != *new {
            return Err("Windows helper pending operation changed; journal retained".into());
        }
        if actual.is_none() && old.is_some() && current != h.target {
            return Err("Windows helper pending operation vanished before commit".into());
        }
    }
    Ok(())
}

fn write_journal(root: &Path, h: &Handoff) -> Result<(), String> {
    let body = serde_json::to_vec_pretty(h).map_err(|e| e.to_string())?;
    replace_private_file(root, &root.join(JOURNAL), &body)
}
fn read_journal(root: &Path) -> Result<Option<Handoff>, String> {
    let path = root.join(JOURNAL);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    crate::windows_file::ensure_regular_file(&path, "Windows helper handoff journal")?;
    verify_private_acl(&path)?;
    if metadata.len() > STATE_LIMIT {
        return Err("Windows helper journal exceeds its bound".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(STATE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > STATE_LIMIT {
        return Err("Windows helper journal exceeds its bound".into());
    }
    let mut h: Handoff = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    h.source.state_path = root.join(STATE_NAME);
    h.target.state_path = root.join(STATE_NAME);
    if let Some(intent) = &mut h.prearm {
        intent.intent_path = root.join(UPDATE_PREARM_INTENT_NAME);
    }
    if let Some(intent) = &mut h.repair {
        intent.intent_path = root.join(UPDATE_REPAIR_INTENT_NAME);
    }
    validate_handoff(root, &h)?;
    Ok(Some(h))
}
fn retire_journal(root: &Path) -> Result<(), String> {
    fs::remove_file(root.join(JOURNAL)).map_err(|e| e.to_string())
}

pub(super) fn retire_cancelled(root: &Path, state: &WindowsInstallState) -> Result<(), String> {
    let Some(h) = read_journal(root)? else {
        return Ok(());
    };
    if state.phase != WindowsInstallPhase::Disabled
        || state.desired_enabled()
        || ![&h.source, &h.target].iter().any(|owner| {
            state.registration_id == owner.registration_id
                && state.package_full_name == owner.package_full_name
                && state.helper_path == owner.helper_path
                && state.helper_sha256 == owner.helper_sha256
                && state.epoch >= owner.epoch
        })
    {
        return Err("Windows cancelled helper journal belongs to another installation".into());
    }
    retire_journal(root)
}

#[cfg(test)]
#[path = "windows_helper_handoff_tests.rs"]
mod tests;
