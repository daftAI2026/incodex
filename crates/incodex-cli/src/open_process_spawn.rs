//! [INPUT]: 依赖 open_process 的 ChildProcess 契约与 macOS posix_spawn ABI。
//! [OUTPUT]: 提供责任隔离的 macOS child 和 posix_spawn 实现。
//! [POS]: open_process 的 Darwin 平台适配；缺私有符号时明确失败。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use std::ffi::{CString, OsStr, OsString};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::ExitStatus;

use super::ChildProcess;

#[cfg(target_os = "macos")]
pub(super) fn spawn_macos(
    program: &Path,
    args: &[String],
    overrides: &[(String, String)],
) -> io::Result<MacChild> {
    let c_program = c_string(program.as_os_str(), "executable path")?;
    let mut c_args = Vec::with_capacity(args.len() + 1);
    c_args.push(c_program.clone());
    for arg in args {
        c_args.push(c_string(OsStr::new(arg), "argument")?);
    }
    let mut argv: Vec<_> = c_args
        .iter()
        .map(|arg| arg.as_ptr() as *mut libc::c_char)
        .collect();
    argv.push(std::ptr::null_mut());

    let mut environment: std::collections::BTreeMap<OsString, OsString> =
        std::env::vars_os().collect();
    for (key, value) in overrides {
        if key.is_empty() || key.as_bytes().contains(&b'=') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "environment key is empty or contains '='",
            ));
        }
        environment.insert(OsString::from(key), OsString::from(value));
    }
    let mut c_environment = Vec::with_capacity(environment.len());
    for (key, value) in environment {
        let mut entry = key.as_bytes().to_vec();
        entry.push(b'=');
        entry.extend_from_slice(value.as_bytes());
        c_environment.push(CString::new(entry).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "environment key or value contains NUL",
            )
        })?);
    }
    let mut envp: Vec<_> = c_environment
        .iter()
        .map(|entry| entry.as_ptr() as *mut libc::c_char)
        .collect();
    envp.push(std::ptr::null_mut());

    let mut actions = SpawnFileActions::new()?;
    let null_device = CString::new("/dev/null").expect("static path has no NUL");
    for fd in [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO] {
        let result = unsafe {
            libc::posix_spawn_file_actions_addopen(
                actions.as_mut_ptr(),
                fd,
                null_device.as_ptr(),
                libc::O_RDWR,
                0,
            )
        };
        check_spawn_result("redirect stdio to /dev/null", result)?;
    }

    let mut attributes = SpawnAttributes::new()?;
    let mut default_signals = unsafe { std::mem::zeroed::<libc::sigset_t>() };
    check_errno_result("initialize child signal defaults", unsafe {
        libc::sigemptyset(&mut default_signals)
    })?;
    check_errno_result("restore child SIGPIPE default", unsafe {
        libc::sigaddset(&mut default_signals, libc::SIGPIPE)
    })?;
    check_spawn_result("set child default signals", unsafe {
        libc::posix_spawnattr_setsigdefault(attributes.as_mut_ptr(), &default_signals)
    })?;
    let flags = (libc::POSIX_SPAWN_CLOEXEC_DEFAULT | libc::POSIX_SPAWN_SETSIGDEF) as libc::c_short;
    check_spawn_result("set child spawn flags", unsafe {
        libc::posix_spawnattr_setflags(attributes.as_mut_ptr(), flags)
    })?;
    set_disclaim(&mut attributes)?;

    let mut pid = 0;
    let result = unsafe {
        libc::posix_spawn(
            &mut pid,
            c_program.as_ptr(),
            actions.as_ptr(),
            attributes.as_ptr(),
            argv.as_ptr(),
            envp.as_ptr(),
        )
    };
    check_spawn_result("launch official executable", result)?;
    Ok(MacChild { pid, status: None })
}

#[cfg(target_os = "macos")]
fn c_string(value: &OsStr, label: &str) -> io::Result<CString> {
    CString::new(value.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, format!("{label} contains NUL")))
}

#[cfg(target_os = "macos")]
struct SpawnAttributes(libc::posix_spawnattr_t);

#[cfg(target_os = "macos")]
impl SpawnAttributes {
    fn new() -> io::Result<Self> {
        let mut attributes = std::mem::MaybeUninit::uninit();
        let result = unsafe { libc::posix_spawnattr_init(attributes.as_mut_ptr()) };
        check_spawn_result("initialize spawn attributes", result)?;
        Ok(Self(unsafe { attributes.assume_init() }))
    }

    fn as_mut_ptr(&mut self) -> *mut libc::posix_spawnattr_t {
        &mut self.0
    }

    fn as_ptr(&self) -> *const libc::posix_spawnattr_t {
        &self.0
    }
}

#[cfg(target_os = "macos")]
impl Drop for SpawnAttributes {
    fn drop(&mut self) {
        unsafe {
            libc::posix_spawnattr_destroy(&mut self.0);
        }
    }
}

#[cfg(target_os = "macos")]
struct SpawnFileActions(libc::posix_spawn_file_actions_t);

