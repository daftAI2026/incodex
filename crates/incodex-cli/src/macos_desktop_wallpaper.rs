//! [INPUT]: macOS AppKit 的主线程桌面配置 API。
//! [OUTPUT]: 返回当前主屏（缺失时首屏）的本地壁纸路径快照。
//! [POS]: open 启动边界；只查询路径，后台资源库负责验证与解码。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use std::{ffi::{c_void, CStr}, os::unix::ffi::OsStrExt, path::PathBuf};

pub(crate) fn current_desktop() -> Option<PathBuf> { None }

fn local_path(_bytes: &[u8]) -> Option<PathBuf> { None }

#[cfg(test)]
mod tests {
    #[test]
    fn background_thread_never_queries_appkit() {
        assert!(std::thread::spawn(super::current_desktop).join().unwrap().is_none());
    }
    #[test]
    fn filesystem_path_preserves_native_bytes() {
        assert_eq!(super::local_path(b"/tmp/wallpaper.heic"), Some(std::path::PathBuf::from("/tmp/wallpaper.heic")));
        assert!(super::local_path(b"relative.heic").is_none());
        assert!(super::local_path(b"").is_none());
    }
}
