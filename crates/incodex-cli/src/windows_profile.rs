use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::TOKEN_QUERY;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::Shell::GetUserProfileDirectoryW;

pub(crate) fn windows_user_profile() -> Result<PathBuf, String> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(format!(
            "cannot open the current Windows user token: {}",
            std::io::Error::last_os_error()
        ));
    }
    let token = OwnedHandle(token);
    let mut length = 0;
    unsafe {
        GetUserProfileDirectoryW(token.0, std::ptr::null_mut(), &mut length);
    }
    if length == 0 {
        return Err(format!(
            "cannot size the current Windows user profile: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut wide = vec![0u16; length as usize];
    if unsafe { GetUserProfileDirectoryW(token.0, wide.as_mut_ptr(), &mut length) } == 0 {
        return Err(format!(
            "cannot resolve the current Windows user profile: {}",
            std::io::Error::last_os_error()
        ));
    }
    let end = wide
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(wide.len());
    let profile = PathBuf::from(OsString::from_wide(&wide[..end]));
    if profile.is_absolute() {
        Ok(profile)
    } else {
        Err(format!(
            "current Windows user profile is not absolute: {}",
            profile.display()
        ))
    }
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
