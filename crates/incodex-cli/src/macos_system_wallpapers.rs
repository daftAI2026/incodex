//! [INPUT]: 已取证的 macOS 版本资源身份、系统图片与 Apple HTTPS 视频源。
//! [OUTPUT]: 系统主题/风景双目录、可取消的有界下载和静态 JPEG 编辑资源。
//! [POS]: Shot 的原生资源获取边界；不改系统桌面，不接收 renderer URL，不持久保存视频。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use crate::system_wallpaper_files::{read_file_limited, PrivateTempDir};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub(crate) const THEME: &str = "system-wallpaper-theme";
pub(crate) const LANDSCAPE: &str = "system-wallpaper-landscape";
const MAX_VIDEO_BYTES: u64 = 256 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_JPEG_BYTES: usize = 8 * 1024 * 1024;
// macOS 27 官方 aerials manifest: shotID GG_LM_H，明亮横向版本，不取画廊缩略图。
const GOLDEN_GATE_THEME: &str = "https://sylvan.apple.com/itunes-assets/Aerials116/v4/cb/5b/50/cb5b5035-6701-619f-9065-3d7d0e5fbef4/GG_LM_H_v063_240fps-TSA.mov";

pub(crate) enum WallpaperSource {
    Image(PathBuf),
    RemoteVideo(&'static str),
}

pub(crate) fn source_for_version(major: u32, id: &str) -> Result<WallpaperSource, String> {
    match (major, id) {
        (26 | 27, LANDSCAPE) => Ok(WallpaperSource::Image("/System/Library/Wallpapers/.default/DefaultAerial.heic".into())),
        (26, THEME) => Ok(WallpaperSource::Image("/System/Library/ExtensionKit/Extensions/NeptuneOneWallpaper.appex/Contents/Resources/TahoeLight.heic".into())),
        (27, THEME) => Ok(WallpaperSource::RemoteVideo(GOLDEN_GATE_THEME)),
        _ => Err("system wallpaper source is not verified for this macOS version".into()),
    }
}

pub(crate) fn major_version() -> Option<u32> {
    let output = Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    std::str::from_utf8(&output.stdout)
        .ok()?
        .trim()
        .split('.')
        .next()?
        .parse()
        .ok()
}

pub(crate) fn entries(major: u32) -> serde_json::Value {
    let names = match major {
        27 => ["Golden Gate", "Golden Gate Sunset"],
        26 => ["Tahoe", "Tahoe Day"],
        _ => return serde_json::json!([]),
    };
    serde_json::json!([
        {"id":THEME,"name":names[0],"thumbnail":"","loadStatus":"idle"},
        {"id":LANDSCAPE,"name":names[1],"thumbnail":"","loadStatus":"idle"}
    ])
}

pub(crate) fn valid_asset_url(url: &str) -> bool {
    let Some(path) = url.strip_prefix("https://sylvan.apple.com/itunes-assets/") else {
        return false;
    };
    !path.is_empty()
        && path.ends_with(".mov")
        && !path.contains("..")
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b))
}

pub(crate) fn load(major: u32, id: &str, alive: &AtomicBool) -> Result<String, String> {
    if !alive.load(Ordering::Acquire) {
        return Err("wallpaper request cancelled".into());
    }
    let source = source_for_version(major, id)?;
    let local = match source {
        WallpaperSource::Image(path) => read_file_limited(&path, MAX_IMAGE_BYTES)
            .and_then(|bytes| crate::macos_image_io::encode_wallpaper_jpeg(&bytes, MAX_JPEG_BYTES))
            .ok(),
        WallpaperSource::RemoteVideo(url) => {
            if !valid_asset_url(url) {
                return Err("untrusted wallpaper URL".into());
            }
            None
        }
    };
    let jpeg = if let Some(jpeg) = local {
        jpeg
    } else {
        let (asset_id, url) = remote_asset(major, id)?;
        let mut directory = PrivateTempDir::new()?;
        let path = directory.next_file("source", "mov");
        let cached = incodex_core::paths::home_dir()
            .join("Library/Application Support/com.apple.wallpaper/aerials/videos")
            .join(format!("{asset_id}.mov"));
        let bytes = if cached
            .ancestors()
            .all(|p| !std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()))
        {
            read_file_limited(&cached, MAX_VIDEO_BYTES).ok()
        } else {
            None
        };
        if let Some(bytes) = bytes {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&path)
                .map_err(|_| "cannot stage local wallpaper")?;
            file.write_all(&bytes)
                .map_err(|_| "cannot stage local wallpaper")?;
        } else {
            download(url, &path, alive)?;
        }
        if !alive.load(Ordering::Acquire) {
            return Err("wallpaper request cancelled".into());
        }
        crate::macos_wallpaper_video::frame_jpeg(&path, MAX_JPEG_BYTES)?
    };
    if !alive.load(Ordering::Acquire) {
        return Err("wallpaper request cancelled".into());
    }
    Ok(format!(
        "data:image/jpeg;base64,{}",
        crate::profile_mask::base64_encode(&jpeg)
    ))
}

