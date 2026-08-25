use std::ffi::{c_void, CStr, CString};

use crate::live_window::WindowCandidate;

const ON_SCREEN_ONLY: u32 = 1 << 0;
const EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;
const SINT32_NUMBER_TYPE: i32 = 3;

#[repr(C)]
#[derive(Default)]
struct Point {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Default)]
struct Size {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Default)]
struct Rect {
    origin: Point,
    size: Size,
}

type WindowListFn = unsafe extern "C" fn(u32, u32) -> *const c_void;
type RectFromDictionaryFn = unsafe extern "C" fn(*const c_void, *mut Rect) -> bool;
type ReleaseFn = unsafe extern "C" fn(*const c_void);
type ArrayCountFn = unsafe extern "C" fn(*const c_void) -> isize;
type ArrayValueFn = unsafe extern "C" fn(*const c_void, isize) -> *const c_void;
type DictionaryValueFn = unsafe extern "C" fn(*const c_void, *const c_void) -> *const c_void;
type NumberValueFn = unsafe extern "C" fn(*const c_void, i32, *mut c_void) -> bool;

struct FrameworkHandle(*mut c_void);

impl FrameworkHandle {
    fn open(path: &str) -> Result<Self, String> {
        let path = CString::new(path).map_err(|error| error.to_string())?;
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
        if handle.is_null() {
            return Err(dynamic_loader_error("cannot load macOS window framework"));
        }
        Ok(Self(handle))
    }

    fn symbol(&self, name: &str) -> Result<*mut c_void, String> {
        let name = CString::new(name).map_err(|error| error.to_string())?;
        unsafe {
            libc::dlerror();
        }
        let symbol = unsafe { libc::dlsym(self.0, name.as_ptr()) };
        if symbol.is_null() {
            return Err(dynamic_loader_error(&format!(
                "macOS window framework is missing symbol {}",
                name.to_string_lossy()
            )));
        }
        Ok(symbol)
    }

    fn data_pointer(&self, name: &str) -> Result<*const c_void, String> {
        let symbol = self.symbol(name)?;
        let value = unsafe { *(symbol as *const *const c_void) };
        if value.is_null() {
            return Err(format!("macOS window framework returned a null {name}"));
        }
        Ok(value)
    }
}

impl Drop for FrameworkHandle {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.0);
        }
    }
}

fn dynamic_loader_error(prefix: &str) -> String {
    let error = unsafe { libc::dlerror() };
    if error.is_null() {
        return prefix.to_string();
    }
    let detail = unsafe { CStr::from_ptr(error) }.to_string_lossy();
    format!("{prefix}: {detail}")
}

struct WindowApi {
    _core_graphics: FrameworkHandle,
    _core_foundation: FrameworkHandle,
    window_list: WindowListFn,
    rect_from_dictionary: RectFromDictionaryFn,
    release: ReleaseFn,
    array_count: ArrayCountFn,
    array_value: ArrayValueFn,
    dictionary_value: DictionaryValueFn,
    number_value: NumberValueFn,
    owner_pid_key: *const c_void,
    layer_key: *const c_void,
    bounds_key: *const c_void,
}

