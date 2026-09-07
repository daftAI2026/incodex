/**
 * [INPUT]: 依赖 std 文件系统、平台环境变量与 serde::Serialize，读取受限的系统壁纸目录
 * [OUTPUT]: 对外提供 SystemWallpaperLibrary、SystemWallpaperEntry 及 list/load 壁纸接口
 * [POS]: capture-window 的本机素材边界；只暴露不含原图的目录元数据，并以会话索引装载原图
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

use std::path::PathBuf;

use serde::Serialize;

pub(crate) const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SystemWallpaperEntry {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) thumbnail: String,
}

pub(crate) struct SystemWallpaperLibrary {
    roots: Vec<PathBuf>,
}

impl SystemWallpaperLibrary {
    pub(crate) fn new() -> Self {
        Self { roots: Vec::new() }
    }

    #[cfg(test)]
    pub(crate) fn from_roots(roots: Vec<PathBuf>) -> Self {
        Self { roots }
    }

    pub(crate) fn list(&mut self) -> Result<Vec<SystemWallpaperEntry>, String> {
        Err("system wallpaper loader is not implemented".to_string())
    }

    pub(crate) fn load(&self, _id: &str) -> Result<String, String> {
        Err("system wallpaper loader is not implemented".to_string())
    }
}

#[cfg(test)]
#[path = "system_wallpapers_tests.rs"]
mod tests;
