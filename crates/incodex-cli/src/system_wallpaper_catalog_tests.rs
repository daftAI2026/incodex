//! [INPUT]: 本机系统版本主题筛选与缩略图候选解析。
//! [OUTPUT]: 证明非主题资源排除、近期版本优先与系统现成预览复用。
//! [POS]: 系统壁纸装载性能边界测试。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use super::*;

#[test]
fn recent_themes_exclude_colors_hardware_and_keep_only_three_release_families() {
    let names = ["Mac Blue.heic", "iMac Pink.heic", "Solid Colors/Gold.png", "Sonoma.heic", "Sonoma Horizon.heic", "Ventura Graphic.heic", "Monterey Graphic.heic", "Big Sur.heic"];
    let paths: Vec<_> = names.iter().map(PathBuf::from).collect();
    let selected = recent_theme_paths(&paths);
    assert_eq!(selected, ["Sonoma.heic", "Sonoma Horizon.heic", "Ventura Graphic.heic", "Monterey Graphic.heic"].map(PathBuf::from));
}
#[test]
fn system_thumbnail_candidates_use_siblings_before_root_previews() {
    let root = Path::new("/System/Library/Desktop Pictures");
    let full = root.join(".wallpapers/Sonoma Horizon/Sonoma Horizon.heic");
    let candidates = thumbnail_paths(&full, root);
    assert_eq!(candidates[0], full.parent().unwrap().join("Sonoma Horizon Thumbnail.png"));
    assert!(candidates.contains(&root.join(".thumbnails/Sonoma Horizon.heic")));
}
