/*
 * [INPUT]: 依赖调用方验证过的 helper、固定观察器入口与既有进程创建标志。
 * [OUTPUT]: 创建不继承父句柄的观察器，保留原有 PID、退出与候选清理语义。
 * [POS]: 观察器的进程边界；后台监听不能延长 CLI 管道或安装锁的寿命。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, GetExitCodeProcess, TerminateProcess, WaitForSingleObject, INFINITE,
    PROCESS_INFORMATION, STARTUPINFOW,
};

pub(super) struct ObserverProcess {
    handle: OwnedHandle,
    pid: u32,
}

impl AsRawHandle for ObserverProcess {
    fn as_raw_handle(&self) -> RawHandle {
        self.handle.as_raw_handle()
    }
}

impl ObserverProcess {
    pub(super) fn id(&self) -> u32 {
        self.pid
    }

    pub(super) fn try_wait(&mut self) -> io::Result<Option<u32>> {
        match unsafe { WaitForSingleObject(self.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => self.exit_code().map(Some),
            WAIT_TIMEOUT => Ok(None),
            _ => Err(io::Error::last_os_error()),
        }
    }

    pub(super) fn kill(&mut self) -> io::Result<()> {
        if self.try_wait()?.is_some() {
            return Ok(());
        }
        if unsafe { TerminateProcess(self.as_raw_handle(), 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn wait(&mut self) -> io::Result<u32> {
        match unsafe { WaitForSingleObject(self.as_raw_handle(), INFINITE) } {
            WAIT_OBJECT_0 => self.exit_code(),
            _ => Err(io::Error::last_os_error()),
        }
    }

    fn exit_code(&self) -> io::Result<u32> {
        let mut code = 0;
        if unsafe { GetExitCodeProcess(self.as_raw_handle(), &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(code)
    }
}

pub(super) fn spawn(helper: &Path, mode: &str) -> Result<ObserverProcess, String> {
    spawn_with_flags(helper, mode, super::observer_creation_flags())
}

fn spawn_with_flags(helper: &Path, mode: &str, flags: u32) -> Result<ObserverProcess, String> {
    if !matches!(mode, super::MODE | super::PREPARED_MODE) {
        return Err("invalid Windows observer entry".into());
    }
    let image: Vec<u16> = helper.as_os_str().encode_wide().chain([0]).collect();
    if image[..image.len() - 1]
        .iter()
        .any(|c| *c == 0 || *c == u16::from(b'"'))
    {
        return Err("invalid Windows observer image path".into());
    }
    let mut command = vec![u16::from(b'"')];
    command.extend(helper.as_os_str().encode_wide());
    command.extend([u16::from(b'"'), u16::from(b' ')]);
    command.extend(mode.encode_utf16().chain([0]));
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // NUL 标准流只替换三个句柄，不能阻止其余可继承句柄进入长期后台进程。
    // 此子进程通过命名事件和私有文件通信，任何父进程句柄都无需继承。
    if unsafe {
        CreateProcessW(
            image.as_ptr(),
            command.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            flags,
            ptr::null(),
            ptr::null(),
            &startup,
            &mut info,
        )
    } == 0
    {
        return Err(format!(
            "cannot start Windows update observer: {}",
            io::Error::last_os_error()
        ));
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(info.hProcess) };
    let thread = unsafe { OwnedHandle::from_raw_handle(info.hThread) };
    drop(thread);
    Ok(ObserverProcess {
        handle,
        pid: info.dwProcessId,
    })
}

#[cfg(test)]
#[path = "windows_observer_process_tests.rs"]
mod tests;