#[cfg(target_os = "macos")]
impl SpawnFileActions {
    fn new() -> io::Result<Self> {
        let mut actions = std::mem::MaybeUninit::uninit();
        let result = unsafe { libc::posix_spawn_file_actions_init(actions.as_mut_ptr()) };
        check_spawn_result("initialize spawn file actions", result)?;
        Ok(Self(unsafe { actions.assume_init() }))
    }

    fn as_mut_ptr(&mut self) -> *mut libc::posix_spawn_file_actions_t {
        &mut self.0
    }

    fn as_ptr(&self) -> *const libc::posix_spawn_file_actions_t {
        &self.0
    }
}

#[cfg(target_os = "macos")]
impl Drop for SpawnFileActions {
    fn drop(&mut self) {
        unsafe {
            libc::posix_spawn_file_actions_destroy(&mut self.0);
        }
    }
}

#[cfg(target_os = "macos")]
type ResponsibilityDisclaim =
    unsafe extern "C" fn(*mut libc::posix_spawnattr_t, bool) -> libc::c_int;

// LLVM 使用此 Darwin 私有 attrs* ABI。它只隔离子进程责任归属，不改父进程或 TCC。
// 普通 posix_spawn 不保证此行为；缺少接口时必须失败。
#[cfg(target_os = "macos")]
fn set_disclaim(attributes: &mut SpawnAttributes) -> io::Result<()> {
    let symbol = unsafe {
        libc::dlsym(
            libc::RTLD_DEFAULT,
            b"responsibility_spawnattrs_setdisclaim\0".as_ptr().cast(),
        )
    };
    let setter = if symbol.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut libc::c_void, ResponsibilityDisclaim>(symbol) })
    };
    call_disclaim_setter(setter, attributes.as_mut_ptr())
}

#[cfg(target_os = "macos")]
fn call_disclaim_setter(
    setter: Option<ResponsibilityDisclaim>,
    attributes: *mut libc::posix_spawnattr_t,
) -> io::Result<()> {
    let setter = setter.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "responsibility_spawnattrs_setdisclaim is unavailable",
        )
    })?;
    let result = unsafe { setter(attributes, true) };
    check_spawn_result("disclaim inherited process responsibility", result)
}

#[cfg(target_os = "macos")]
fn check_spawn_result(operation: &str, result: libc::c_int) -> io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::new(
            io::Error::from_raw_os_error(result).kind(),
            format!("{operation}: {}", io::Error::from_raw_os_error(result)),
        ))
    }
}

#[cfg(target_os = "macos")]
fn check_errno_result(operation: &str, result: libc::c_int) -> io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("{operation}: {}", io::Error::last_os_error()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn fail_disclaim(_: *mut libc::posix_spawnattr_t, _: bool) -> libc::c_int {
        libc::EPERM
    }

    #[test]
    fn missing_disclaim_symbol_fails_closed() {
        let error = call_disclaim_setter(None, std::ptr::null_mut()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert!(error
            .to_string()
            .contains("responsibility_spawnattrs_setdisclaim"));
    }

    #[test]
    fn nonzero_disclaim_result_fails_closed() {
        let error = call_disclaim_setter(Some(fail_disclaim), std::ptr::null_mut()).unwrap_err();
        assert_eq!(
            error.kind(),
            io::Error::from_raw_os_error(libc::EPERM).kind()
        );
        assert!(error
            .to_string()
            .contains("disclaim inherited process responsibility"));
        assert!(error.to_string().contains(&libc::EPERM.to_string()));
    }
}

#[cfg(target_os = "macos")]
pub(in crate::open) struct MacChild {
    pid: libc::pid_t,
    status: Option<ExitStatus>,
}

#[cfg(target_os = "macos")]
impl ChildProcess for MacChild {
    fn id(&self) -> u32 {
        self.pid as u32
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if let Some(status) = self.status {
            return Ok(Some(status));
        }
        let mut raw_status = 0;
        loop {
            let result = unsafe { libc::waitpid(self.pid, &mut raw_status, libc::WNOHANG) };
            if result == self.pid {
                let status = ExitStatus::from_raw(raw_status);
                self.status = Some(status);
                return Ok(Some(status));
            }
            if result == 0 {
                return Ok(None);
            }
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
    }

    fn wait(&mut self) -> io::Result<ExitStatus> {
        if let Some(status) = self.status {
            return Ok(status);
        }
        let mut raw_status = 0;
        loop {
            let result = unsafe { libc::waitpid(self.pid, &mut raw_status, 0) };
            if result == self.pid {
                let status = ExitStatus::from_raw(raw_status);
                self.status = Some(status);
                return Ok(status);
            }
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
    }

    fn kill(&mut self) -> io::Result<()> {
        self.signal(libc::SIGKILL)
    }

    fn signal(&mut self, signal: libc::c_int) -> io::Result<()> {
        if self.status.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot signal an already reaped child",
            ));
        }
        if unsafe { libc::kill(self.pid, signal) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}
