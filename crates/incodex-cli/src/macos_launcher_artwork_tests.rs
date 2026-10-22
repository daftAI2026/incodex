// [INPUT]: 依赖临时.app图标、假偏好域、内嵌PNG徽标与正式launcher publisher。
// [OUTPUT]: 验证图标选择只读、AppKit合成、V4拥有权与入口清理。
// [POS]: artwork到launcher清理的隔离端到端回归；不启动App或读取真实偏好。
// [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use super::{render, render_with_badge};
use crate::macos_launch_icon::{
    inspect_launchers, launcher_path, publish_launcher, remove_confirmed_launchers,
};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const APP_ICON_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x20, 0x08, 0x06, 0x00, 0x00, 0x00, 0x73, 0x7a, 0x7a,
    0xf4, 0x00, 0x00, 0x00, 0x37, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0xed, 0xce, 0x21, 0x01, 0x00,
    0x20, 0x0c, 0x00, 0xb0, 0x87, 0x40, 0x10, 0x8b, 0x74, 0xc4, 0x79, 0x26, 0x1a, 0x9c, 0x10, 0x17,
    0x37, 0x13, 0xf3, 0x8b, 0x75, 0xb2, 0x3a, 0xde, 0xdd, 0x2d, 0x21, 0x20, 0x20, 0x20, 0x20, 0x20,
    0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x30, 0x1d, 0xf8, 0x6b, 0x66, 0x7c, 0x88,
    0x5e, 0x09, 0x7c, 0x04, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

const BADGE_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x08, 0x08, 0x06, 0x00, 0x00, 0x00, 0xc4, 0x0f, 0xbe,
    0x8b, 0x00, 0x00, 0x00, 0x1a, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xf8, 0xff, 0xff, 0x3f,
    0x03, 0x3e, 0xcc, 0x40, 0x8a, 0x82, 0x05, 0x68, 0x78, 0x20, 0x14, 0x90, 0xe7, 0x0b, 0x00, 0x1c,
    0xc0, 0xc9, 0x41, 0xab, 0x27, 0x7c, 0x95, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

