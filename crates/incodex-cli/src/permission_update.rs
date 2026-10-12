/*
 * [INPUT]: 依赖已获权更新登记、已发布 Runtime 和当前宿主的权限引导。
 * [OUTPUT]: 更新后有界预检；真实缺失才弹卡，人的授权不倒计时。
 * [POS]: Runtime 发布与官方恢复共用的后置步骤，不启动或重签宿主。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::accessibility_guide_host::{run_permission_update_guide, Outcome};
use crate::macos_update_assets::{
    read_registration, read_regular_file, sha256_hex, UpdateRegistration,
};
use incodex_core::paths::{user_root, DEFAULT_APP};
use serde::{Deserialize, Serialize};

const TASK_ENV: &str = "INCODEX_INTERNAL_PERMISSION_UPDATE";
const HOST_START_TIMEOUT: Duration = Duration::from_secs(60);
const HOST_POLL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Source {
    Runtime,
    OfficialUpdate,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Task {
    schema_version: u8,
    install_id: String,
    helper_sha256: String,
    runtime_sha256: String,
    source: Source,
}
impl Task {
    fn parse(raw: &str) -> Result<Self, String> {
        if raw.len() > 4096 {
            return Err("invalid permission update task".into());
        }
        let task: Self =
            serde_json::from_str(raw).map_err(|_| "invalid permission update task".to_string())?;
        if task.schema_version != 1
            || task.install_id.is_empty()
            || task.install_id.len() > 128
            || !task
                .install_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
            || !crate::macos_update_assets::is_sha256(&task.helper_sha256)
            || !crate::macos_update_assets::is_sha256(&task.runtime_sha256)
        {
            return Err("invalid permission update task".into());
        }
        Ok(task)
    }
    fn matches(&self, registration: &UpdateRegistration) -> bool {
        registration.app_path == Path::new(DEFAULT_APP)
            && registration.install_id == self.install_id
            && registration.helper_sha256 == self.helper_sha256
    }
}

/// 登记缺失时不创建任务。环境变量只传递绑定值，不构成授权。
pub(crate) fn schedule(source: Source) {
    let root = user_root();
    if let Err(_error) = schedule_with_root(&root, source) {
        crate::macos_update_log::log_coordinator_event(
            &root,
            "permission-update scheduling-failed",
        );
    }
}
fn schedule_with_root(root: &Path, source: Source) -> Result<(), String> {
    let Some(registration) = read_registration(root)? else {
        return Ok(());
    };
    if registration.app_path != Path::new(DEFAULT_APP) {
        return Ok(());
    }
    let executable = std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|e| format!("cannot locate permission update publisher: {e}"))?;
    if sha256_hex(&read_regular_file(
        &executable,
        "permission update publisher",
    )?) != registration.helper_sha256
    {
        return Err("permission update helper was not refreshed".into());
    }
    if !incodex_runtime_bundle::deployed_current_matches_embedded(root)? {
        return Err("permission update Runtime selection changed".into());
    }
    let task = Task {
        schema_version: 1,
        install_id: registration.install_id.clone(),
        helper_sha256: registration.helper_sha256.clone(),
        runtime_sha256: incodex_runtime_bundle::runtime_identity()?.manifest_sha256,
        source,
    };
    // 子进程重新验证登记、字节与安装证明；不继承恢复模式或开发覆盖。
    verify_registered_helper(root, &registration)?;
    let mut child = task_command(&registration.helper_path, &task)?
        .spawn()
        .map_err(|e| format!("cannot start permission update check: {e}"))?;
    crate::macos_update_log::log_coordinator_event(root, "permission-update scheduled");
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
fn task_command(helper: &Path, task: &Task) -> Result<Command, String> {
    let mut command = Command::new(helper);
    command
        .env_remove("INCODEX_MACOS_UPDATE_RELAUNCH")
        .env_remove("INCODEX_INTERNAL_UPDATE_NOTICE_WORKER")
        .env_remove("INCODEX_INTERNAL_RUNTIME_SYNC")
        .env_remove("INCODEX_DEV_HOT")
        .env(
            TASK_ENV,
            serde_json::to_string(task).map_err(|e| e.to_string())?,
        )
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(command)
}

pub(crate) fn try_run() -> Option<Result<(), String>> {
    let raw = std::env::var_os(TASK_ENV)?;
    Some(
        raw.to_str()
            .ok_or("invalid permission update task".to_string())
            .and_then(Task::parse)
            .and_then(|task| run_task(&user_root(), &task)),
    )
}
fn authorized_registration(root: &Path, task: &Task) -> Result<UpdateRegistration, String> {
    let registration =
        read_registration(root)?.ok_or("permission update registration is missing")?;
    if !task.matches(&registration) {
        return Err("permission update registration changed".into());
    }
    crate::macos_update_assets::verify_running_helper(
        root,
        &registration,
        Some(&task.helper_sha256),
    )?;
    if task.runtime_sha256 != incodex_runtime_bundle::runtime_identity()?.manifest_sha256
        || !incodex_runtime_bundle::deployed_current_matches_embedded(root)?
    {
        return Err("permission update Runtime generation changed".into());
    }
    Ok(registration)
}
fn verify_registered_helper(root: &Path, registration: &UpdateRegistration) -> Result<(), String> {
    let expected = root
        .join("helpers/macos-update")
        .join(&registration.helper_sha256)
        .join("incodex");
    if registration.helper_path != expected
        || sha256_hex(&read_regular_file(&expected, "permission update helper")?)
            != registration.helper_sha256
    {
        return Err("permission update helper failed verification".into());
    }
    Ok(())
}
fn run_task(root: &Path, task: &Task) -> Result<(), String> {
    // 验证先于锁和日志，伪造请求不能创建产品状态。
    let registration = authorized_registration(root, task)?;
    let app = Path::new(DEFAULT_APP);
    let _lock = crate::accessibility_setup::acquire_guide_owner(root, app)?;
    authorized_registration(root, task)?;
    let mut app_verifier = crate::accessibility_target::verifier(
        app,
        crate::accessibility_target::supports_permission_continuity(app),
        || {
            incodex_transaction::validate_committed_live_snapshot(root, &task.install_id, app)
                .map_err(|e| e.to_string())?;
            crate::macos_signing::verify_patched(root, app, None).map(|_| ())
        },
    );
    app_verifier()?;
    let Some(_) = wait_for_host(app, task.source) else {
        crate::macos_update_log::log_coordinator_event(
            root,
            "permission-update host-unavailable pending",
        );
        return Ok(());
    };
    let request_id = crate::accessibility_setup::request_setup(root, app, &task.install_id)?;
    let result = run_permission_update_guide(root, app, || {
        // 当前流程保留原资产与请求。新 CLI/Runtime 发布不打断已打开的卡。
        // 安装 epoch 与签名变化仍拒绝。PID 由每份新快照独立验证。
        let current = read_registration(root)?.ok_or("permission update authorization retired")?;
        if current.install_id != registration.install_id
            || current.app_path != registration.app_path
            || current.signing_certificate_sha256 != registration.signing_certificate_sha256
        {
            return Err("permission update install generation changed".into());
        }
        app_verifier()
    });
    let state = match &result {
        Ok(Outcome::Granted) => "granted",
        Ok(Outcome::Pending) => "deferred",
        Err(_) => "error",
    };
    crate::accessibility_setup::finish_cli_setup(root, app, &task.install_id, &request_id, state)?;
    crate::macos_update_log::log_coordinator_event(root, &format!("permission-update {state}"));
    result.map(|_| ())
}
fn wait_for_host(app: &Path, source: Source) -> Option<(i32, String)> {
    let deadline = Instant::now() + HOST_START_TIMEOUT;
    loop {
        let report = incodex_macos::inspect_accessibility_for_app(app);
        if let Some(pid) = report.pid {
            if let Some(start) = incodex_core::session::process_start_identity(pid) {
                return Some((pid, start));
            }
        }
        // Runtime 更新不打开已关闭的 App；官方恢复留时间给既有 coordinator 重开。
        if source == Source::Runtime || Instant::now() >= deadline {
            return None;
        }
        thread::sleep(HOST_POLL.min(deadline.saturating_duration_since(Instant::now())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn task() -> Task {
        Task {
            schema_version: 1,
            install_id: "install-a".into(),
            helper_sha256: "a".repeat(64),
            runtime_sha256: "b".repeat(64),
            source: Source::Runtime,
        }
    }
    #[test]
    fn task_round_trip_and_rejects_unknown_authority() {
        let raw = serde_json::to_string(&task()).unwrap();
        assert_eq!(Task::parse(&raw).unwrap().source, Source::Runtime);
        for bad in [
            raw.replace("\"schemaVersion\":1", "\"schemaVersion\":2"),
            raw.replace("\"source\":\"runtime\"", "\"source\":\"install\""),
            raw.replace("install-a", "../escape"),
            raw.replace("install-a", ""),
            raw.replace('}', ",\"appPath\":\"/tmp/Other.app\"}"),
        ] {
            assert!(Task::parse(&bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn child_has_only_bound_task_and_clears_relaunch_modes() {
        let command = task_command(Path::new("/private/helper/incodex"), &task()).unwrap();
        assert_eq!(command.get_program(), "/private/helper/incodex");
        assert_eq!(command.get_args().count(), 0);
        let env: std::collections::BTreeMap<_, _> = command
            .get_envs()
            .map(|(k, v)| (k.to_str().unwrap(), v.and_then(|v| v.to_str())))
            .collect();
        for key in [
            "INCODEX_MACOS_UPDATE_RELAUNCH",
            "INCODEX_INTERNAL_UPDATE_NOTICE_WORKER",
            "INCODEX_DEV_HOT",
        ] {
            assert_eq!(env[key], None);
        }
        let actual = Task::parse(env[TASK_ENV].unwrap()).unwrap();
        assert_eq!(actual.install_id, task().install_id);
        assert_eq!(actual.helper_sha256, task().helper_sha256);
        assert_eq!(actual.runtime_sha256, task().runtime_sha256);
    }
    #[test]
    fn oversized_task_is_rejected_even_when_json_is_valid() {
        let raw = " ".repeat(4097) + &serde_json::to_string(&task()).unwrap();
        assert!(Task::parse(&raw).is_err());
    }
}
