use std::path::{Path, PathBuf};

pub const DEFAULT_APP: &str = "/Applications/ChatGPT.app";
pub const RUNTIME_DIR_NAME: &str = "runtime";
pub const RUNTIME_CURRENT_NAME: &str = "current.json";
pub const ASAR_REL: &str = "Contents/Resources/app.asar";

pub fn default_app() -> PathBuf {
    PathBuf::from(DEFAULT_APP)
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn user_root() -> PathBuf {
    user_root_from(&home_dir())
}

pub fn user_root_from(home: &Path) -> PathBuf {
    home.join(".incodex")
}
