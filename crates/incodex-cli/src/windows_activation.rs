use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStrExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsActivationRequest {
    package_full_name: String,
    app_user_model_id: String,
    arguments: String,
    environment: Vec<u16>,
}

impl WindowsActivationRequest {
    pub fn new<I>(
        package_full_name: &str,
        app_user_model_id: &str,
        arguments: I,
        environment: BTreeMap<String, OsString>,
    ) -> Result<Self, String>
    where
        I: IntoIterator<Item = OsString>,
    {
        validate_text(package_full_name, "package full name")?;
        validate_text(app_user_model_id, "app user model id")?;
        let arguments = arguments
            .into_iter()
            .map(|argument| quote_windows_argument(&argument))
            .collect::<Result<Vec<_>, _>>()?
            .join(" ");
        let environment = environment_block(environment)?;
        Ok(Self {
            package_full_name: package_full_name.to_string(),
            app_user_model_id: app_user_model_id.to_string(),
            arguments,
            environment,
        })
    }

    pub fn package_full_name(&self) -> &str {
        &self.package_full_name
    }

    pub fn app_user_model_id(&self) -> &str {
        &self.app_user_model_id
    }

    pub fn arguments(&self) -> &str {
        &self.arguments
    }

    pub fn environment(&self) -> &[u16] {
        &self.environment
    }
}

fn validate_text(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.contains('\0') {
        return Err(format!("Windows {label} is empty or contains NUL"));
    }
    Ok(())
}

fn environment_block(environment: BTreeMap<String, OsString>) -> Result<Vec<u16>, String> {
    let mut block = Vec::new();
    for (name, value) in environment {
        if name.is_empty() || name.contains(['=', '\0']) {
            return Err("Windows activation environment name is invalid".to_string());
        }
        let value: Vec<u16> = value.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(format!(
                "Windows activation environment value for {name} contains NUL"
            ));
        }
        block.extend(name.encode_utf16());
        block.push('=' as u16);
        block.extend(value);
        block.push(0);
    }
    block.push(0);
    if block.len() == 1 {
        block.push(0);
    }
    Ok(block)
}

fn quote_windows_argument(argument: &OsStr) -> Result<String, String> {
    let wide: Vec<u16> = argument.encode_wide().collect();
    if wide.contains(&0) {
        return Err("Windows activation argument contains NUL".to_string());
    }
    let needs_quotes =
        wide.is_empty() || wide.iter().any(|unit| matches!(*unit, 0x20 | 0x09 | 0x22));
    if !needs_quotes {
        return String::from_utf16(&wide)
            .map_err(|_| "Windows activation argument is not valid Unicode".to_string());
    }

    let mut quoted = Vec::with_capacity(wide.len() + 2);
    quoted.push(b'"' as u16);
    let mut backslashes = 0;
    for unit in wide {
        if unit == b'\\' as u16 {
            backslashes += 1;
            continue;
        }
        if unit == b'"' as u16 {
            quoted.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2 + 1));
        } else {
            quoted.extend(std::iter::repeat_n(b'\\' as u16, backslashes));
        }
        backslashes = 0;
        quoted.push(unit);
    }
    quoted.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2));
    quoted.push(b'"' as u16);
    String::from_utf16(&quoted)
        .map_err(|_| "Windows activation argument is not valid Unicode".to_string())
}
