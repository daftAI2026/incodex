use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Component, Path, PathBuf};

use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

pub(crate) fn system_binary_path(relative: impl AsRef<Path>) -> Result<PathBuf, String> {
    let relative = relative.as_ref();
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("Windows system binary path must be a safe relative path".to_string());
    }

    let mut wide = vec![0u16; 260];
    loop {
        let length = unsafe { GetSystemDirectoryW(wide.as_mut_ptr(), wide.len() as u32) };
        if length == 0 {
            return Err(format!(
                "cannot locate the Windows system directory: {}",
                std::io::Error::last_os_error()
            ));
        }
        if length as usize >= wide.len() {
            wide.resize(length as usize + 1, 0);
            continue;
        }
        wide.truncate(length as usize);
        break;
    }

    let executable = PathBuf::from(OsString::from_wide(&wide)).join(relative);
    if executable.is_file() {
        Ok(executable)
    } else {
        Err(format!(
            "system Windows binary is unavailable: {}",
            executable.display()
        ))
    }
}
