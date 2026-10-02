// 自动换代只豁免仍被系统挂起的精确启动 PID；句柄在整个安装事务内保持身份稳定。
use std::mem::size_of;
use std::ptr;
use windows_sys::Wdk::System::Threading::{NtQueryInformationThread, ThreadSuspendCount};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Threading::{
    OpenProcess, OpenThread, PROCESS_QUERY_LIMITED_INFORMATION, THREAD_QUERY_INFORMATION,
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub(crate) struct SuspendedLaunch {
    _process: Handle,
    thread: Handle,
    package: String,
    pid: u32,
    tid: u32,
}

impl SuspendedLaunch {
    pub(crate) fn capture(package: &str, pid: u32, tid: u32) -> Result<Self, String> {
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let process = Handle(process);
        let thread = unsafe { OpenThread(THREAD_QUERY_INFORMATION, 0, tid) };
        if thread.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let held = Self {
            _process: process,
            thread: Handle(thread),
            package: package.into(),
            pid,
            tid,
        };
        held.verify()?;
        Ok(held)
    }

    pub(crate) fn verify(&self) -> Result<(), String> {
        crate::windows_process::validate_debugged_package_process(
            &self.package,
            self.pid,
            self.tid,
        )
        .map_err(|error| error.to_string())?;
        let command = crate::windows_process::process_command_line(self.pid)
            .map_err(|error| error.to_string())?;
        if !crate::windows_update_repair::is_primary_package_process(&command)
            || crate::windows_activation::windows_debugger_route(&command)?
                != crate::windows_activation::WindowsDebuggerRoute::ResumeNormally
        {
            return Err("Windows prearm target is not the ordinary primary launch".into());
        }
        let mut count = 0u32;
        let result = unsafe {
            NtQueryInformationThread(
                self.thread.0,
                ThreadSuspendCount,
                (&mut count as *mut u32).cast(),
                size_of::<u32>() as u32,
                ptr::null_mut(),
            )
        };
        if result < 0 || count != 1 {
            return Err(format!(
                "Windows prearm target is not singly suspended (NTSTATUS 0x{:08X}, count {count})",
                result as u32
            ));
        }
        Ok(())
    }
}
