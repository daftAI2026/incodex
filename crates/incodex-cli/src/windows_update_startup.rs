use std::path::Path;

use crate::windows_install_state::WindowsInstallState;

const RUN_VALUE_NAME: &str = "IncodexUpdateRepair";
const OBSERVER_ARGUMENT: &str = "--incodex-windows-update-observer";
const RUN_COMMAND_LIMIT: usize = 260;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingRunValue {
    Missing,
    Owned,
    Foreign,
}

pub(crate) fn register(_state: &WindowsInstallState) -> Result<(), String> {
    Err("Windows update startup registration is not implemented".to_string())
}

pub(crate) fn remove() -> Result<(), String> {
    Err("Windows update startup removal is not implemented".to_string())
}

fn build_run_command(_helper_path: &Path) -> Result<String, String> {
    Err("Windows update startup command construction is not implemented".to_string())
}

fn validate_run_command_length(_command: &str) -> Result<(), String> {
    Err("Windows update startup command length validation is not implemented".to_string())
}

fn classify_run_value(_existing: Option<&str>, _owned_command: &str) -> ExistingRunValue {
    ExistingRunValue::Foreign
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn red_builds_exact_quoted_observer_command() {
        let helper = Path::new(
            r"C:\Users\Alice Example\.incodex\windows\i\0123456789abcdef\i.exe",
        );

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

        assert_eq!(
            classify_run_value(None, owned),
            ExistingRunValue::Missing
        );
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
