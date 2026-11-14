/*
 * [INPUT]: 依赖不继承父句柄的观察器进程、原有事件与有界闭集日志。
 * [OUTPUT]: 提供不递归续接的候选订阅、精确激活 ACK，以及未提交候选的自有资源清理。
 * [POS]: 观察器的进程交接协议子模块；不执行包恢复，也不拥有宿主或会话。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::process::{spawn as spawn_observer, ObserverProcess};
use super::*;
use std::io::Read;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

pub(super) const OBSERVER_LOG_LIMIT: usize = 64 * 1024;

pub(crate) struct PreparedObserver {
    child: ObserverProcess,
    ready: Arc<OwnedHandle>,
    root: std::path::PathBuf,
    helper_identity: String,
    registration_id: String,
    preserved: bool,
}

impl PreparedObserver {
    pub(crate) fn preserve(&mut self) {
        self.preserved = true;
    }

    fn wait_for_ack(&mut self, phase: &str, detail: &str) -> Result<(), String> {
        use std::os::windows::io::AsRawHandle;

        let handles = [self.ready.0, self.child.as_raw_handle() as HANDLE];
        let result = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, START_TIMEOUT_MS) };
        if result != WAIT_OBJECT_0 {
            return Err(if result == WAIT_OBJECT_0 + 1 {
                "Windows update observer exited before its authorization acknowledgement".into()
            } else if result == WAIT_TIMEOUT {
                "Windows update observer authorization acknowledgement timed out".into()
            } else {
                system_error(
                    "cannot wait for Windows update observer authorization acknowledgement",
                )
            });
        }
        if self
            .child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err("Windows update observer exited after its readiness event".into());
        }
        if !observer_status_has_event(
            &self.root,
            self.child.id(),
            &self.helper_identity,
            phase,
            detail,
        )? {
            return Err("Windows update observer readiness log did not match its child".into());
        }
        if self
            .child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err("Windows update observer exited before acknowledgement".into());
        }
        Ok(())
    }

    pub(crate) fn wait_active(mut self) -> Result<u32, String> {
        self.preserve();
        let registration_id = self.registration_id.clone();
        self.wait_for_ack("helper-generation-active", &registration_id)?;
        Ok(self.child.id())
    }
}

impl Drop for PreparedObserver {
    fn drop(&mut self) {
        if !self.preserved {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

pub(crate) fn prepare(state: &WindowsInstallState) -> Result<PreparedObserver, String> {
    // 调用方持安装锁，且已在锁外停止旧观察器；不能在这里反向等待旧 owner。
    spawn_prepared_observer(state, PREPARED_MODE)
}

pub(crate) fn start(state: &WindowsInstallState) -> Result<(), String> {
    let root = crate::windows_activation::installed_debugger_user_root(&state.helper_path)?;
    stop(&root)?;
    let mut observer = spawn_prepared_observer(state, MODE)?;
    // 普通 install 保持旧合同：订阅 ACK 足够，不等待代际 ACK。
    observer.preserve();
    Ok(())
}

fn spawn_prepared_observer(
    state: &WindowsInstallState,
    mode: &str,
) -> Result<PreparedObserver, String> {
    let helper = state.helper_path.as_path();
    let root = crate::windows_activation::installed_debugger_user_root(helper)?;
    if !crate::windows_helper::installed_windows_helper_path_matches(
        &root,
        &state.helper_sha256,
        helper,
    ) {
        return Err("Windows update observer helper path is not content-addressed".into());
    }
    crate::windows_helper::verify_helper(helper, &state.helper_sha256)?;
    let helper_identity = helper
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .ok_or("Windows update observer helper identity is unavailable")?
        .to_string();
    let ready = event(&root, "ready", true)?;
    if unsafe { ResetEvent(ready.0) } == 0 {
        return Err(system_error("cannot reset observer readiness"));
    }
    let child = spawn_observer(helper, mode)?;
    let mut observer = PreparedObserver {
        child,
        ready,
        root,
        helper_identity,
        registration_id: state.registration_id.clone(),
        preserved: false,
    };
    observer.wait_for_ack("subscribed", "startup reconciliation pending")?;
    if unsafe { ResetEvent(observer.ready.0) } == 0 {
        return Err(system_error(
            "cannot reset observer readiness for generation acknowledgement",
        ));
    }
    Ok(observer)
}

fn observer_status_has_event(
    root: &Path,
    pid: u32,
    helper_identity: &str,
    phase: &str,
    detail: &str,
) -> Result<bool, String> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
    };

    let path = root.join("windows/update-observer.json");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&path)
        .map_err(|error| format!("cannot open bounded observer status: {error}"))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.len() > OBSERVER_LOG_LIMIT as u64
    {
        return Err("observer status is not a bounded regular file".into());
    }
    incodex_core::windows_session::verify_private_acl(&path)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((OBSERVER_LOG_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    observer_log_has_event(&bytes, pid, helper_identity, phase, detail)
}

pub(super) fn observer_log_has_event(
    bytes: &[u8],
    pid: u32,
    helper_identity: &str,
    phase: &str,
    detail: &str,
) -> Result<bool, String> {
    if bytes.len() > OBSERVER_LOG_LIMIT {
        return Err("observer status exceeds its bound".into());
    }
    let record: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "observer status is invalid".to_string())?;
    if record["schemaVersion"].as_u64() != Some(1) {
        return Err("observer status schema is invalid".into());
    }
    let events = record["events"]
        .as_array()
        .filter(|events| events.len() <= 128)
        .ok_or_else(|| "observer status event history is invalid".to_string())?;
    Ok(events.iter().any(|event| {
        event["pid"].as_u64() == Some(u64::from(pid))
            && event["helperIdentity"].as_str() == Some(helper_identity)
            && event["phase"].as_str() == Some(phase)
            && event["detail"].as_str() == Some(detail)
    }))
}
