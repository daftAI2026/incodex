//! [INPUT]: 依赖 Darwin dlopen、dlsym 与 dlclose。
//! [OUTPUT]: 提供持有动态库句柄的符号与全局数据解析。
//! [POS]: macOS 私有框架与 CoreGraphics 适配器共用的加载边界。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use std::ffi::{c_void, CStr, CString};

pub(crate) struct FrameworkHandle(*mut c_void);

impl FrameworkHandle {
    pub(crate) fn open(path: &str) -> Result<Self, String> {
        let path = CString::new(path).map_err(|error| error.to_string())?;
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
        if handle.is_null() {
            return Err(dynamic_loader_error("cannot load macOS framework"));
        }
        Ok(Self(handle))
    }

    pub(crate) fn symbol(&self, name: &str) -> Result<*mut c_void, String> {
        let name = CString::new(name).map_err(|error| error.to_string())?;
        unsafe {
            libc::dlerror();
        }
        let symbol = unsafe { libc::dlsym(self.0, name.as_ptr()) };
        if symbol.is_null() {
            return Err(dynamic_loader_error(&format!(
                "macOS framework is missing symbol {}",
                name.to_string_lossy()
            )));
        }
        Ok(symbol)
    }

    pub(crate) fn data_pointer(&self, name: &str) -> Result<*const c_void, String> {
        let symbol = self.symbol(name)?;
        let value = unsafe { *(symbol as *const *const c_void) };
        if value.is_null() {
            return Err(format!("macOS framework returned a null {name}"));
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
