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

#[cfg(target_os = "macos")]
#[test]
fn system_pair_uses_version_identity_not_current_desktop_or_gallery_order() {
    use crate::macos_system_wallpapers::{source_for_version, WallpaperSource};
    assert!(matches!(source_for_version(27, "system-wallpaper-theme"), Ok(WallpaperSource::RemoteVideo(_))));
    assert!(matches!(source_for_version(27, "system-wallpaper-landscape"), Ok(WallpaperSource::Image(_))));
    assert!(matches!(source_for_version(26, "system-wallpaper-theme"), Ok(WallpaperSource::Image(_))));
    assert!(source_for_version(99, "system-wallpaper-theme").is_err());
    assert!(source_for_version(27, "/tmp/picture.mov").is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn wallpaper_download_rejects_non_apple_urls_and_redirect_like_inputs() {
    use crate::macos_system_wallpapers::valid_asset_url;
    assert!(valid_asset_url("https://sylvan.apple.com/itunes-assets/Aerials116/v4/sample.mov"));
    for url in ["http://sylvan.apple.com/itunes-assets/a.mov", "https://sylvan.apple.com.evil/itunes-assets/a.mov", "https://sylvan.apple.com@evil/itunes-assets/a.mov", "https://127.0.0.1/a.mov", "file:///tmp/a.mov", "https://sylvan.apple.com/itunes-assets/../a.mov", "https://sylvan.apple.com/itunes-assets/a.mov?url=evil"] {
        assert!(!valid_asset_url(url), "{url}");
    }
}