const TRANSPARENT_BADGE_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x08, 0x08, 0x06, 0x00, 0x00, 0x00, 0xc4, 0x0f, 0xbe,
    0x8b, 0x00, 0x00, 0x00, 0x01, 0x73, 0x52, 0x47, 0x42, 0x00, 0xae, 0xce, 0x1c, 0xe9, 0x00, 0x00,
    0x00, 0x38, 0x65, 0x58, 0x49, 0x66, 0x4d, 0x4d, 0x00, 0x2a, 0x00, 0x00, 0x00, 0x08, 0x00, 0x01,
    0x87, 0x69, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x1a, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x02, 0xa0, 0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x08, 0xa0, 0x03,
    0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0xb6, 0x4c,
    0x59, 0x68, 0x00, 0x00, 0x00, 0x0e, 0x49, 0x44, 0x41, 0x54, 0x18, 0x19, 0x63, 0x60, 0x18, 0x05,
    0xa0, 0x10, 0x00, 0x00, 0x01, 0x08, 0x00, 0x01, 0x94, 0x9c, 0x5c, 0x33, 0x00, 0x00, 0x00, 0x00,
    0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn app_with_icon() -> Self {
        let root = std::env::temp_dir().join(format!(
            "incodex-launcher-artwork-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let app = root.join("Fixture.app");
        let resources = app.join("Contents/Resources");
        let macos = app.join("Contents/MacOS");
        fs::create_dir_all(&resources).unwrap();
        fs::create_dir_all(&macos).unwrap();

        // icp5 is the 32px PNG representation used by the fixture's declared app icon.
        let chunk_len = (8 + APP_ICON_PNG.len()) as u32;
        let file_len = 8 + chunk_len as usize;
        let mut icns = Vec::with_capacity(file_len);
        icns.extend_from_slice(b"icns");
        icns.extend_from_slice(&(file_len as u32).to_be_bytes());
        icns.extend_from_slice(b"icp5");
        icns.extend_from_slice(&chunk_len.to_be_bytes());
        icns.extend_from_slice(APP_ICON_PNG);
        fs::write(resources.join("Fixture.icns"), icns).unwrap();
        fs::write(
            app.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
            <plist version="1.0"><dict>
            <key>CFBundleIdentifier</key><string>test.incodex.launcher.artwork</string>
            <key>CFBundleName</key><string>Launcher Artwork Fixture</string>
            <key>CFBundlePackageType</key><string>APPL</string>
            <key>CFBundleExecutable</key><string>Fixture</string>
            <key>CFBundleIconFile</key><string>Fixture.icns</string>
            </dict></plist>"#,
        )
        .unwrap();
        let executable = macos.join("Fixture");
        fs::write(&executable, b"temporary fixture; never launched").unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        Self(root)
    }

    fn app(&self) -> PathBuf {
        self.0.join("Fixture.app")
    }

    fn home(&self) -> PathBuf {
        let home = self.0.join("fixture HOME");
        fs::create_dir_all(&home).unwrap();
        home
    }

    fn cli(&self) -> PathBuf {
        let cli = self.0.join("fake incodex CLI");
        fs::write(&cli, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
        cli
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn icns_representations(bytes: &[u8]) -> HashMap<[u8; 4], (u32, u32)> {
    assert_eq!(&bytes[..4], b"icns");
    assert_eq!(
        u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize,
        bytes.len()
    );
    let mut representations = HashMap::new();
    let mut offset = 8;
    while offset < bytes.len() {
        let tag: [u8; 4] = bytes[offset..offset + 4].try_into().unwrap();
        let size = u32::from_be_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        assert!(
            size >= 8 && offset + size <= bytes.len(),
            "invalid ICNS chunk"
        );
        let png = &bytes[offset + 8..offset + size];
        assert!(png.len() >= 24, "ICNS PNG representation is truncated");
        assert_eq!(
            &png[..8],
            b"\x89PNG\r\n\x1a\n",
            "ICNS representation is not PNG"
        );
        let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
        assert!(
            representations.insert(tag, (width, height)).is_none(),
            "duplicate ICNS representation"
        );
        offset += size;
    }
    assert_eq!(offset, bytes.len());
    representations
}

fn icns_png_payload(bytes: &[u8], expected_tag: [u8; 4]) -> &[u8] {
    let mut offset = 8;
    while offset < bytes.len() {
        let tag: [u8; 4] = bytes[offset..offset + 4].try_into().unwrap();
        let size = u32::from_be_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if tag == expected_tag {
            return &bytes[offset + 8..offset + size];
        }
        offset += size;
    }
    panic!("missing ICNS representation {expected_tag:?}");
}

#[test]
fn declared_app_icon_is_composited_into_all_icns_sizes() {
    let fixture = Fixture::app_with_icon();
    assert!(fixture
        .app()
        .join("Contents/Resources/Fixture.icns")
        .is_file());
    let icns = render_with_badge(&fixture.app(), BADGE_PNG)
        .expect("valid .app icon plus transparent badge must render");
    let unbadged = render_with_badge(&fixture.app(), TRANSPARENT_BADGE_PNG)
        .expect("fully transparent badge must preserve a valid app icon");
    assert_ne!(
        icns_png_payload(&icns, *b"ic10"),
        icns_png_payload(&unbadged, *b"ic10"),
        "badge must change the composed app artwork"
    );
    let representations = icns_representations(&icns);
    let expected = [
        (*b"icp4", 16),
        (*b"icp5", 32),
        (*b"icp6", 64),
        (*b"ic07", 128),
        (*b"ic08", 256),
        (*b"ic09", 512),
        (*b"ic10", 1024),
    ];
    assert_eq!(representations.len(), expected.len());
    for (tag, size) in expected {
        assert_eq!(
            representations.get(&tag),
            Some(&(size, size)),
            "invalid ICNS representation {tag:?}"
        );
    }
}

#[test]
fn embedded_badge_asset_renders_for_a_declared_app_icon() {
    let fixture = Fixture::app_with_icon();
    let icns = render(&fixture.app()).expect("embedded badge must render for a valid .app icon");
    assert_eq!(icns_representations(&icns).len(), 7);
}

#[test]
fn native_compositor_preserves_asymmetric_source_and_badge_png_orientation() {
    let fixture = Fixture::app_with_icon();
    let harness = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("native/macos_launcher_icon_orientation_test.m");
    let executable = fixture.0.join("launcher-icon-orientation-test");
    let compilation = Command::new("xcrun")
        .args(["clang", "-fobjc-arc", "-Wall", "-Wextra", "-Werror"])
        .arg(&harness)
        .args([
            "-framework",
            "AppKit",
            "-framework",
            "CoreGraphics",
            "-framework",
            "Foundation",
            "-framework",
            "ImageIO",
            "-o",
        ])
        .arg(&executable)
        .output()
        .expect("xcrun must compile the isolated native orientation harness");
    assert!(
        compilation.status.success(),
        "native orientation harness failed to compile: {}",
        String::from_utf8_lossy(&compilation.stderr)
    );

    let result = Command::new(&executable)
        .output()
        .expect("run isolated native orientation harness");
    assert!(
        result.status.success(),
        "native orientation contract failed: {}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        result.stderr.is_empty(),
        "native renderer must not leak CoreGraphics diagnostics: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn native_renderer_uses_only_safe_official_bundle_icon_preferences() {
    let fixture = Fixture::app_with_icon();
    let harness = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("native/macos_launcher_icon_preferences_test.m");
    let executable = fixture.0.join("launcher-icon-preferences-test");
    let compilation = Command::new("xcrun")
        .args(["clang", "-fobjc-arc", "-Wall", "-Wextra", "-Werror"])
        .arg(&harness)
        .args([
            "-framework",
            "AppKit",
            "-framework",
            "CoreGraphics",
            "-framework",
            "Foundation",
            "-framework",
            "ImageIO",
            "-o",
        ])
        .arg(&executable)
        .output()
        .expect("xcrun must compile the isolated native preference harness");
    assert!(
        compilation.status.success(),
        "native preference harness failed to compile: {}",
        String::from_utf8_lossy(&compilation.stderr)
    );

    let fake_home = fixture.0.join("fake HOME");
    fs::create_dir_all(&fake_home).unwrap();
    let result = Command::new(&executable)
        .arg(fixture.0.join("preference-cases"))
        .arg(&fake_home)
        .env("HOME", &fake_home)
        .env_remove("CFFIXED_USER_HOME")
        .output()
        .expect("run isolated native preference harness");
    assert!(
        result.status.success(),
        "native preference contract failed: {}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        result.stderr.is_empty(),
        "native renderer must not leak system diagnostics: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn production_publisher_owns_dynamic_icon_after_source_app_disappears() {
    let fixture = Fixture::app_with_icon();
    let home = fixture.home();
    let cli = fixture.cli();
    let app = fixture.app();

    let published = publish_launcher(&home, &cli, &app)
        .expect("production publisher must render and publish the fixture app icon");
    let expected_path = launcher_path(&home);
    assert_eq!(published, expected_path);

    let resources = published.join("Contents/Resources");
    let marker = fs::read(resources.join(".incodex-launcher-owner")).unwrap();
    let marker_prefix = b"incodex-launcher-owner-v4\nsha256:";
    assert!(marker.starts_with(marker_prefix));
    assert_eq!(marker.len(), marker_prefix.len() + 64 + 1);
    let icns = fs::read(resources.join("IncodexLauncher.icns")).unwrap();
    assert_eq!(icns_representations(&icns).len(), 7);

    let inventory = inspect_launchers(&home);
    assert!(
        inventory.unknown.is_empty(),
        "published launcher is recognized"
    );
    assert_eq!(inventory.owned.len(), 1);
    assert_eq!(inventory.owned[0], published);

    fs::remove_dir_all(&app).unwrap();
    let removal = remove_confirmed_launchers(&home, std::slice::from_ref(&published), true)
        .expect("owned launcher removal must not need the source app");
    assert_eq!(removal.removed.as_slice(), std::slice::from_ref(&published));
    assert!(!published.exists());
}

#[test]
fn app_without_declared_icon_resource_is_rejected() {
    let fixture = Fixture::app_with_icon();
    fs::remove_file(fixture.app().join("Contents/Resources/Fixture.icns")).unwrap();
    let error = render_with_badge(&fixture.app(), BADGE_PNG).unwrap_err();
    assert!(
        error.contains("icon"),
        "expected explicit icon failure, got {error}"
    );
}

#[test]
fn folder_with_app_suffix_is_not_a_valid_icon_source() {
    let path = std::env::temp_dir().join(format!(
        "incodex-invalid-artwork-{}.app",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    let error = render_with_badge(&path, BADGE_PNG).unwrap_err();
    let _ = fs::remove_dir_all(path);
    assert!(error.contains(".app") || error.contains("bundle"));
}
