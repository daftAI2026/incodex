use std::time::{Duration, Instant};

use incodex_core::quiescence::{
    request_normal_exit_and_wait_with, QuiescenceClock, QuiescenceError, QUIESCENCE_TIMEOUT,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_FAIL_SHUTDOWN, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY,
    ERROR_SEM_TIMEOUT, ERROR_SUCCESS, FILETIME, HANDLE,
};
use windows_sys::Win32::System::Pipes::CallNamedPipeW;
use windows_sys::Win32::System::RestartManager::{
    RmEndSession, RmRegisterResources, RmShutdown, RmStartSession, CCH_RM_SESSION_KEY,
    RM_UNIQUE_PROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::windows_process::running_package_process_ids;

const RUNTIME_CONTROL_PIPE_PREFIX: &str = r"\\.\pipe\Incodex-Runtime-Control-";
const RUNTIME_CONTROL_TIMEOUT_MS: u32 = 1_000;

pub(crate) fn request_official_package_exit_and_wait(
    package_full_name: &str,
    registration_id: Option<&str>,
) -> Result<(), String> {
    let mut clock = SystemClock;
    request_normal_exit_and_wait_with(
        || {
            running_package_process_ids(package_full_name)
                .map_err(|error| format!("cannot inspect running Windows Codex processes: {error}"))
        },
        |process_ids| {
            if request_installed_runtime_normal_exit(registration_id)? {
                Ok(())
            } else {
                request_restart_manager_shutdown(process_ids)
            }
        },
        &mut clock,
    )
    .map_err(|error| format_quiescence_error(package_full_name, error))
}

fn format_quiescence_error(package_full_name: &str, error: QuiescenceError) -> String {
    match error {
        QuiescenceError::Probe(error) => error,
        QuiescenceError::Request(error) => {
            format!("failed to ask official Windows Codex to quit: {error}")
        }
        QuiescenceError::TimedOut => format!(
            "timed out waiting for official Windows Codex package to exit after {} seconds: {package_full_name}",
            QUIESCENCE_TIMEOUT.as_secs()
        ),
    }
}

fn request_installed_runtime_normal_exit(registration_id: Option<&str>) -> Result<bool, String> {
    let Some(registration_id) = registration_id else {
        return Ok(false);
    };
    if registration_id.len() != 32 || !registration_id.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("installed Windows Runtime has an invalid registration identity".to_string());
    }
    let pipe_name = format!(
        "{RUNTIME_CONTROL_PIPE_PREFIX}{}",
        registration_id.to_ascii_lowercase()
    )
    .encode_utf16()
    .chain([0])
    .collect::<Vec<_>>();
    let request = b"quit\n";
    let mut response = [0_u8; 32];
    let mut read = 0;
    if unsafe {
        CallNamedPipeW(
            pipe_name.as_ptr(),
            request.as_ptr().cast(),
            request.len() as u32,
            response.as_mut_ptr().cast(),
            response.len() as u32,
            &mut read,
            RUNTIME_CONTROL_TIMEOUT_MS,
        )
    } != 0
    {
        return if &response[..read as usize] == b"accepted\n" {
            Ok(true)
        } else {
            Err("installed Windows Runtime returned an invalid normal-exit response".to_string())
        };
    }
    let error = unsafe { GetLastError() };
    if matches!(
        error,
        ERROR_FILE_NOT_FOUND | ERROR_PIPE_BUSY | ERROR_SEM_TIMEOUT
    ) {
        Ok(false)
    } else {
        Err(format!(
            "installed Windows Runtime normal-exit request failed: {}",
            std::io::Error::from_raw_os_error(error as i32)
        ))
    }
}

fn request_restart_manager_shutdown(process_ids: &[u32]) -> Result<(), String> {
    let processes = process_ids
        .iter()
        .map(|process_id| restart_manager_process(*process_id))
        .collect::<Result<Vec<_>, _>>()?;
    let session = RestartManagerSession::start()?;
    let registered = unsafe {
        RmRegisterResources(
            session.handle,
            0,
            std::ptr::null(),
            processes.len() as u32,
            processes.as_ptr(),
            0,
            std::ptr::null(),
        )
    };
    require_success(
        "cannot register exact Codex processes for normal shutdown",
        registered,
    )?;

    let shutdown = unsafe { RmShutdown(session.handle, 0, None) };
    if shutdown != ERROR_SUCCESS && shutdown != ERROR_FAIL_SHUTDOWN {
        return Err(format!(
            "Windows Restart Manager normal shutdown request failed with code {shutdown}"
        ));
    }
    session.finish()
}

fn restart_manager_process(process_id: u32) -> Result<RM_UNIQUE_PROCESS, String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    let process = ProcessHandle::new(process).map_err(|error| {
        format!("cannot open Codex process {process_id} for normal shutdown: {error}")
    })?;
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    if unsafe {
        GetProcessTimes(
            process.handle,
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return Err(format!(
            "cannot identify Codex process {process_id} for normal shutdown: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(RM_UNIQUE_PROCESS {
        dwProcessId: process_id,
        ProcessStartTime: created,
    })
}

fn require_success(context: &str, status: u32) -> Result<(), String> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("{context}: Windows error {status}"))
    }
}

struct RestartManagerSession {
    handle: u32,
    open: bool,
}

impl RestartManagerSession {
    fn start() -> Result<Self, String> {
        let mut handle = 0;
        let mut key = vec![0_u16; CCH_RM_SESSION_KEY as usize + 1];
        let status = unsafe { RmStartSession(&mut handle, 0, key.as_mut_ptr()) };
        require_success("cannot start Windows Restart Manager session", status)?;
        Ok(Self { handle, open: true })
    }

    fn finish(mut self) -> Result<(), String> {
        let status = unsafe { RmEndSession(self.handle) };
        self.open = false;
        require_success("cannot finish Windows Restart Manager session", status)
    }
}

impl Drop for RestartManagerSession {
    fn drop(&mut self) {
        if self.open {
            let _ = unsafe { RmEndSession(self.handle) };
        }
    }
}

struct ProcessHandle {
    handle: HANDLE,
}

impl ProcessHandle {
    fn new(handle: HANDLE) -> Result<Self, std::io::Error> {
        if handle.is_null() {
            let code = unsafe { GetLastError() };
            Err(std::io::Error::from_raw_os_error(code as i32))
        } else {
            Ok(Self { handle })
        }
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

struct SystemClock;

impl QuiescenceClock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}
