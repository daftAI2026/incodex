use std::ptr;

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};
use windows_sys::Win32::UI::Shell::CommandLineToArgvW;

const TOKEN_PREFIX: &str = "--incodex-activation-token=";
const DEBUGGER_PIPE_PREFIX: &str = r"\\.\pipe\Incodex-Activation-Debugger-";
const ENVIRONMENT_PIPE_PREFIX: &str = r"\\.\pipe\Incodex-Activation-Environment-";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsActivationCapability {
    token: String,
    debugger_pipe_name: String,
    environment_pipe_name: String,
}

impl WindowsActivationCapability {
    pub fn create() -> Result<Self, String> {
        let token = random_token()?;
        Ok(Self {
            debugger_pipe_name: format!("{DEBUGGER_PIPE_PREFIX}{token}"),
            environment_pipe_name: format!("{ENVIRONMENT_PIPE_PREFIX}{token}"),
            token,
        })
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn command_line_argument(&self) -> String {
        format!("{TOKEN_PREFIX}{}", self.token)
    }

    pub fn debugger_pipe_name(&self) -> &str {
        &self.debugger_pipe_name
    }

    pub fn environment_pipe_name(&self) -> &str {
        &self.environment_pipe_name
    }
}

pub fn activation_token_from_command_line(command_line: &str) -> Result<Option<String>, String> {
    if command_line.contains('\0') {
        return Err("Windows activation command line contains NUL".to_string());
    }
    let wide = command_line.encode_utf16().chain([0]).collect::<Vec<_>>();
    let mut count = 0;
    let argv = unsafe { CommandLineToArgvW(wide.as_ptr(), &mut count) };
    if argv.is_null() {
        return Err("cannot parse Windows activation command line".to_string());
    }
    let mut token = None;
    for index in 0..count {
        let argument = unsafe { *argv.add(index as usize) };
        let mut length = 0;
        while unsafe { *argument.add(length) } != 0 {
            length += 1;
        }
        let argument = String::from_utf16_lossy(unsafe {
            std::slice::from_raw_parts(argument, length)
        });
        if let Some(value) = argument.strip_prefix(TOKEN_PREFIX) {
            if !valid_token(value) {
                unsafe { LocalFree(argv.cast()) };
                return Err("Windows activation token is invalid".to_string());
            }
            if token.replace(value.to_string()).is_some() {
                unsafe { LocalFree(argv.cast()) };
                return Err("Windows activation command line repeats its token".to_string());
            }
        }
    }
    unsafe { LocalFree(argv.cast()) };
    Ok(token)
}

fn valid_token(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
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
