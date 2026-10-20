//! [INPUT]: 依赖 CoreGraphics 窗口列表与 CoreFoundation 字典、数组和数值读取。
//! [OUTPUT]: 提供普通窗口边界快照与保留坐标的窗口身份候选快照。
//! [POS]: live_window 的 macOS 采集适配器；观察器持有加载器，避免每帧重复解析符号。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use std::ffi::c_void;

use crate::framework::FrameworkHandle;
use crate::live_window::WindowCandidate as BoundsCandidate;
use crate::window_server::{WindowCandidate, WindowListSnapshot};

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

pub(crate) struct WindowListApi {
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
    window_number_key: Option<*const c_void>,
    layer_key: *const c_void,
    bounds_key: *const c_void,
}

struct SystemWindowCandidate {
    pid: i32,
    window_id: Option<u32>,
    layer: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl WindowListApi {
    pub(crate) fn load_for_observer() -> Result<Self, String> {
        Self::load(true)
    }

    fn load_for_bounds() -> Result<Self, String> {
        Self::load(false)
    }

    fn load(include_window_numbers: bool) -> Result<Self, String> {
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
        let window_number_key = include_window_numbers
            .then(|| core_graphics.data_pointer("kCGWindowNumber"))
            .transpose()?;
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
            window_number_key,
            layer_key,
            bounds_key,
        })
    }

    pub(crate) fn observer_snapshot(&self) -> Result<WindowListSnapshot, String> {
        let windows = self
            .read_windows()?
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        let windows = windows
            .into_iter()
            .map(|window| WindowCandidate {
                pid: window.pid,
                window_id: window.window_id,
                layer: window.layer,
                x: window.x,
                y: window.y,
                width: window.width,
                height: window.height,
            })
            .collect();
        Ok(WindowListSnapshot {
            complete: true,
            windows,
        })
    }

    fn read_windows(&self) -> Result<Vec<Result<SystemWindowCandidate, String>>, String> {
        let array = unsafe { (self.window_list)(ON_SCREEN_ONLY | EXCLUDE_DESKTOP_ELEMENTS, 0) };
        if array.is_null() {
            return Err("CoreGraphics did not return a window list".into());
        }
        let _guard = ArrayGuard {
            value: array,
            release: &self.release,
        };
        let count = unsafe { (self.array_count)(array) };
        if count < 0 {
            return Err("CoreGraphics returned a negative window count".into());
        }
        Ok((0..count)
            .map(|index| {
                let dictionary = unsafe { (self.array_value)(array, index) };
                if dictionary.is_null() {
                    return Err("CoreGraphics returned a null window record".into());
                }
                self.read_window(dictionary)
            })
            .collect())
    }

    fn read_window(&self, dictionary: *const c_void) -> Result<SystemWindowCandidate, String> {
        let number = |key| unsafe { dictionary_i32(self, dictionary, key, SINT32_NUMBER_TYPE) };
        let pid = number(self.owner_pid_key)
            .ok_or_else(|| "CoreGraphics window record has no owner PID".to_owned())?;
        let window_id = self
            .window_number_key
            .and_then(number)
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value != 0);
        let layer = number(self.layer_key)
            .ok_or_else(|| "CoreGraphics window record has no layer".to_owned())?;
        let bounds = unsafe { (self.dictionary_value)(dictionary, self.bounds_key) };
        if bounds.is_null() {
            return Err("CoreGraphics window record has no bounds".into());
        }
        let mut rect = Rect::default();
        if !unsafe { (self.rect_from_dictionary)(bounds, &mut rect) } {
            return Err("CoreGraphics returned invalid window bounds".into());
        }
        let (Some(x), Some(y), Some(width), Some(height)) = (
            integral_i32(rect.origin.x),
            integral_i32(rect.origin.y),
            integral_i32(rect.size.width),
            integral_i32(rect.size.height),
        ) else {
            return Err("CoreGraphics returned non-integral window bounds".into());
        };
        Ok(SystemWindowCandidate {
            pid,
            window_id,
            layer,
            x,
            y,
            width,
            height,
        })
    }

    fn bounds_candidates(&self) -> Result<Vec<BoundsCandidate>, String> {
        Ok(self
            .read_windows()?
            .into_iter()
            .filter_map(Result::ok)
            .map(|window| BoundsCandidate {
                pid: window.pid,
                layer: window.layer,
                x: window.x,
                y: window.y,
                width: window.width,
                height: window.height,
            })
            .collect())
    }
}

unsafe fn dictionary_i32(
    api: &WindowListApi,
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

pub(crate) fn system_window_candidates() -> Result<Vec<BoundsCandidate>, String> {
    WindowListApi::load_for_bounds()?.bounds_candidates()
}
