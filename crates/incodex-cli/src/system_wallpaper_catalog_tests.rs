//! [INPUT]: 当前桌面的现成缩略图候选解析。
//! [OUTPUT]: 证明系统现成预览复用及查找顺序。
//! [POS]: 系统壁纸装载性能边界测试。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use super::*;

#[test]
fn system_thumbnail_candidates_use_siblings_before_root_previews() {
    let root = Path::new("/System/Library/Desktop Pictures");
    let full = root.join(".wallpapers/Sonoma Horizon/Sonoma Horizon.heic");
    let candidates = thumbnail_paths(&full, root);
    assert_eq!(
        candidates[0],
        full.parent().unwrap().join("Sonoma Horizon Thumbnail.png")
    );
    assert!(candidates.contains(&root.join(".thumbnails/Sonoma Horizon.heic")));
}

#[test]
fn current_tahoe_can_reuse_its_adjacent_preview() {
    let root = Path::new(
        "/System/Library/ExtensionKit/Extensions/NeptuneOneWallpaper.appex/Contents/Resources",
    );
    assert!(
        thumbnail_paths(&root.join("TahoeLight.heic"), root).contains(&root.join("thumbnail.heic"))
    );
}
