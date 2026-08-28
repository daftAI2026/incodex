use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INVALID_PARAMETER, FILETIME, HANDLE, STILL_ACTIVE,
};
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, GetProcessTimes, OpenProcess,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::{
    apply_private_acl, verify_private_acl, WindowsSessionIdentity, FILE_ATTRIBUTE_REPARSE_POINT,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WindowsSessionOwner {
    pub(super) session_id: String,
    pub(super) pid: u32,
    pub(super) process_creation_time: u64,
    pub(super) identity: WindowsSessionIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WindowsProcessProbe {
    Live(u64),
    Dead,
    Unknown,
}

pub(super) fn write_owner_manifest(path: &Path, owner: &WindowsSessionOwner) -> Result<(), String> {
    let body = serde_json::json!({
        "schema": 1,
        "sessionId": owner.session_id,
        "pid": owner.pid,
        "processCreationTime": owner.process_creation_time.to_string(),
        "volumeSerialNumber": owner.identity.volume_serial_number,
        "fileIndex": owner.identity.file_index.to_string(),
    });
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            format!(
                "cannot create Windows session owner {}: {error}",
                path.display()
            )
        })?;
    let result = file
        .write_all(format!("{body}\n").as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|error| {
            format!(
                "cannot write Windows session owner {}: {error}",
                path.display()
            )
        });
    drop(file);
    if let Err(error) = result {
        let _ = fs::remove_file(path);
        return Err(error);
    }
    if let Err(error) = apply_private_acl(path).and_then(|_| verify_private_acl(path)) {
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(())
}

pub(super) fn read_owner_manifest(path: &Path) -> Result<WindowsSessionOwner, String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| {
            format!(
                "cannot open Windows session owner {}: {error}",
                path.display()
            )
        })?;
    let metadata = file.metadata().map_err(|error| {
        format!(
            "cannot inspect Windows session owner {}: {error}",
            path.display()
        )
    })?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!(
            "Windows session owner is not a plain file: {}",
            path.display()
        ));
    }
    verify_private_acl(path)?;
    let mut body = String::new();
    file.read_to_string(&mut body).map_err(|error| {
        format!(
            "cannot read Windows session owner {}: {error}",
            path.display()
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(&body)
        .map_err(|error| format!("invalid Windows session owner {}: {error}", path.display()))?;
    if value.get("schema").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err("unsupported Windows session owner schema".to_string());
    }
    let session_id = value
        .get("sessionId")
        .and_then(serde_json::Value::as_str)
        .filter(|value| value.starts_with("s-") && value.len() > 2)
        .ok_or("invalid Windows session owner id")?
        .to_string();
    let pid = value
        .get("pid")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or("invalid Windows session owner pid")?;
    let process_creation_time = parse_owner_u64(&value, "processCreationTime")?;
    let volume_serial_number = value
        .get("volumeSerialNumber")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or("invalid Windows session owner volume")?;
    let file_index = parse_owner_u64(&value, "fileIndex")?;
    Ok(WindowsSessionOwner {
        session_id,
        pid,
        process_creation_time,
        identity: WindowsSessionIdentity {
            volume_serial_number,
            file_index,
        },
    })
}

fn parse_owner_u64(value: &serde_json::Value, name: &str) -> Result<u64, String> {
    value
        .get(name)
        .and_then(serde_json::Value::as_str)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| format!("invalid Windows session owner {name}"))
}

pub(super) fn current_process_creation_time() -> Result<u64, String> {
    process_creation_time(unsafe { GetCurrentProcess() })
        .map_err(|error| format!("cannot read current Windows process creation time: {error}"))
}

pub(super) fn probe_process(pid: u32) -> WindowsProcessProbe {
    if pid == 0 {
        return WindowsProcessProbe::Dead;
    }
    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if raw.is_null() {
        return if io::Error::last_os_error().raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32)
        {
            WindowsProcessProbe::Dead
        } else {
            WindowsProcessProbe::Unknown
        };
    }
    let handle = OwnedHandle(raw);
    let mut exit_code = 0;
    if unsafe { GetExitCodeProcess(handle.0, &mut exit_code) } == 0 {
        return WindowsProcessProbe::Unknown;
    }
    if exit_code != STILL_ACTIVE as u32 {
        return WindowsProcessProbe::Dead;
    }
    match process_creation_time(handle.0) {
        Ok(value) => WindowsProcessProbe::Live(value),
        Err(_) => WindowsProcessProbe::Unknown,
    }
}

fn process_creation_time(process: HANDLE) -> Result<u64, io::Error> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    if unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64)
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
