//! [INPUT]: 下载器独占的私有本地 MOV 文件和 AVFoundation 的受限媒体解码。
//! [OUTPUT]: 提供首帧、最长边 2600px 的 JPEG，编码复用 macos_image_io。
//! [POS]: Shot 视频壁纸到静态编辑图片的 adapter；禁止所有外部媒体引用，不修改系统壁纸。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use std::ffi::{c_void, CString};
use std::path::Path;
use std::sync::OnceLock;
type Obj = *mut c_void;
#[repr(C)]
#[derive(Clone, Copy)]
struct Time {
    value: i64,
    scale: i32,
    flags: u32,
    epoch: i64,
}
#[repr(C)]
struct Size {
    width: f64,
    height: f64,
}
#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const std::ffi::c_char) -> Obj;
    fn sel_registerName(name: *const std::ffi::c_char) -> Obj;
    fn objc_msgSend();
    fn objc_autoreleasePoolPush() -> Obj;
    fn objc_autoreleasePoolPop(pool: Obj);
}
static LOADED: OnceLock<bool> = OnceLock::new();

/// 路径必须来自下载器当前持有的 0700 私有目录，不能传入 renderer 或清单路径。
pub(crate) fn frame_jpeg(path: &Path, max_bytes: usize) -> Result<Vec<u8>, String> {
    if !*LOADED.get_or_init(|| unsafe {
        // ObjC 类型注册在进程内有效；框架惰性载入后维持到进程退出。
        !libc::dlopen(
            c"/System/Library/Frameworks/AVFoundation.framework/AVFoundation".as_ptr(),
            libc::RTLD_LAZY,
        )
        .is_null()
            && !libc::dlopen(
                c"/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics".as_ptr(),
                libc::RTLD_LAZY,
            )
            .is_null()
    }) {
        return Err("AVFoundation unavailable".into());
    }
    use std::os::unix::ffi::OsStrExt;
    let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| "invalid video path")?;
    unsafe {
        let pool = objc_autoreleasePoolPush();
        let result = generate(&path, max_bytes);
        objc_autoreleasePoolPop(pool);
        result
    }
}

unsafe fn generate(path: &CString, max_bytes: usize) -> Result<Vec<u8>, String> {
    let send1: unsafe extern "C" fn(Obj, Obj, Obj) -> Obj =
        std::mem::transmute(objc_msgSend as *const ());
    let send2: unsafe extern "C" fn(Obj, Obj, Obj, Obj) -> Obj =
        std::mem::transmute(objc_msgSend as *const ());
    let number: unsafe extern "C" fn(Obj, Obj, usize) -> Obj =
        std::mem::transmute(objc_msgSend as *const ());
    let string = send1(
        objc_getClass(c"NSString".as_ptr()),
        sel_registerName(c"stringWithUTF8String:".as_ptr()),
        path.as_ptr() as Obj,
    );
    if string.is_null() {
        return Err("video path is not valid UTF-8".into());
    }
    let url = send1(
        objc_getClass(c"NSURL".as_ptr()),
        sel_registerName(c"fileURLWithPath:".as_ptr()),
        string,
    );
    let key = libc::dlsym(
        libc::RTLD_DEFAULT,
        c"AVURLAssetReferenceRestrictionsKey".as_ptr(),
    );
    if key.is_null() {
        return Err("AVFoundation reference policy unavailable".into());
    }
    let restriction = number(
        objc_getClass(c"NSNumber".as_ptr()),
        sel_registerName(c"numberWithUnsignedLong:".as_ptr()),
        0xffff,
    );
    let options = send2(
        objc_getClass(c"NSDictionary".as_ptr()),
        sel_registerName(c"dictionaryWithObject:forKey:".as_ptr()),
        restriction,
        *key.cast::<Obj>(),
    );
    if url.is_null() || options.is_null() {
        return Err("cannot enforce local media restrictions".into());
    }
    let asset = send2(
        objc_getClass(c"AVURLAsset".as_ptr()),
        sel_registerName(c"URLAssetWithURL:options:".as_ptr()),
        url,
        options,
    );
    if asset.is_null() {
        return Err("cannot open wallpaper video asset".into());
    }
    let generator = send1(
        objc_getClass(c"AVAssetImageGenerator".as_ptr()),
        sel_registerName(c"assetImageGeneratorWithAsset:".as_ptr()),
        asset,
    );
    if generator.is_null() {
        return Err("cannot create wallpaper frame generator".into());
    }
    let set_size: unsafe extern "C" fn(Obj, Obj, Size) =
        std::mem::transmute(objc_msgSend as *const ());
    set_size(
        generator,
        sel_registerName(c"setMaximumSize:".as_ptr()),
        Size {
            width: 2600.0,
            height: 2600.0,
        },
    );
    let set_bool: unsafe extern "C" fn(Obj, Obj, i8) =
        std::mem::transmute(objc_msgSend as *const ());
    set_bool(
        generator,
        sel_registerName(c"setAppliesPreferredTrackTransform:".as_ptr()),
        1,
    );
    for symbol in [c"CGImageRelease", c"CGImageGetWidth", c"CGImageGetHeight"] {
        if libc::dlsym(libc::RTLD_DEFAULT, symbol.as_ptr()).is_null() {
            return Err("CoreGraphics frame API unavailable".into());
        }
    }
    let copy: unsafe extern "C" fn(Obj, Obj, Time, *mut Time, *mut Obj) -> *const c_void =
        std::mem::transmute(objc_msgSend as *const ());
    let zero = Time {
        value: 0,
        scale: 600,
        flags: 1,
        epoch: 0,
    };
    let mut actual = zero;
    let mut error = std::ptr::null_mut();
    let image = copy(
        generator,
        sel_registerName(c"copyCGImageAtTime:actualTime:error:".as_ptr()),
        zero,
        &mut actual,
        &mut error,
    );
    if image.is_null() {
        return Err("cannot decode wallpaper video frame".into());
    }
    let release: unsafe extern "C" fn(*const c_void) =
        std::mem::transmute(libc::dlsym(libc::RTLD_DEFAULT, c"CGImageRelease".as_ptr()));
    let width: unsafe extern "C" fn(*const c_void) -> usize =
        std::mem::transmute(libc::dlsym(libc::RTLD_DEFAULT, c"CGImageGetWidth".as_ptr()));
    let height: unsafe extern "C" fn(*const c_void) -> usize = std::mem::transmute(libc::dlsym(
        libc::RTLD_DEFAULT,
        c"CGImageGetHeight".as_ptr(),
    ));
    let result =
        if width(image) == 0 || height(image) == 0 || width(image).max(height(image)) > 2600 {
            Err("wallpaper frame exceeds its pixel bounds".into())
        } else {
            crate::macos_image_io::encode_wallpaper_frame(image, max_bytes)
        };
    release(image);
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn local_system_video_produces_a_bounded_real_frame() {
        let path = std::path::Path::new("/System/Library/Wallpapers/.default/Golden Gate.mov");
        if !path.exists() {
            return;
        }
        let jpeg = super::frame_jpeg(path, 8 * 1024 * 1024).unwrap();
        let (width, height) = crate::macos_image_io::image_dimensions(&jpeg).unwrap();
        assert!(width.max(height) <= 2600 && width.min(height) > 1000);
    }
}
