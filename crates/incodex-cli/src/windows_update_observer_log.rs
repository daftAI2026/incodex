// 单一观察者持有 owner 锁后写入；保留有限事件链，不记录轮询、DOM 或账户数据。
use std::io::Read;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

const MAX_BYTES: usize = 64 * 1024;
const MAX_EVENTS: usize = 128;
const MAX_DETAIL_CHARS: usize = 512;

pub(crate) fn status(root: &Path, phase: &str, detail: &str) -> Result<(), String> {
    let parent = incodex_core::windows_session::ensure_private_windows_dir(&root.join("windows"))?;
    let path = parent.join("update-observer.json");
    let previous = read_history(&path)?;
    let mut events = previous["events"].as_array().cloned().unwrap_or_default();
    let detail: String = detail.chars().take(MAX_DETAIL_CHARS).collect();
    let phase: String = phase.chars().take(64).collect();
    let pid = std::process::id();
    if events.last().is_some_and(|last| {
        last["pid"] == pid && last["phase"] == phase && last["detail"] == detail
    }) {
        return Ok(());
    }
    let current = serde_json::json!({
        "pid": pid, "phase": phase, "detail": detail,
        "unixSeconds": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
    });
    events.push(current.clone());
    if events.len() > MAX_EVENTS {
        events.drain(..events.len() - MAX_EVENTS);
    }
    let mut record = current;
    record["schemaVersion"] = 1.into();
    record["productVersion"] = env!("CARGO_PKG_VERSION").into();
    // 内容寻址目录可与私有实验清单的 helper SHA 对照，不冒充源码内嵌身份。
    record["helperIdentity"] = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.parent()?
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
        .into();
    record["events"] = events.into();
    loop {
        let bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
        if bytes.len() <= MAX_BYTES {
            return crate::windows_runtime::replace_private_file(&parent, &path, &bytes);
        }
        record["events"].as_array_mut().unwrap().remove(0);
    }
}

fn read_history(path: &Path) -> Result<serde_json::Value, String> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
    };
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(serde_json::Value::Null)
        }
        Err(error) => return Err(format!("cannot read observer history: {error}")),
    };
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err("observer history is not a regular file".into());
    }
    incodex_core::windows_session::verify_private_acl(path)?;
    // 损坏或旧版文件只影响诊断历史；读取也受限，不能让大日志耗尽内存。
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Ok(serde_json::Value::Null);
    }
    Ok(serde_json::from_slice(&bytes).unwrap_or_default())
}
