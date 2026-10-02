use std::path::Path;
use std::ptr;
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RRF_RT_REG_SZ,
};

use crate::windows_install_state::WindowsInstallState;

const RUN_VALUE_NAME: &str = "IncodexUpdateRepair";
const OBSERVER_ARGUMENT: &str = crate::windows_update_observer::MODE;
const RUN_COMMAND_LIMIT: usize = 260;
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
fn registry_error(code: u32) -> String {
    format!(
        "Windows update observer login entry: {}",
        std::io::Error::from_raw_os_error(code as i32)
    )
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

fn read_value(name: &str) -> Result<Option<String>, String> {
    let mut size = 0;
    let key = wide(RUN_KEY);
    let name = wide(name);
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut size,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if result != ERROR_SUCCESS {
        return Err(registry_error(result));
    }
    if !(2..=64 * 1024).contains(&size) || size % 2 != 0 {
        return Err("Invalid update observer Run value size".into());
    }
    let mut buffer = vec![0u16; size as usize / 2];
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if result != ERROR_SUCCESS {
        return Err(registry_error(result));
    }
    buffer.truncate(size as usize / 2);
    if buffer.pop() != Some(0) || buffer.contains(&0) {
        return Err("Invalid update observer Run string".into());
    }
    String::from_utf16(&buffer)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn write_value(name: &str, command: &str) -> Result<(), String> {
    let mut key = ptr::null_mut();
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            0,
            ptr::null(),
            0,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            ptr::null(),
            &mut key,
            ptr::null_mut(),
        )
    };
    if result != ERROR_SUCCESS {
        return Err(registry_error(result));
    }
    let key = Key(key);
    let body = wide(command);
    let result = unsafe {
        RegSetValueExW(
            key.0,
            wide(name).as_ptr(),
            0,
            REG_SZ,
            body.as_ptr().cast(),
            (body.len() * 2) as u32,
        )
    };
    if result != ERROR_SUCCESS {
        return Err(registry_error(result));
    }
    if read_value(name)?.as_deref() != Some(command) {
        return Err("Observer login entry verification failed".into());
    }
    Ok(())
}

fn delete_value(name: &str) -> Result<(), String> {
    let mut key = ptr::null_mut();
    let result = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            0,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            &mut key,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    if result != ERROR_SUCCESS {
        return Err(registry_error(result));
    }
    let key = Key(key);
    let result = unsafe { RegDeleteValueW(key.0, wide(name).as_ptr()) };
    if result != ERROR_SUCCESS && result != ERROR_FILE_NOT_FOUND {
        return Err(registry_error(result));
    }
    Ok(())
}

