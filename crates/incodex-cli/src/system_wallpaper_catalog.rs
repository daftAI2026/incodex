//! [INPUT]: 已验证的当前桌面原图路径与所属目录。
//! [OUTPUT]: 提供现成缩略图候选，不扫描或排列历史系统版本。
//! [POS]: system_wallpapers 的目录语义策略，不读取网络或创造可用素材。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use std::path::{Path, PathBuf};

pub(crate) fn thumbnail_paths(full: &Path, root: &Path) -> Vec<PathBuf> {
    let Some(stem) = full.file_stem().and_then(|stem| stem.to_str()) else {
        return Vec::new();
    };
    let Some(parent) = full.parent() else {
        return Vec::new();
    };
    let mut candidates = vec![
        parent.join(format!("{stem} Thumbnail.png")),
        parent.join(format!("{stem} Thumbnail@2x.png")),
    ];
    // Tahoe 的完整图片位于系统扩展资源，现成预览没有同名 stem。
    if stem == "TahoeLight" && root.ends_with("NeptuneOneWallpaper.appex/Contents/Resources") {
        candidates.insert(0, root.join("thumbnail.heic"));
    }
    for extension in ["png", "jpg", "heic"] {
        candidates.push(root.join(".thumbnails").join(format!("{stem}.{extension}")));
    }
    candidates
}

#[cfg(test)]
#[path = "system_wallpaper_catalog_tests.rs"]
mod tests;