fn remote_asset(major: u32, id: &str) -> Result<(&'static str, &'static str), String> {
    match (major, id) {
        (27, THEME) => Ok(("4DFE24ED-71CC-42D4-9FE8-3B8959B6CC19", GOLDEN_GATE_THEME)),
        (27, LANDSCAPE) => Ok(("4207734D-74FE-4F92-B5E1-6EC8DEE24A15", "https://sylvan.apple.com/itunes-assets/Aerials116/v4/cb/5b/50/cb5b5035-6701-619f-9065-3d7d0e5fbef4/GG_A_SUNSET_MarshallsBeach_c28_v7_24comp_HFR_16Mbps.mov")),
        (26, LANDSCAPE) => Ok(("4C108785-A7BA-422E-9C79-B0129F1D5550", "https://sylvan.apple.com/itunes-assets/Aerials116/v4/cb/5b/50/cb5b5035-6701-619f-9065-3d7d0e5fbef4/LIGHT02_20250613_V2_sdr_4k_rate12000_240p_t2160_grover74_tsa_MTE-Modified.mov")),
        _ => Err("no verified remote asset for this wallpaper".into()),
    }
}

fn download(url: &str, path: &std::path::Path, alive: &AtomicBool) -> Result<(), String> {
    if !valid_asset_url(url) {
        return Err("untrusted wallpaper URL".into());
    }
    use std::os::unix::fs::OpenOptionsExt;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| "cannot create wallpaper download")?;
    let mut child = Command::new("/usr/bin/curl")
        .args([
            "-q",
            "--fail",
            "--silent",
            "--proto",
            "=https",
            "--connect-timeout",
            "10",
            "--max-time",
            "150",
            "--max-filesize",
            &MAX_VIDEO_BYTES.to_string(),
            url,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::from(file))
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "cannot start wallpaper download")?;
    let deadline = Instant::now() + Duration::from_secs(155);
    loop {
        let too_large = std::fs::metadata(path)
            .map(|m| m.len() > MAX_VIDEO_BYTES)
            .unwrap_or(true);
        if !alive.load(Ordering::Acquire) || Instant::now() >= deadline || too_large {
            let _ = child.kill();
            let _ = child.wait();
            return Err("wallpaper download cancelled or exceeded its limits".into());
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err("wallpaper download failed".into()),
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("wallpaper download failed".into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn cancelled_request_never_starts_network() {
        assert!(super::load(27, super::THEME, &std::sync::atomic::AtomicBool::new(false)).is_err());
    }
    #[test]
    #[ignore = "requires an explicit 179 MB Apple download"]
    fn official_golden_gate_download_decodes_a_real_frame() {
        let mut dir = crate::system_wallpaper_files::PrivateTempDir::new().unwrap();
        let path = dir.next_file("official", "mov");
        super::download(
            super::GOLDEN_GATE_THEME,
            &path,
            &std::sync::atomic::AtomicBool::new(true),
        )
        .unwrap();
        let jpeg = crate::macos_wallpaper_video::frame_jpeg(&path, super::MAX_JPEG_BYTES).unwrap();
        let (w, h) = crate::macos_image_io::image_dimensions(&jpeg).unwrap();
        assert!(w.max(h) <= 2600 && w.min(h) > 1000);
        if let Ok(output) = std::env::var("INCODEX_WALLPAPER_EVIDENCE") {
            std::fs::write(output, jpeg).unwrap();
        }
    }
}
