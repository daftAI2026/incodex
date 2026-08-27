use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Component, Path, PathBuf};

use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

pub(crate) fn windows_path_for_display(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = value.strip_prefix(r"\\?\") {
        let bytes = rest.as_bytes();
        if bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/')
        {
            return rest.to_string();
        }
    }
    value.into_owned()
}

pub(crate) fn system_binary_path(relative: impl AsRef<Path>) -> Result<PathBuf, String> {
    let relative = relative.as_ref();
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("Windows system binary path must be a safe relative path".to_string());
    }

    let mut wide = vec![0u16; 260];
    loop {
        let length = unsafe { GetSystemDirectoryW(wide.as_mut_ptr(), wide.len() as u32) };
        if length == 0 {
            return Err(format!(
                "cannot locate the Windows system directory: {}",
                std::io::Error::last_os_error()
            ));
        }
        if length as usize >= wide.len() {
            wide.resize(length as usize + 1, 0);
            continue;
        }
        wide.truncate(length as usize);
        break;
    }

    let executable = PathBuf::from(OsString::from_wide(&wide)).join(relative);
    if executable.is_file() {
        Ok(executable)
    } else {
        Err(format!(
            "system Windows binary is unavailable: {}",
            executable.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::windows_path_for_display;
    use std::path::Path;

    #[test]
    fn user_facing_windows_paths_hide_only_verbatim_disk_and_unc_prefixes() {
        assert_eq!(
            windows_path_for_display(Path::new(r"\\?\C:\Users\Kid\.incodex")),
            r"C:\Users\Kid\.incodex"
        );
        assert_eq!(
            windows_path_for_display(Path::new(r"\\?\UNC\server\share\Codex")),
            r"\\server\share\Codex"
        );
        assert_eq!(
            windows_path_for_display(Path::new(r"C:\Users\Kid\.incodex")),
            r"C:\Users\Kid\.incodex"
        );
        assert_eq!(
            windows_path_for_display(Path::new(r"\\?\Volume{fixture}\Codex")),
            r"\\?\Volume{fixture}\Codex"
        );
    }
}
