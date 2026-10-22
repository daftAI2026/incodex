// [INPUT]: 依赖应用.bundle、嵌入徽标PNG与AppKit原生桥。
// [OUTPUT]: 对外提供含徽标的多尺寸ICNS字节。
// [POS]: CLI启动图标publisher的渲染适配；不读取宿主偏好设置。
// [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use std::ffi::{CStr, CString};
use std::fs;
use std::os::raw::{c_char, c_int};
use std::path::Path;
use std::ptr;
use std::slice;

const ERROR_CAPACITY: usize = 512;
const MAX_ICNS_BYTES: usize = 64 * 1024 * 1024;

unsafe extern "C" {
    fn incodex_launcher_artwork_render(
        app_path: *const c_char,
        badge_bytes: *const u8,
        badge_length: usize,
        out_bytes: *mut *mut u8,
        out_length: *mut usize,
        error: *mut c_char,
        error_capacity: usize,
    ) -> c_int;
}

pub(crate) fn render(app: &Path) -> Result<Vec<u8>, String> {
    render_with_badge(app, include_bytes!("../assets/launcher-wordmark-badge.png"))
}

fn render_with_badge(app: &Path, badge_png: &[u8]) -> Result<Vec<u8>, String> {
    let resolved = fs::canonicalize(app)
        .map_err(|error| format!("cannot resolve application path {}: {error}", app.display()))?;
    if !resolved.is_dir()
        || !resolved
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
    {
        return Err(format!(
            "launcher artwork source is not an .app directory: {}",
            resolved.display()
        ));
    }
    let path = resolved
        .to_str()
        .ok_or_else(|| format!("application path is not UTF-8: {}", resolved.display()))?;
    let path = CString::new(path).map_err(|_| "application path contains NUL".to_owned())?;

    let mut output = ptr::null_mut();
    let mut output_length = 0;
    let mut error = [0 as c_char; ERROR_CAPACITY];
    let status = unsafe {
        incodex_launcher_artwork_render(
            path.as_ptr(),
            badge_png.as_ptr(),
            badge_png.len(),
            &mut output,
            &mut output_length,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    if status != 1 {
        if !output.is_null() {
            unsafe { libc::free(output.cast()) };
        }
        let message = unsafe { CStr::from_ptr(error.as_ptr()) }.to_string_lossy();
        return Err(if message.is_empty() {
            "native launcher artwork renderer failed without a diagnostic".to_owned()
        } else {
            message.into_owned()
        });
    }
    if output.is_null() || !(8..=MAX_ICNS_BYTES).contains(&output_length) {
        if !output.is_null() {
            unsafe { libc::free(output.cast()) };
        }
        return Err("native launcher artwork renderer returned invalid output bounds".to_owned());
    }

    let bytes = unsafe { slice::from_raw_parts(output, output_length) };
    let valid_icns = bytes.get(..4) == Some(b"icns")
        && u32::from_be_bytes(bytes[4..8].try_into().expect("validated header")) as usize
            == output_length;
    if !valid_icns {
        unsafe { libc::free(output.cast()) };
        return Err("native launcher artwork renderer returned malformed ICNS data".to_owned());
    }
    let owned = bytes.to_vec();
    unsafe { libc::free(output.cast()) };
    Ok(owned)
}

#[cfg(test)]
#[path = "macos_launcher_artwork_tests.rs"]
mod tests;
