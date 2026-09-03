use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const KEYCHAIN_SERVICE: &str = "Codex Storage Key";
pub const KEYCHAIN_ACCOUNT: &str = "Codex";
pub const MAX_HELPER_OUTPUT_BYTES: usize = 4096;
pub const HELPER_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallerIdentity {
    pub real_uid: u32,
    pub effective_uid: u32,
    pub parent_uid: u32,
    pub parent_executable: PathBuf,
    pub parent_identifier: String,
}

pub fn is_allowed_query(
    service: &str,
    account: &str,
    return_data: bool,
    match_limit_one: bool,
) -> bool {
    service == KEYCHAIN_SERVICE && account == KEYCHAIN_ACCOUNT && return_data && match_limit_one
}

pub fn is_authorized_caller(
    caller: &CallerIdentity,
    expected_parent_executable: &Path,
    expected_identifier: &str,
) -> bool {
    if caller.real_uid != caller.effective_uid
        || caller.parent_uid != caller.effective_uid
        || caller.parent_identifier != expected_identifier
    {
        return false;
    }

    let Ok(actual_executable) = fs::canonicalize(&caller.parent_executable) else {
        return false;
    };
    let Ok(expected_executable) = fs::canonicalize(expected_parent_executable) else {
        return false;
    };
    actual_executable == expected_executable
}

pub fn parse_helper_response(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.is_empty() {
        return Err("macOS Keychain helper returned an empty response".into());
    }
    if bytes.len() > MAX_HELPER_OUTPUT_BYTES {
        return Err("macOS Keychain helper response exceeded its limit".into());
    }
    Ok(bytes.to_vec())
}