fn prove_owned_command(command: &str) -> Result<(), String> {
    let path = command
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix(&format!("\" {OBSERVER_ARGUMENT}")))
        .ok_or_else(|| "Refusing an unrecognized IncodexUpdateRepair login value".to_string())?;
    let helper = Path::new(path);
    if build_run_command(helper)? != command {
        return Err("Observer login command is not canonical".into());
    }
    incodex_core::windows_path::reject_reparse_ancestors(helper)?;
    let helper = crate::windows_file::canonical_regular_file(helper, "update observer helper")?;
    let root = crate::windows_activation::installed_debugger_user_root(&helper)?;
    let expected = crate::windows_profile::windows_user_profile()?.join(".incodex");
    let expected = std::fs::canonicalize(expected).map_err(|error| error.to_string())?;
    if root != expected {
        return Err("Observer login helper is outside the current user profile".into());
    }
    incodex_core::windows_session::verify_private_acl(&helper)?;
    let hash = crate::windows_file::sha256_file(&helper)?;
    if !crate::windows_helper::installed_windows_helper_path_matches(&root, &hash, &helper) {
        return Err("Observer login helper content address does not match".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingRunValue {
    Missing,
    Owned,
    Foreign,
}

pub(crate) fn register(state: &WindowsInstallState) -> Result<(), String> {
    let _gate = crate::windows_install_state::acquire_windows_install_state()?;
    let root = crate::windows_activation::installed_debugger_user_root(&state.helper_path)?;
    if crate::windows_install_state::read_windows_install_state(&root)?.as_ref() != Some(state)
        || !state.desired_enabled()
    {
        return Err("Windows install authorization changed before observer registration".into());
    }
    let command = build_run_command(&state.helper_path)?;
    prove_owned_command(&command)?;
    let existing = read_value(RUN_VALUE_NAME)?;
    if classify_run_value(existing.as_deref(), &command) == ExistingRunValue::Foreign {
        prove_owned_command(existing.as_deref().unwrap())?;
    }
    write_value(RUN_VALUE_NAME, &command)
}

pub(crate) fn remove() -> Result<(), String> {
    let _gate = crate::windows_install_state::acquire_windows_install_state()?;
    if let Some(existing) = read_value(RUN_VALUE_NAME)? {
        let root = crate::windows_profile::windows_user_profile()?.join(".incodex");
        prove_removable_command(&existing, &root)?;
        delete_value(RUN_VALUE_NAME)?;
    }
    Ok(())
}

fn prove_removable_command(command: &str, root: &Path) -> Result<(), String> {
    // 卸载沿用现有持久身份证据，不要求已丢失的 helper 重新出现。
    let state = crate::windows_install_state::read_windows_install_state_for_uninstall(root)
        .ok()
        .flatten();
    let registration = crate::windows_registration::read_windows_debug_registration(root)
        .ok()
        .flatten();
    for helper in state
        .as_ref()
        .map(|state| &state.helper_path)
        .into_iter()
        .chain(registration.as_ref().map(|entry| &entry.helper_path))
    {
        if build_run_command(helper).is_ok_and(|owned| owned == command) {
            return Ok(());
        }
    }
    prove_owned_command(command)
}

pub(crate) fn is_registered(helper: &Path) -> Result<bool, String> {
    Ok(read_value(RUN_VALUE_NAME)?.as_deref() == Some(build_run_command(helper)?.as_str()))
}

fn build_run_command(helper_path: &Path) -> Result<String, String> {
    if !helper_path.is_absolute() {
        return Err("Observer helper must be absolute".into());
    }
    crate::windows_activation::installed_debugger_user_root(helper_path)?;
    let path = crate::windows_system::windows_path_for_display(helper_path);
    if path.contains(['"', '\0', '\r', '\n']) {
        return Err("Observer helper path cannot be quoted".into());
    }
    let command = format!("\"{path}\" {OBSERVER_ARGUMENT}");
    validate_run_command_length(&command)?;
    Ok(command)
}

fn validate_run_command_length(command: &str) -> Result<(), String> {
    if command.encode_utf16().count() > RUN_COMMAND_LIMIT {
        return Err("Observer Run command exceeds the 260 character limit".into());
    }
    Ok(())
}

fn classify_run_value(existing: Option<&str>, owned_command: &str) -> ExistingRunValue {
    match existing {
        None => ExistingRunValue::Missing,
        Some(value) if value == owned_command => ExistingRunValue::Owned,
        Some(_) => ExistingRunValue::Foreign,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_login_entry_can_be_removed_after_its_helper_disappears() {
        let root = std::env::temp_dir().join(format!(
            "incodex-startup-missing-helper-{}",
            std::process::id()
        ));
        let state = crate::windows_install::install_windows_runtime_with(
            &root,
            "OpenAI.Codex_1.2.3.4_x64__2p2nqsd0c76g0",
            &std::env::current_exe().unwrap(),
            |_| Ok(vec![]),
            |_| Ok(false),
            |_| Ok(()),
            |_| Ok(()),
        )
        .unwrap();
        let command = build_run_command(&state.helper_path).unwrap();
        std::fs::remove_file(&state.helper_path).unwrap();
        prove_removable_command(&command, &root)
            .expect("recorded ownership survives a missing helper");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn registry_round_trip_uses_only_a_unique_disposable_value() {
        let name = format!(
            "IncodexObserverTest-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        assert!(read_value(&name).unwrap().is_none());
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = delete_value(&self.0);
            }
        }
        let cleanup = Cleanup(name);
        let command = r#""C:\Windows\System32\cmd.exe" /c exit 0"#;
        write_value(&cleanup.0, command).unwrap();
        assert_eq!(read_value(&cleanup.0).unwrap().as_deref(), Some(command));
        delete_value(&cleanup.0).unwrap();
        assert!(read_value(&cleanup.0).unwrap().is_none());
    }

    #[test]
    fn rejects_unmanaged_helpers_and_unquoted_extra_arguments() {
        assert!(build_run_command(Path::new(r"C:\Windows\System32\cmd.exe")).is_err());
        assert!(prove_owned_command(
            r#""C:\Windows\System32\cmd.exe" --incodex-windows-update-observer"#
        )
        .is_err());
        assert!(prove_owned_command(r#""C:\Users\Alice\.incodex\windows\i\0123456789abcdef\i.exe" --incodex-windows-update-observer extra"#).is_err());
    }

    #[test]
    fn red_builds_exact_quoted_observer_command() {
        let helper = Path::new(r"C:\Users\Alice Example\.incodex\windows\i\0123456789abcdef\i.exe");

        assert_eq!(
            build_run_command(helper).expect("quoted Run command"),
            r#""C:\Users\Alice Example\.incodex\windows\i\0123456789abcdef\i.exe" --incodex-windows-update-observer"#
        );
    }

    #[test]
    fn red_rejects_run_commands_over_the_260_character_limit() {
        let within_limit = "x".repeat(RUN_COMMAND_LIMIT);
        assert!(validate_run_command_length(&within_limit).is_ok());

        let over_limit = "x".repeat(RUN_COMMAND_LIMIT + 1);
        let error = validate_run_command_length(&over_limit)
            .expect_err("Run command over 260 characters must be rejected");
        assert!(error.contains("260"), "{error}");
    }

    #[test]
    fn red_only_the_exact_fixed_command_is_owned() {
        let owned = r#""C:\Users\Alice\.incodex\windows\i\0123456789abcdef\i.exe" --incodex-windows-update-observer"#;
        let foreign = r#""C:\Users\Alice\other.exe" --incodex-windows-update-observer"#;

        assert_eq!(classify_run_value(None, owned), ExistingRunValue::Missing);
        assert_eq!(
            classify_run_value(Some(owned), owned),
            ExistingRunValue::Owned
        );
        assert_eq!(
            classify_run_value(Some(foreign), owned),
            ExistingRunValue::Foreign
        );
    }
}
