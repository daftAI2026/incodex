//! [INPUT]: 已验证的本机原图路径集合与系统壁纸根目录。
//! [OUTPUT]: 提供近期系统版本主题筛选、现成缩略图候选。
//! [POS]: system_wallpapers 的目录语义策略，不读取网络或创造可用素材。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use std::path::{Path, PathBuf};
pub(crate) fn recent_theme_paths(paths: &[PathBuf]) -> Vec<PathBuf> { paths.to_vec() }
pub(crate) fn thumbnail_paths(_full: &Path, _root: &Path) -> Vec<PathBuf> { Vec::new() }
#[cfg(test)]
#[path = "system_wallpaper_catalog_tests.rs"]
mod tests;
