//! Read-only native window state for the exact isolated `open` child PID.
use std::ffi::{c_void, CString};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenWindowObservation {
    Present,
    Minimized,
    Missing,
    Unknown,
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    static kAXTrustedCheckOptionPrompt: *const c_void;
    fn AXUIElementCreateApplication(pid: i32) -> *const c_void;
    fn AXUIElementCopyAttributeValue(
        element: *const c_void,
        attribute: *const c_void,
        value: *mut *const c_void,
    ) -> i32;
    fn AXUIElementGetPid(element: *const c_void, pid: *mut i32) -> i32;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFBooleanTrue: *const c_void;
    fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        count: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> *const c_void;
    fn CFStringCreateWithCString(
        allocator: *const c_void,
        text: *const i8,
        encoding: u32,
    ) -> *const c_void;
    fn CFRelease(value: *const c_void);
    fn CFGetTypeID(value: *const c_void) -> usize;
    fn CFArrayGetTypeID() -> usize;
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
    fn CFBooleanGetTypeID() -> usize;
    fn CFBooleanGetValue(value: *const c_void) -> bool;
}

/// This is a non-prompting preflight for the CLI, not a probe of Codex's own grant.
pub fn open_window_observer_trusted() -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe { AXIsProcessTrusted() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Request the standard macOS Accessibility consent prompt for this process.
/// The prompt is asynchronous; callers must recheck `open_window_observer_trusted`.
pub fn prompt_open_window_observer_accessibility() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let key = unsafe { kAXTrustedCheckOptionPrompt };
        let value = unsafe { kCFBooleanTrue };
        if key.is_null() || value.is_null() {
            return Err("macOS Accessibility request options are unavailable".into());
        }
        let options = unsafe {
            CFDictionaryCreate(
                std::ptr::null(),
                &key,
                &value,
                1,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if options.is_null() {
            return Err("unable to create macOS Accessibility request options".into());
        }
        unsafe {
            AXIsProcessTrustedWithOptions(options);
            CFRelease(options);
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("macOS Accessibility requests are unavailable on this platform".into())
    }
}

#[cfg(target_os = "macos")]
fn attribute(name: &str) -> Option<*const c_void> {
    let text = CString::new(name).ok()?;
    let value = unsafe { CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), 0x0800_0100) };
    (!value.is_null()).then_some(value)
}

/// After the primary page has been observed ready, an empty AXWindows list is the
/// current Codex macOS close-as-hide state. A minimized window stays in AXWindows.
/// Any failed or ambiguous read remains Unknown; it can never authorize cleanup.
pub fn observe_open_window(pid: i32) -> OpenWindowObservation {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        return OpenWindowObservation::Unknown;
    }
    #[cfg(target_os = "macos")]
    {
        if pid <= 0 || !open_window_observer_trusted() {
            return OpenWindowObservation::Unknown;
        }
        let Some(windows_key) = attribute("AXWindows") else {
            return OpenWindowObservation::Unknown;
        };
        let app = unsafe { AXUIElementCreateApplication(pid) };
        if app.is_null() {
            unsafe { CFRelease(windows_key) };
            return OpenWindowObservation::Unknown;
        }
        let mut observed_pid = 0;
        if unsafe { AXUIElementGetPid(app, &mut observed_pid) } != 0 || observed_pid != pid {
            unsafe {
                CFRelease(windows_key);
                CFRelease(app);
            }
            return OpenWindowObservation::Unknown;
        }
        let hidden = unsafe { bool_attribute(app, "AXHidden") };
        if hidden != Some(false) {
            unsafe {
                CFRelease(windows_key);
                CFRelease(app);
            }
            return OpenWindowObservation::Unknown;
        }
        let mut windows = std::ptr::null();
        let status = unsafe { AXUIElementCopyAttributeValue(app, windows_key, &mut windows) };
        unsafe {
            CFRelease(windows_key);
            CFRelease(app);
        }
        if status != 0 || windows.is_null() {
            return OpenWindowObservation::Unknown;
        }
        let result = unsafe { classify_windows(windows, pid) };
        unsafe { CFRelease(windows) };
        result
    }
}

#[cfg(target_os = "macos")]
unsafe fn classify_windows(windows: *const c_void, pid: i32) -> OpenWindowObservation {
    if unsafe { CFGetTypeID(windows) != CFArrayGetTypeID() } {
        return OpenWindowObservation::Unknown;
    }
    match unsafe { CFArrayGetCount(windows) } {
        0 => OpenWindowObservation::Missing,
        1 => {
            let window = unsafe { CFArrayGetValueAtIndex(windows, 0) };
            let mut window_pid = 0;
            if window.is_null()
                || unsafe { AXUIElementGetPid(window, &mut window_pid) } != 0
                || window_pid != pid
            {
                return OpenWindowObservation::Unknown;
            }
            match unsafe { bool_attribute(window, "AXMinimized") } {
                Some(true) => OpenWindowObservation::Minimized,
                Some(false) => OpenWindowObservation::Present,
                None => OpenWindowObservation::Unknown,
            }
        }
        _ => OpenWindowObservation::Unknown,
    }
}

#[cfg(target_os = "macos")]
unsafe fn bool_attribute(element: *const c_void, name: &str) -> Option<bool> {
    let key = attribute(name)?;
    let mut value = std::ptr::null();
    let status = unsafe { AXUIElementCopyAttributeValue(element, key, &mut value) };
    unsafe { CFRelease(key) };
    if status != 0 || value.is_null() {
        return None;
    }
    let result = (unsafe { CFGetTypeID(value) } == unsafe { CFBooleanGetTypeID() })
        .then(|| unsafe { CFBooleanGetValue(value) });
    unsafe { CFRelease(value) };
    result
}
