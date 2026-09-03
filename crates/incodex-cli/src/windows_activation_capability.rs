use std::path::Path;
use std::ptr;

use sha2::{Digest, Sha256};
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};
use windows_sys::Win32::UI::Shell::CommandLineToArgvW;

const USER_DATA_PREFIX: &str = "--user-data-dir=";
const JOB_PREFIX: &str = r"Local\Incodex-";
const ENVIRONMENT_PIPE_PREFIX: &str = r"\\.\pipe\Incodex-Activation-Environment-";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsDebuggerRoute {
    ResumeNormally,
    AssignToJob(String),
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsActivationCapability {
    token: String,
    job_name: String,
    environment_pipe_name: String,
}

impl WindowsActivationCapability {
    pub fn create() -> Result<Self, String> {
        Ok(Self::from_token(random_token()?))
    }

    pub fn from_user_data_dir(user_data_dir: &str) -> Result<Self, String> {
        validate_isolated_user_data_dir(user_data_dir)?;
        let token = Sha256::digest(user_data_dir.as_bytes())[..16]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Ok(Self::from_token(token))
    }

    fn from_token(token: String) -> Self {
        Self {
            job_name: format!("{JOB_PREFIX}{token}"),
            environment_pipe_name: format!("{ENVIRONMENT_PIPE_PREFIX}{token}"),
            token,
        }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn job_name(&self) -> &str {
        &self.job_name
    }

    pub fn environment_pipe_name(&self) -> &str {
        &self.environment_pipe_name
    }
}

fn random_token() -> Result<String, String> {
    let mut random = [0u8; 16];
    let status = unsafe {
        BCryptGenRandom(
            ptr::null_mut(),
            random.as_mut_ptr(),
            random.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err(format!(
            "cannot create Windows activation token: NTSTATUS 0x{:08X}",
            status as u32
        ));
    }
    Ok(random.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn activation_capability_from_command_line(
    command_line: &str,
) -> Result<Option<WindowsActivationCapability>, String> {
    if command_line.contains('\0') {
        return Err("Windows activation command line contains NUL".to_string());
    }
    let wide = command_line.encode_utf16().chain([0]).collect::<Vec<_>>();
    let mut count = 0;
    let argv = unsafe { CommandLineToArgvW(wide.as_ptr(), &mut count) };
    if argv.is_null() {
        return Err("cannot parse Windows activation command line".to_string());
    }
    let mut capability = None;
    for index in 0..count {
        let argument = unsafe { *argv.add(index as usize) };
        let mut length = 0;
        while unsafe { *argument.add(length) } != 0 {
            length += 1;
        }
        let argument =
            String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(argument, length) });
        if let Some(value) = argument.strip_prefix(USER_DATA_PREFIX) {
            let Ok(parsed) = WindowsActivationCapability::from_user_data_dir(value) else {
                continue;
            };
            if capability.replace(parsed).is_some() {
                unsafe { LocalFree(argv.cast()) };
                return Err(
                    "Windows activation command line repeats its user data directory".to_string(),
                );
            }
        }
    }
    unsafe { LocalFree(argv.cast()) };
    Ok(capability)
}

pub fn windows_debugger_route(command_line: &str) -> Result<WindowsDebuggerRoute, String> {
    match activation_capability_from_command_line(command_line)? {
        Some(capability) => Ok(WindowsDebuggerRoute::AssignToJob(
            capability.job_name().to_string(),
        )),
        None => Ok(WindowsDebuggerRoute::ResumeNormally),
    }
}

pub fn windows_transient_debugger_route(
    expected_job_name: &str,
    command_line: &str,
) -> Result<WindowsDebuggerRoute, String> {
    match windows_debugger_route(command_line)? {
        WindowsDebuggerRoute::AssignToJob(job_name) if job_name == expected_job_name => {
            Ok(WindowsDebuggerRoute::AssignToJob(job_name))
        }
        _ => Ok(WindowsDebuggerRoute::Reject),
    }
}

fn validate_isolated_user_data_dir(value: &str) -> Result<(), String> {
    if value.contains('\0') {
        return Err("Windows activation user data directory contains NUL".to_string());
    }
    let path = Path::new(value);
    let chromium = path.file_name().and_then(|name| name.to_str());
    let session = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str());
    let sessions = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .and_then(|name| name.to_str());
    let incodex = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .and_then(|name| name.to_str());
    if !path.is_absolute()
        || !chromium.is_some_and(|name| name.eq_ignore_ascii_case("chromium"))
        || !session.is_some_and(|name| name.starts_with("s-") && name.len() > 2)
        || !sessions.is_some_and(|name| name.eq_ignore_ascii_case("sessions"))
        || !incodex.is_some_and(|name| name.eq_ignore_ascii_case(".incodex"))
    {
        return Err(
            "Windows activation user data directory is not an Incodex session profile".to_string(),
        );
    }
    Ok(())
}
