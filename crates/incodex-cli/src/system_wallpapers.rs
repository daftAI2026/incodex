/**
 * [INPUT]: 依赖主线程提供的当前桌面路径、受限文件读取、ImageIO 与缩略图 sips 适配
 * [OUTPUT]: 对外提供 SystemWallpaperLibrary、SystemWallpaperEntry 及 list/load 壁纸接口
 * [POS]: 非 macOS 的当前桌面素材边界及 macOS 旧合同测试入口；绝不把 renderer 输入当作路径，macOS 生产改由双壁纸 adapter 负责
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::profile_mask::base64_encode;
use crate::system_wallpaper_files::{is_reparse_point, read_file_limited, PrivateTempDir};

/// 单个系统壁纸允许读取的源文件上限，先由 metadata 拒绝超限文件。
pub(crate) const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;

const MAX_WALLPAPERS: usize = 128;
const MAX_SCAN_DEPTH: usize = 16;
const MAX_THUMBNAIL_DATA_URL_BYTES: usize = 256 * 1024;
const MAX_LOAD_DATA_URL_BYTES: usize = 48 * 1024 * 1024;
const SIPS_TIMEOUT: Duration = Duration::from_secs(5);
const LIST_DEADLINE: Duration = Duration::from_secs(90);
const PNG_DATA_URL_PREFIX: &str = "data:image/png;base64,";
const SIPS_PATH: &str = "/usr/bin/sips";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SystemWallpaperEntry {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) thumbnail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageFormat {
    Png,
    Jpeg,
    Heic,
}

#[derive(Debug, Clone)]
struct WallpaperCandidate {
    path: PathBuf,
    root: PathBuf,
    format: ImageFormat,
    size: u64,
}

#[derive(Debug, Clone)]
struct SystemWallpaperRecord {
    id: String,
    path: PathBuf,
    root: PathBuf,
    format: ImageFormat,
}

pub(crate) struct SystemWallpaperLibrary {
    roots: Vec<PathBuf>,
    current: Option<PathBuf>,
    records: Vec<SystemWallpaperRecord>,
    entries: Vec<SystemWallpaperEntry>,
    listed: bool,
}

impl SystemWallpaperLibrary {
    pub(crate) fn new(current: Option<PathBuf>) -> Self {
        Self {
            roots: Vec::new(),
            current,
            records: Vec::new(),
            entries: Vec::new(),
            listed: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_roots(roots: Vec<PathBuf>) -> Self {
        Self::from_roots_with_current(roots, None)
    }

    #[cfg(test)]
    pub(crate) fn from_roots_with_current(roots: Vec<PathBuf>, current: Option<PathBuf>) -> Self {
        Self {
            roots,
            current,
            records: Vec::new(),
            entries: Vec::new(),
            listed: false,
        }
    }

    pub(crate) fn list(&mut self) -> Result<Vec<SystemWallpaperEntry>, String> {
        if self.listed {
            return Ok(self.entries.clone());
        }

        let deadline = Instant::now() + LIST_DEADLINE;
        let current = self
            .current
            .as_deref()
            .and_then(|path| current_candidate(path));
        let mut candidates = discover_candidates(&self.roots, deadline)?;
        let current_path = current.as_ref().map(|candidate| candidate.path.clone());
        if let Some(current) = current {
            candidates.retain(|candidate| candidate.path != current.path);
            candidates.insert(0, current);
        }
        candidates.truncate(MAX_WALLPAPERS);
        let mut temporary = None;
        let mut records = Vec::new();
        let mut entries = Vec::new();
        let mut conversion_failures = 0usize;
        let mut next_index = 0usize;

        for candidate in candidates {
            check_deadline(deadline)?;
            let Some(thumbnail) = build_thumbnail(&candidate, &mut temporary, deadline)? else {
                conversion_failures += 1;
                continue;
            };
            let is_current = current_path.as_deref() == Some(candidate.path.as_path());
            let id = if is_current {
                "system-wallpaper-current".to_string()
            } else {
                let id = format!("system-wallpaper-{next_index}");
                next_index = next_index.saturating_add(1);
                id
            };
            let name = if is_current {
                "Current Desktop".to_string()
            } else {
                display_name(&candidate.path)
            };
            records.push(SystemWallpaperRecord {
                id: id.clone(),
                path: candidate.path,
                root: candidate.root,
                format: candidate.format,
            });
            entries.push(SystemWallpaperEntry {
                id,
                name,
                thumbnail,
            });
        }

        if records.is_empty() && conversion_failures > 0 {
            return Err(conversion_unavailable_message());
        }

        self.records = records;
        self.entries = entries.clone();
        self.listed = true;
        Ok(entries)
    }

    pub(crate) fn load(&self, id: &str) -> Result<String, String> {
        let record = self
            .records
            .iter()
            .find(|record| record.id == id)
            .ok_or_else(|| "system wallpaper id is not in the current catalog".to_string())?;

        let path = verify_record_path(record)?;
        let deadline = Instant::now() + SIPS_TIMEOUT;
        let mut temporary = None;
        let bytes = match record.format {
            ImageFormat::Png => {
                let bytes = read_file_limited(&path, load_raw_limit() as u64)?;
                if is_png(&bytes) {
                    bytes
                } else {
                    convert_to_png(&path, &mut temporary, None, deadline)
                        .map_err(conversion_error_message)?
                }
            }
            ImageFormat::Jpeg => {
                let bytes = read_file_limited(&path, load_raw_limit() as u64)?;
                return jpeg_data_url(bytes, MAX_LOAD_DATA_URL_BYTES);
            }
            ImageFormat::Heic => {
                #[cfg(target_os = "macos")]
                {
                    let input = read_file_limited(&path, MAX_SOURCE_BYTES)?;
                    let bytes =
                        crate::macos_image_io::encode_wallpaper_jpeg(&input, load_raw_limit())
                            .map_err(|error| {
                                format!("macOS wallpaper ImageIO conversion failed: {error}")
                            })?;
                    return jpeg_data_url(bytes, MAX_LOAD_DATA_URL_BYTES);
                }
                #[cfg(not(target_os = "macos"))]
                {
                    return Err(conversion_unavailable_message());
                }
            }
        };

        png_data_url(bytes, MAX_LOAD_DATA_URL_BYTES)
    }
}

fn current_candidate(path: &Path) -> Option<WallpaperCandidate> {
    let canonical = fs::canonicalize(path).ok()?;
    let metadata = fs::metadata(&canonical).ok()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_SOURCE_BYTES {
        return None;
    }
    let format = image_format(&canonical)?;
    let root = canonical.parent()?.to_path_buf();
    Some(WallpaperCandidate {
        path: canonical,
        root,
        format,
        size: metadata.len(),
    })
}

fn discover_candidates(
    roots: &[PathBuf],
    deadline: Instant,
) -> Result<Vec<WallpaperCandidate>, String> {
    let mut candidates = Vec::new();
    for root in roots {
        check_deadline(deadline)?;
        let Some(canonical_root) = trusted_root(root) else {
            continue;
        };
        let mut visited = HashSet::new();
        walk_directory(
            &canonical_root,
            &canonical_root,
            &mut candidates,
            &mut visited,
            0,
            deadline,
        )?;
        if candidates.len() >= MAX_WALLPAPERS {
            break;
        }
    }
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    candidates.truncate(MAX_WALLPAPERS);
    Ok(candidates)
}

fn trusted_root(root: &Path) -> Option<PathBuf> {
    let metadata = fs::symlink_metadata(root).ok()?;
    if is_reparse_point(&metadata) || !metadata.is_dir() {
        return None;
    }
    fs::canonicalize(root).ok()
}

fn walk_directory(
    directory: &Path,
    canonical_root: &Path,
    candidates: &mut Vec<WallpaperCandidate>,
    visited: &mut HashSet<PathBuf>,
    depth: usize,
    deadline: Instant,
) -> Result<(), String> {
    check_deadline(deadline)?;
    if depth >= MAX_SCAN_DEPTH {
        return Ok(());
    }
    let _directory_metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) if !is_reparse_point(&metadata) && metadata.is_dir() => metadata,
        _ => return Ok(()),
    };
    let canonical_directory = match fs::canonicalize(directory) {
        Ok(canonical) if is_within(canonical_root, &canonical) => canonical,
        _ => return Ok(()),
    };
    if !visited.insert(canonical_directory.clone()) {
        return Ok(());
    }
    let mut children = match fs::read_dir(&canonical_directory) {
        Ok(entries) => entries.filter_map(Result::ok).collect::<Vec<_>>(),
        Err(_) => return Ok(()),
    };
    children.sort_by_key(|entry| entry.path());

    for entry in children {
        check_deadline(deadline)?;
        if candidates.len() >= MAX_WALLPAPERS {
            break;
        }
        let path = entry.path();
        if is_preview_path(&path) {
            continue;
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if is_reparse_point(&metadata) {
            continue;
        }
        if metadata.is_dir() {
            let Some(canonical_child) = fs::canonicalize(&path)
                .ok()
                .filter(|canonical| is_within(canonical_root, canonical))
            else {
                continue;
            };
            walk_directory(
                &canonical_child,
                canonical_root,
                candidates,
                visited,
                depth + 1,
                deadline,
            )?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let Some(format) = image_format(&path) else {
            continue;
        };
        let canonical = match fs::canonicalize(&path) {
            Ok(canonical) if is_within(canonical_root, &canonical) => canonical,
            _ => continue,
        };
        let size = match fs::metadata(&canonical).map(|metadata| metadata.len()) {
            Ok(size) if size > 0 && size <= MAX_SOURCE_BYTES => size,
            _ => continue,
        };
        candidates.push(WallpaperCandidate {
            path: canonical,
            root: canonical_root.to_path_buf(),
            format,
            size,
        });
    }
    Ok(())
}

fn image_format(path: &Path) -> Option<ImageFormat> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some(ImageFormat::Png),
        "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
        "heic" => Some(ImageFormat::Heic),
        _ => None,
    }
}

fn is_preview_path(path: &Path) -> bool {
    path.components().any(|component| {
        let value = component.as_os_str().to_string_lossy().to_ascii_lowercase();
        value == ".thumbnails" || value == "thumbnails" || value.contains("thumbnail")
    })
}

fn display_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("System wallpaper")
        .to_string()
}

fn jpeg_data_url(bytes: Vec<u8>, max_bytes: usize) -> Result<String, String> {
    if !bytes.starts_with(b"\xff\xd8\xff") {
        return Err("invalid wallpaper JPEG".to_string());
    }
    let encoded = base64_encode(&bytes);
    let prefix = "data:image/jpeg;base64,";
    if prefix.len() + encoded.len() > max_bytes {
        return Err("system wallpaper image exceeds the safe data URL limit".to_string());
    }
    Ok(format!("{prefix}{encoded}"))
}

fn build_thumbnail(
    candidate: &WallpaperCandidate,
    temporary: &mut Option<PrivateTempDir>,
    deadline: Instant,
) -> Result<Option<String>, String> {
    verify_candidate_path(candidate)?;

    #[cfg(target_os = "windows")]
    {
        if candidate.format != ImageFormat::Png || candidate.size > thumbnail_raw_limit() as u64 {
            return Ok(None);
        }
        let bytes = read_file_limited(&candidate.path, thumbnail_raw_limit() as u64)?;
        return Ok(png_data_url(bytes, MAX_THUMBNAIL_DATA_URL_BYTES).ok());
    }

    #[cfg(target_os = "macos")]
    {
        for path in
            crate::system_wallpaper_catalog::thumbnail_paths(&candidate.path, &candidate.root)
        {
            let Some(format) = image_format(&path) else {
                continue;
            };
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            let preview = WallpaperCandidate {
                path,
                root: candidate.root.clone(),
                format,
                size: metadata.len(),
            };
            if verify_candidate_path(&preview).is_err() {
                continue;
            }
            let bytes = if format == ImageFormat::Png {
                read_file_limited(&preview.path, thumbnail_raw_limit() as u64).ok()
            } else {
                convert_to_png(&preview.path, temporary, Some(256), deadline).ok()
            };
            if let Some(data_url) =
                bytes.and_then(|bytes| png_data_url(bytes, MAX_THUMBNAIL_DATA_URL_BYTES).ok())
            {
                return Ok(Some(data_url));
            }
        }
        if candidate.format == ImageFormat::Png && candidate.size <= thumbnail_raw_limit() as u64 {
            let bytes = read_file_limited(&candidate.path, thumbnail_raw_limit() as u64)?;
            if let Ok(data_url) = png_data_url(bytes, MAX_THUMBNAIL_DATA_URL_BYTES) {
                return Ok(Some(data_url));
            }
        }
        let bytes = match convert_to_png(&candidate.path, temporary, Some(256), deadline) {
            Ok(bytes) => bytes,
            Err(ConversionError::TimedOut) => {
                return Err("system wallpaper catalog timed out".to_string())
            }
            Err(ConversionError::Unavailable | ConversionError::Failed) => return Ok(None),
        };
        return Ok(png_data_url(bytes, MAX_THUMBNAIL_DATA_URL_BYTES).ok());
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (temporary, deadline);
        if candidate.format != ImageFormat::Png || candidate.size > thumbnail_raw_limit() as u64 {
            return Ok(None);
        }
        let bytes = read_file_limited(&candidate.path, thumbnail_raw_limit() as u64)?;
        Ok(png_data_url(bytes, MAX_THUMBNAIL_DATA_URL_BYTES).ok())
    }
}

fn verify_candidate_path(candidate: &WallpaperCandidate) -> Result<(), String> {
    let metadata = fs::symlink_metadata(&candidate.path)
        .map_err(|_| "system wallpaper file is no longer available".to_string())?;
    if is_reparse_point(&metadata) || !metadata.is_file() {
        return Err("system wallpaper file is no longer a regular file".to_string());
    }
    let canonical = fs::canonicalize(&candidate.path)
        .map_err(|_| "system wallpaper file is no longer available".to_string())?;
    if canonical != candidate.path || !is_within(&candidate.root, &canonical) {
        return Err("system wallpaper file moved outside the system catalog".to_string());
    }
    let size = fs::metadata(&canonical)
        .map_err(|_| "system wallpaper file is no longer available".to_string())?
        .len();
    if size == 0 || size > MAX_SOURCE_BYTES || image_format(&canonical) != Some(candidate.format) {
        return Err("system wallpaper file changed beyond the safe limit".to_string());
    }
    Ok(())
}

fn verify_record_path(record: &SystemWallpaperRecord) -> Result<PathBuf, String> {
    if !is_valid_id(&record.id) {
        return Err("system wallpaper id is invalid".to_string());
    }
    let canonical = fs::canonicalize(&record.path)
        .map_err(|_| "system wallpaper file is no longer available".to_string())?;
    if canonical != record.path || !is_within(&record.root, &canonical) {
        return Err("system wallpaper file moved outside the system catalog".to_string());
    }
    let metadata = fs::metadata(&canonical)
        .map_err(|_| "system wallpaper file is no longer available".to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_SOURCE_BYTES {
        return Err("system wallpaper file exceeds the safe size limit".to_string());
    }
    Ok(canonical)
}

fn is_valid_id(id: &str) -> bool {
    if id == "system-wallpaper-current" {
        return true;
    }
    id.strip_prefix("system-wallpaper-")
        .filter(|suffix| !suffix.is_empty())
        .map(|suffix| suffix.bytes().all(|byte| byte.is_ascii_digit()))
        .unwrap_or(false)
}

fn convert_to_png(
    input: &Path,
    temporary: &mut Option<PrivateTempDir>,
    max_dimension: Option<u32>,
    deadline: Instant,
) -> Result<Vec<u8>, ConversionError> {
    convert_to_image(input, temporary, max_dimension, deadline)
}

fn convert_to_image(
    input: &Path,
    temporary: &mut Option<PrivateTempDir>,
    max_dimension: Option<u32>,
    deadline: Instant,
) -> Result<Vec<u8>, ConversionError> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (input, temporary, max_dimension, deadline);
        return Err(ConversionError::Unavailable);
    }

    #[cfg(target_os = "macos")]
    {
        check_deadline(deadline).map_err(|_| ConversionError::TimedOut)?;
        let directory = match temporary {
            Some(directory) => directory,
            None => {
                *temporary = Some(PrivateTempDir::new().map_err(|_| ConversionError::Unavailable)?);
                temporary.as_mut().expect("temporary directory initialized")
            }
        };
        let source = directory.next_file("source", source_extension(input));
        copy_source_for_conversion(input, &source)?;
        let output = directory.next_file("output", "png");
        let mut command = Command::new(SIPS_PATH);
        if let Some(max_dimension) = max_dimension {
            command.arg("-Z").arg(max_dimension.to_string());
        }
        command
            .arg("-s")
            .arg("format")
            .arg("png")
            .arg(&source)
            .arg("--out")
            .arg(&output)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let process_deadline = std::cmp::min(deadline, Instant::now() + SIPS_TIMEOUT);
        run_bounded(command, process_deadline)?;
        let bytes = read_file_limited(&output, load_raw_limit() as u64)
            .map_err(|_| ConversionError::Failed)?;
        if !is_png(&bytes) {
            return Err(ConversionError::Failed);
        }
        Ok(bytes)
    }
}

fn source_extension(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "png",
        Some("jpg") | Some("jpeg") => "jpg",
        Some("heic") => "heic",
        _ => "image",
    }
}

fn copy_source_for_conversion(input: &Path, output: &Path) -> Result<(), ConversionError> {
    let bytes = read_file_limited(input, MAX_SOURCE_BYTES).map_err(|_| ConversionError::Failed)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|_| ConversionError::Failed)?;
    file.write_all(&bytes).map_err(|_| ConversionError::Failed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConversionError {
    Unavailable,
    TimedOut,
    Failed,
}

fn conversion_error_message(error: ConversionError) -> String {
    match error {
        ConversionError::Unavailable => conversion_unavailable_message(),
        ConversionError::TimedOut => "system wallpaper conversion timed out".to_string(),
        ConversionError::Failed => "system wallpaper conversion failed".to_string(),
    }
}

fn run_bounded(mut command: Command, deadline: Instant) -> Result<(), ConversionError> {
    let mut child = command.spawn().map_err(|_| ConversionError::Unavailable)?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return status
                    .success()
                    .then_some(())
                    .ok_or(ConversionError::Failed);
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ConversionError::TimedOut);
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ConversionError::Failed);
            }
        }
    }
}

fn png_data_url(bytes: Vec<u8>, max_bytes: usize) -> Result<String, String> {
    if !is_png(&bytes) {
        return Err("system wallpaper conversion did not produce PNG".to_string());
    }
    let encoded = base64_encode(&bytes);
    let size = PNG_DATA_URL_PREFIX.len() + encoded.len();
    if size > max_bytes {
        return Err("system wallpaper image exceeds the safe data URL limit".to_string());
    }
    Ok(format!("{PNG_DATA_URL_PREFIX}{encoded}"))
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}

fn thumbnail_raw_limit() -> usize {
    ((MAX_THUMBNAIL_DATA_URL_BYTES - PNG_DATA_URL_PREFIX.len()) / 4) * 3
}

fn load_raw_limit() -> usize {
    ((MAX_LOAD_DATA_URL_BYTES - PNG_DATA_URL_PREFIX.len()) / 4) * 3
}

fn is_within(root: &Path, path: &Path) -> bool {
    path == root || path.strip_prefix(root).is_ok()
}

fn check_deadline(deadline: Instant) -> Result<(), String> {
    if Instant::now() >= deadline {
        Err("system wallpaper catalog timed out".to_string())
    } else {
        Ok(())
    }
}

fn conversion_unavailable_message() -> String {
    #[cfg(target_os = "windows")]
    {
        return "Windows system wallpaper PNG preview conversion is unavailable".to_string();
    }
    #[cfg(target_os = "macos")]
    {
        return "macOS system wallpaper preview conversion is unavailable".to_string();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    "system wallpaper preview conversion is unavailable on this platform".to_string()
}

#[cfg(test)]
#[path = "system_wallpapers_tests.rs"]
mod tests;
