/**
 * [INPUT]: 依赖 HOME 环境与标准库路径拼接。
 * [OUTPUT]: 提供官方 App 常量与 Incodex 用户目录定位。
 * [POS]: Unix 路径定义边界；不创建目录或探测应用。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
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
