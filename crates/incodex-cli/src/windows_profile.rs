use std::path::PathBuf;

pub(crate) fn windows_user_profile() -> Result<PathBuf, String> {
    let profile = std::env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or("USERPROFILE is unavailable")?;
    if profile.is_absolute() {
        Ok(profile)
    } else {
        Err(format!(
            "USERPROFILE is not absolute: {}",
            profile.display()
        ))
    }
}