impl WindowApi {
    fn load() -> Result<Self, String> {
        let core_graphics = FrameworkHandle::open(
            "/System/Library/Frameworks/CoreGraphics.framework/Versions/A/CoreGraphics",
        )?;
        let core_foundation = FrameworkHandle::open(
            "/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation",
        )?;
        let window_list = unsafe {
            std::mem::transmute::<*mut c_void, WindowListFn>(
                core_graphics.symbol("CGWindowListCopyWindowInfo")?,
            )
        };
        let rect_from_dictionary = unsafe {
            std::mem::transmute::<*mut c_void, RectFromDictionaryFn>(
                core_graphics.symbol("CGRectMakeWithDictionaryRepresentation")?,
            )
        };
        let release = unsafe {
            std::mem::transmute::<*mut c_void, ReleaseFn>(core_foundation.symbol("CFRelease")?)
        };
        let array_count = unsafe {
            std::mem::transmute::<*mut c_void, ArrayCountFn>(
                core_foundation.symbol("CFArrayGetCount")?,
            )
        };
        let array_value = unsafe {
            std::mem::transmute::<*mut c_void, ArrayValueFn>(
                core_foundation.symbol("CFArrayGetValueAtIndex")?,
            )
        };
        let dictionary_value = unsafe {
            std::mem::transmute::<*mut c_void, DictionaryValueFn>(
                core_foundation.symbol("CFDictionaryGetValue")?,
            )
        };
        let number_value = unsafe {
            std::mem::transmute::<*mut c_void, NumberValueFn>(
                core_foundation.symbol("CFNumberGetValue")?,
            )
        };
        let owner_pid_key = core_graphics.data_pointer("kCGWindowOwnerPID")?;
        let layer_key = core_graphics.data_pointer("kCGWindowLayer")?;
        let bounds_key = core_graphics.data_pointer("kCGWindowBounds")?;
        Ok(Self {
            _core_graphics: core_graphics,
            _core_foundation: core_foundation,
            window_list,
            rect_from_dictionary,
            release,
            array_count,
            array_value,
            dictionary_value,
            number_value,
            owner_pid_key,
            layer_key,
            bounds_key,
        })
    }
}

unsafe fn dictionary_i32(
    api: &WindowApi,
    dictionary: *const c_void,
    key: *const c_void,
    number_type: i32,
) -> Option<i32> {
    let number = unsafe { (api.dictionary_value)(dictionary, key) };
    if number.is_null() {
        return None;
    }
    let mut value = 0_i32;
    unsafe {
        (api.number_value)(number, number_type, (&mut value as *mut i32).cast()).then_some(value)
    }
}

fn integral_i32(value: f64) -> Option<i32> {
    if !value.is_finite() || value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
        return None;
    }
    Some(value.round() as i32)
}

struct ArrayGuard<'a> {
    value: *const c_void,
    release: &'a ReleaseFn,
}
impl Drop for ArrayGuard<'_> {
    fn drop(&mut self) {
        unsafe { (self.release)(self.value) };
    }
}

pub(crate) fn system_window_candidates() -> Result<Vec<WindowCandidate>, String> {
    let api = WindowApi::load()?;
    let array = unsafe { (api.window_list)(ON_SCREEN_ONLY | EXCLUDE_DESKTOP_ELEMENTS, 0) };
    if array.is_null() {
        return Err("CoreGraphics did not return a window list".into());
    }
    let _guard = ArrayGuard {
        value: array,
        release: &api.release,
    };
    let count = unsafe { (api.array_count)(array) };
    let mut windows = Vec::new();
    for index in 0..count {
        let dictionary = unsafe { (api.array_value)(array, index) };
        if dictionary.is_null() {
            continue;
        }
        let Some(pid) =
            (unsafe { dictionary_i32(&api, dictionary, api.owner_pid_key, SINT32_NUMBER_TYPE) })
        else {
            continue;
        };
        let Some(layer) =
            (unsafe { dictionary_i32(&api, dictionary, api.layer_key, SINT32_NUMBER_TYPE) })
        else {
            continue;
        };
        let bounds_dictionary = unsafe { (api.dictionary_value)(dictionary, api.bounds_key) };
        if bounds_dictionary.is_null() {
            continue;
        }
        let mut rect = Rect::default();
        if !unsafe { (api.rect_from_dictionary)(bounds_dictionary, &mut rect) } {
            continue;
        }
        let (Some(x), Some(y), Some(width), Some(height)) = (
            integral_i32(rect.origin.x),
            integral_i32(rect.origin.y),
            integral_i32(rect.size.width),
            integral_i32(rect.size.height),
        ) else {
            continue;
        };
        windows.push(WindowCandidate {
            pid,
            layer,
            x,
            y,
            width,
            height,
        });
    }
    Ok(windows)
}
