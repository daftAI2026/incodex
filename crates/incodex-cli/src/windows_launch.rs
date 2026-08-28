use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    FlushFileBuffers, ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, PIPE_WAIT,
};

use crate::windows_activation_capability::WindowsActivationCapability;
use crate::windows_process::authenticate_process_in_named_job;

const REQUEST_LIMIT: usize = 64;
const RESPONSE_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowsLaunchMode {
    Runtime,
    Cdp,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct WindowsActivationEnvironment {
    pub mode: WindowsLaunchMode,
    pub environment: BTreeMap<String, String>,
}

pub struct WindowsActivationEnvironmentPipe {
    handle: HANDLE,
    name: String,
}

// The pipe owns one exclusive HANDLE. Moving the owner transfers the only
// close responsibility; methods do not share the handle across threads.
unsafe impl Send for WindowsActivationEnvironmentPipe {}

impl WindowsActivationEnvironmentPipe {
    pub fn create(capability: &WindowsActivationCapability) -> Result<Self, String> {
        let name = capability.environment_pipe_name().to_string();
        let wide = name.encode_utf16().chain([0]).collect::<Vec<_>>();
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                RESPONSE_LIMIT as u32,
                REQUEST_LIMIT as u32,
                0,
                std::ptr::null(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(format!(
                "cannot create Windows activation environment pipe: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self { handle, name })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn respond_once(
        self,
        job_name: &str,
        package_full_name: &str,
        response: &WindowsActivationEnvironment,
    ) -> Result<u32, String> {
        if unsafe { ConnectNamedPipe(self.handle, std::ptr::null_mut()) } == 0
            && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED
        {
            return Err(format!(
                "cannot accept Windows activation environment client: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut process_id = 0;
        if unsafe { GetNamedPipeClientProcessId(self.handle, &mut process_id) } == 0 {
            return Err(format!(
                "cannot identify Windows activation environment client: {}",
                std::io::Error::last_os_error()
            ));
        }
        authenticate_process_in_named_job(job_name, package_full_name, process_id)
            .map_err(|error| format!("Windows activation Job client was refused: {error}"))?;
        let mut request = [0u8; REQUEST_LIMIT];
        let mut read = 0;
        if unsafe {
            ReadFile(
                self.handle,
                request.as_mut_ptr(),
                request.len() as u32,
                &mut read,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(format!(
                "cannot read Windows activation environment request: {}",
                std::io::Error::last_os_error()
            ));
        }
        if &request[..read as usize] != b"environment\n" {
            return Err("Windows activation environment request is invalid".to_string());
        }
        validate_environment(response)?;
        let body = serde_json::to_vec(response)
            .map_err(|error| format!("cannot serialize Windows activation environment: {error}"))?;
        if body.len() > RESPONSE_LIMIT {
            return Err("Windows activation environment exceeds the size limit".to_string());
        }
        let mut written = 0;
        if unsafe {
            WriteFile(
                self.handle,
                body.as_ptr(),
                body.len() as u32,
                &mut written,
                std::ptr::null_mut(),
            )
        } == 0
            || written as usize != body.len()
        {
            return Err(format!(
                "cannot write Windows activation environment response: {}",
                std::io::Error::last_os_error()
            ));
        }
        if unsafe { FlushFileBuffers(self.handle) } == 0 {
            return Err(format!(
                "cannot flush Windows activation environment response: {}",
                std::io::Error::last_os_error()
            ));
        }
        unsafe { DisconnectNamedPipe(self.handle) };
        Ok(process_id)
    }
}

impl Drop for WindowsActivationEnvironmentPipe {
    fn drop(&mut self) {
        if self.handle != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.handle) };
            self.handle = INVALID_HANDLE_VALUE;
        }
    }
}

fn validate_environment(response: &WindowsActivationEnvironment) -> Result<(), String> {
    for (name, value) in &response.environment {
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
            || value.contains('\0')
        {
            return Err(format!(
                "Windows activation environment entry {name:?} is invalid"
            ));
        }
    }
    Ok(())
}
