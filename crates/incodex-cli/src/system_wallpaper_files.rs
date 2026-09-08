//! [INPUT]: 原生壁纸 adapter 的主机路径与明确字节上限。
//! [OUTPUT]: no-follow 有界读取、reparse 检查与独占私有临时目录。
//! [POS]: 静态图片与视频下载共享的文件安全边界，不进行目录发现或网络请求。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use std::fs::{self, File, OpenOptions};
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn read_file_limited(path: &Path, max_bytes: u64) -> Result<Vec<u8>, String> {
    let path_metadata = fs::symlink_metadata(path)
        .map_err(|_| "system wallpaper file cannot be read".to_string())?;
    if is_reparse_point(&path_metadata) || !path_metadata.is_file() {
        return Err("system wallpaper file exceeds the safe read limit".to_string());
    }
    let file = open_wallpaper_file(path)?;
    let metadata = file
        .metadata()
        .map_err(|_| "system wallpaper file cannot be read".to_string())?;
    if is_reparse_point(&metadata)
        || !metadata.is_file()
        || metadata.len() > max_bytes
        || metadata.len() > usize::MAX as u64
    {
        return Err("system wallpaper file exceeds the safe read limit".to_string());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| "system wallpaper file cannot be read".to_string())?;
    if bytes.len() as u64 > max_bytes {
        return Err("system wallpaper file changed beyond the safe read limit".to_string());
    }
    Ok(bytes)
}

fn open_wallpaper_file(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    #[cfg(windows)]
    options.custom_flags(0x0020_0000);
    options
        .open(path)
        .map_err(|_| "system wallpaper file cannot be read".to_string())
}

#[cfg(windows)]
pub(crate) fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
pub(crate) fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

pub(crate) struct PrivateTempDir {
    path: PathBuf,
    next_file: u64,
}

impl PrivateTempDir {
    pub(crate) fn new() -> Result<Self, String> {
        for _ in 0..32 {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "incodex-wallpaper-convert-{}-{sequence}",
                std::process::id()
            ));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => {
                    return Ok(Self { path, next_file: 0 });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => {
                    return Err("cannot create private wallpaper conversion directory".to_string())
                }
            }
        }
        Err("cannot create private wallpaper conversion directory".to_string())
    }

    pub(crate) fn next_file(&mut self, prefix: &str, extension: &str) -> PathBuf {
        let sequence = self.next_file;
        self.next_file = self.next_file.saturating_add(1);
        self.path.join(format!("{prefix}-{sequence}.{extension}"))
    }
}

impl Drop for PrivateTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
