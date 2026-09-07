//! [INPUT]: macOS AppKit 的主线程桌面配置 API。
//! [OUTPUT]: 返回当前主屏（缺失时首屏）的本地壁纸路径快照。
//! [POS]: open 启动边界；只查询路径，后台资源库负责验证与解码。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use std::{
    ffi::{c_void, CStr},
    os::unix::ffi::OsStrExt,
    path::PathBuf,
};

// 仅在实验 open 主线程按需装载 AppKit，不增加普通 CLI 的框架启动成本。
#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const std::ffi::c_char) -> *mut c_void;
    fn sel_registerName(name: *const std::ffi::c_char) -> *mut c_void;
    fn objc_msgSend();
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
    fn pthread_main_np() -> i32;
}

pub(crate) fn current_desktop() -> Option<PathBuf> {
    unsafe {
        if pthread_main_np() != 1 {
            return None;
        }
        let framework = libc::dlopen(
            c"/System/Library/Frameworks/AppKit.framework/AppKit".as_ptr(),
            libc::RTLD_LAZY,
        );
        if framework.is_null() {
            return None;
        }
        let pool = objc_autoreleasePoolPush();
        let result = query_desktop();
        objc_autoreleasePoolPop(pool);
        libc::dlclose(framework);
        result
    }
}

unsafe fn query_desktop() -> Option<PathBuf> {
    let send: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
        std::mem::transmute(objc_msgSend as *const ());
    let send_arg: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void =
        std::mem::transmute(objc_msgSend as *const ());
    let send_count: unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize =
        std::mem::transmute(objc_msgSend as *const ());
    let send_index: unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void =
        std::mem::transmute(objc_msgSend as *const ());
    let screens = objc_getClass(c"NSScreen".as_ptr());
    let mut screen = send(screens, sel_registerName(c"mainScreen".as_ptr()));
    if screen.is_null() {
        let all = send(screens, sel_registerName(c"screens".as_ptr()));
        if send_count(all, sel_registerName(c"count".as_ptr())) == 0 {
            return None;
        }
        screen = send_index(all, sel_registerName(c"objectAtIndex:".as_ptr()), 0);
    }
    let workspace = send(
        objc_getClass(c"NSWorkspace".as_ptr()),
        sel_registerName(c"sharedWorkspace".as_ptr()),
    );
    let url = send_arg(
        workspace,
        sel_registerName(c"desktopImageURLForScreen:".as_ptr()),
        screen,
    );
    if url.is_null() {
        return None;
    }
    let is_file: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i8 =
        std::mem::transmute(objc_msgSend as *const ());
    if is_file(url, sel_registerName(c"isFileURL".as_ptr())) == 0 {
        return None;
    }
    let path = send(url, sel_registerName(c"fileSystemRepresentation".as_ptr()));
    if path.is_null() {
        return None;
    }
    local_path(CStr::from_ptr(path.cast()).to_bytes())
}

fn local_path(bytes: &[u8]) -> Option<PathBuf> {
    let path = PathBuf::from(std::ffi::OsStr::from_bytes(bytes));
    path.is_absolute().then_some(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn background_thread_never_queries_appkit() {
        assert!(std::thread::spawn(super::current_desktop)
            .join()
            .unwrap()
            .is_none());
    }
    #[test]
    fn filesystem_path_preserves_native_bytes() {
        assert_eq!(
            super::local_path(b"/tmp/wallpaper.heic"),
            Some(std::path::PathBuf::from("/tmp/wallpaper.heic"))
        );
        assert!(super::local_path(b"relative.heic").is_none());
        assert!(super::local_path(b"").is_none());
    }
}
