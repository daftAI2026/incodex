//! [INPUT]: 依赖 SkyLight 窗口查询与 CoreFoundation 数组、数字和释放函数。
//! [OUTPUT]: 提供按固定窗口 ID 读取 PID、标签和 ordered-in 状态的查询器。
//! [POS]: WindowServer 私有 ABI 适配层；不读标题，不调用授权 API。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
//
// SkyLight 是私有 ABI，不是 macOS 的稳定公共契约。运行时缺少符号时构造器失败。
//

use std::ffi::c_void;

use crate::framework::FrameworkHandle;
use crate::window_server::WindowServerRow;

const CF_NUMBER_SINT64_TYPE: i32 = 4; // kCFNumberSInt64Type
const QUERY_OPTIONS_WITHOUT_TITLES: i32 = 0; // 不请求 title 字段

type ReleaseFn = unsafe extern "C" fn(*const c_void);
type NumberCreateFn = unsafe extern "C" fn(*const c_void, i32, *const c_void) -> *const c_void;
type ArrayCreateFn = unsafe extern "C" fn(
    *const c_void,
    *const *const c_void,
    isize,
    *const c_void,
) -> *const c_void;
type MainConnectionIdFn = unsafe extern "C" fn() -> i32;
type QueryWindowsFn = unsafe extern "C" fn(i32, *const c_void, i32) -> *const c_void;
type QueryCopyWindowsFn = unsafe extern "C" fn(*const c_void) -> *const c_void;
type IteratorAdvanceFn = unsafe extern "C" fn(*const c_void) -> bool;
type IteratorWindowIdFn = unsafe extern "C" fn(*const c_void) -> u32;
type IteratorPidFn = unsafe extern "C" fn(*const c_void) -> i32;
type IteratorTagsFn = unsafe extern "C" fn(*const c_void) -> u64;
type IsOrderedInFn = unsafe extern "C" fn(i32, u32, *mut u8) -> i32;

pub(crate) struct WindowServerApi {
    _sky_light: FrameworkHandle,
    _core_foundation: FrameworkHandle,
    release: ReleaseFn,
    number_create: NumberCreateFn,
    array_create: ArrayCreateFn,
    query_windows: QueryWindowsFn,
    query_copy_windows: QueryCopyWindowsFn,
    iterator_advance: IteratorAdvanceFn,
    iterator_window_id: IteratorWindowIdFn,
    iterator_pid: IteratorPidFn,
    iterator_tags: IteratorTagsFn,
    is_ordered_in: IsOrderedInFn,
    connection_id: i32,
}

impl WindowServerApi {
    pub(crate) fn load() -> Result<Self, String> {
        let sky_light = FrameworkHandle::open(
            "/System/Library/PrivateFrameworks/SkyLight.framework/Versions/A/SkyLight",
        )?;
        let core_foundation = FrameworkHandle::open(
            "/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation",
        )?;
        let release = unsafe { load_symbol::<ReleaseFn>(&core_foundation, "CFRelease")? };
        let number_create =
            unsafe { load_symbol::<NumberCreateFn>(&core_foundation, "CFNumberCreate")? };
        let array_create =
            unsafe { load_symbol::<ArrayCreateFn>(&core_foundation, "CFArrayCreate")? };
        let main_connection_id =
            unsafe { load_symbol::<MainConnectionIdFn>(&sky_light, "CGSMainConnectionID")? };
        let query_windows =
            unsafe { load_symbol::<QueryWindowsFn>(&sky_light, "SLSWindowQueryWindows")? };
        let query_copy_windows = unsafe {
            load_symbol::<QueryCopyWindowsFn>(&sky_light, "SLSWindowQueryResultCopyWindows")?
        };
        let iterator_advance =
            unsafe { load_symbol::<IteratorAdvanceFn>(&sky_light, "SLSWindowIteratorAdvance")? };
        let iterator_window_id = unsafe {
            load_symbol::<IteratorWindowIdFn>(&sky_light, "SLSWindowIteratorGetWindowID")?
        };
        let iterator_pid =
            unsafe { load_symbol::<IteratorPidFn>(&sky_light, "SLSWindowIteratorGetPID")? };
        let iterator_tags =
            unsafe { load_symbol::<IteratorTagsFn>(&sky_light, "SLSWindowIteratorGetTags")? };
        let is_ordered_in =
            unsafe { load_symbol::<IsOrderedInFn>(&sky_light, "SLSWindowIsOrderedIn")? };
        let connection_id = unsafe { main_connection_id() };
        if connection_id == 0 {
            return Err("SkyLight returned an invalid main connection ID".into());
        }
        Ok(Self {
            _sky_light: sky_light,
            _core_foundation: core_foundation,
            release,
            number_create,
            array_create,
            query_windows,
            query_copy_windows,
            iterator_advance,
            iterator_window_id,
            iterator_pid,
            iterator_tags,
            is_ordered_in,
            connection_id,
        })
    }

