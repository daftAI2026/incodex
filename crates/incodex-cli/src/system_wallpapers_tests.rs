/**
 * [INPUT]: 依赖 system_wallpapers 的测试构造器与系统壁纸会话 API
 * [OUTPUT]: 证明本机扫描、会话索引、目录边界和资源上限的失败回归用例
 * [POS]: system_wallpapers 的独立契约测试；不触碰真实系统壁纸目录，也不写系统路径
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{SystemWallpaperLibrary, MAX_SOURCE_BYTES};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "incodex-system-wallpapers-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temporary wallpaper fixture");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn tiny_png() -> &'static [u8] {
    &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0,
        0, 1, 8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156,
        99, 248, 207, 192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73,
        69, 78, 68, 174, 66, 96, 130,
    ]
}

fn write_png(path: &Path) {
    fs::write(path, tiny_png()).expect("write png fixture");
}

#[test]
fn recursive_scan_excludes_thumbnails_and_madesktop_descriptors() {
    let fixture = TempDir::new();
    let nested = fixture.path().join("Nested");
    let thumbnails = fixture.path().join(".thumbnails");
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir_all(&thumbnails).unwrap();
    write_png(&fixture.path().join("Sunrise.png"));
    write_png(&nested.join("Forest.PNG"));
    write_png(&thumbnails.join("Should-not-appear.png"));
    fs::write(fixture.path().join("Animated.madesktop"), b"descriptor").unwrap();

    let mut library = SystemWallpaperLibrary::from_roots(vec![fixture.path().to_path_buf()]);
    let entries = library.list().expect("fixture scan");

    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(),
        ["Forest", "Sunrise"]
    );
    assert!(entries.iter().all(|entry| {
        entry.id.starts_with("system-wallpaper-")
            && entry.thumbnail.starts_with("data:image/png;base64,")
            && !entry.thumbnail.contains("Should-not-appear")
    }));
}

#[test]
fn load_requires_a_session_id_and_never_accepts_a_renderer_path() {
    let fixture = TempDir::new();
    let image = fixture.path().join("Only.png");
    write_png(&image);
    let mut library = SystemWallpaperLibrary::from_roots(vec![fixture.path().to_path_buf()]);

    assert!(library.load("system-wallpaper-0").is_err());
    let entries = library.list().expect("fixture scan");
    let data_url = library.load(&entries[0].id).expect("load indexed image");
    assert!(data_url.starts_with("data:image/png;base64,"));
    assert!(library.load(image.to_str().unwrap()).is_err());
    assert!(library.load("system-wallpaper-999").is_err());
}

#[cfg(unix)]
#[test]
fn scanner_rejects_symlinked_files_outside_the_allowed_root() {
    let fixture = TempDir::new();
    let outside = TempDir::new();
    write_png(&fixture.path().join("inside.png"));
    write_png(&outside.path().join("outside.png"));
    std::os::unix::fs::symlink(outside.path().join("outside.png"), fixture.path().join("escape.png"))
        .expect("create symlink fixture");

    let mut library = SystemWallpaperLibrary::from_roots(vec![fixture.path().to_path_buf()]);
    let entries = library.list().expect("fixture scan");

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "inside");
}

#[test]
fn oversized_files_are_skipped_before_thumbnail_or_original_reads() {
    let fixture = TempDir::new();
    write_png(&fixture.path().join("small.png"));
    let oversized = fixture.path().join("oversized.png");
    let file = fs::File::create(&oversized).unwrap();
    file.set_len(MAX_SOURCE_BYTES + 1).unwrap();

    let mut library = SystemWallpaperLibrary::from_roots(vec![fixture.path().to_path_buf()]);
    let entries = library.list().expect("fixture scan");

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "small");
    assert!(library.load("system-wallpaper-1").is_err());
}