    pub(crate) fn query_window(&self, window_id: u32) -> Result<Vec<WindowServerRow>, String> {
        if window_id == 0 {
            return Err("WindowServer query requires a nonzero window ID".into());
        }
        let window_number = i64::from(window_id);
        let number = unsafe {
            (self.number_create)(
                std::ptr::null(),
                CF_NUMBER_SINT64_TYPE,
                (&window_number as *const i64).cast(),
            )
        };
        let _number_guard = CfGuard::new(number, &self.release)?;
        let window_numbers = [number];
        let array = unsafe {
            (self.array_create)(
                std::ptr::null(),
                window_numbers.as_ptr(),
                window_numbers.len() as isize,
                std::ptr::null(),
            )
        };
        let _array_guard = CfGuard::new(array, &self.release)?;
        let query = unsafe {
            (self.query_windows)(self.connection_id, array, QUERY_OPTIONS_WITHOUT_TITLES)
        };
        let _query_guard = CfGuard::new(query, &self.release)?;
        let iterator = unsafe { (self.query_copy_windows)(query) };
        let _iterator_guard = CfGuard::new(iterator, &self.release)?;

        let mut rows = Vec::with_capacity(2);
        while unsafe { (self.iterator_advance)(iterator) } {
            let observed_window_id = unsafe { (self.iterator_window_id)(iterator) };
            let pid = unsafe { (self.iterator_pid)(iterator) };
            let tags = unsafe { (self.iterator_tags)(iterator) };
            let mut ordered_in = 0_u8;
            let status = unsafe {
                (self.is_ordered_in)(self.connection_id, observed_window_id, &mut ordered_in)
            };
            if status != 0 {
                return Err(format!(
                    "SkyLight could not read ordered-in for window {observed_window_id} (status {status})"
                ));
            }
            rows.push(WindowServerRow {
                pid,
                window_id: observed_window_id,
                tags,
                ordered_in: Some(ordered_in != 0),
            });
            if rows.len() == 2 {
                break;
            }
        }
        Ok(rows)
    }
}

/// # Safety
/// T 必须是目标 C 符号的精确函数指针类型。
unsafe fn load_symbol<T: Copy>(framework: &FrameworkHandle, name: &str) -> Result<T, String> {
    let symbol = framework.symbol(name)?;
    Ok(unsafe { std::mem::transmute_copy::<*mut c_void, T>(&symbol) })
}

struct CfGuard<'a> {
    value: *const c_void,
    release: &'a ReleaseFn,
}

impl<'a> CfGuard<'a> {
    fn new(value: *const c_void, release: &'a ReleaseFn) -> Result<Self, String> {
        if value.is_null() {
            return Err("CoreFoundation returned a null WindowServer query object".into());
        }
        Ok(Self { value, release })
    }
}

impl Drop for CfGuard<'_> {
    fn drop(&mut self) {
        unsafe { (self.release)(self.value) };
    }
}
