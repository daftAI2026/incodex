/*
 * [INPUT]: 依赖包身份、原生线程挂起计数、内容寻址 helper 与同一预挂钩操作参数。
 * [OUTPUT]: 提供单次启动及并发启动集合的稳定句柄证明，按退出证据退休缓存，不恢复或终结进程。
 * [POS]: 自动换代事务的进程边界；只接纳同一 helper/operation 持有的普通挂起首线程。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::ptr;
use windows_sys::Wdk::System::Threading::{NtQueryInformationThread, ThreadSuspendCount};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, Thread32First, Thread32Next,
    PROCESSENTRY32W, TH32CS_SNAPPROCESS, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, OpenThread, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, THREAD_QUERY_INFORMATION,
};
use windows_sys::Win32::UI::Shell::CommandLineToArgvW;

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub(crate) struct SuspendedLaunch {
    process: Handle,
    thread: Handle,
    package: String,
    pid: u32,
    tid: u32,
}

impl SuspendedLaunch {
    pub(crate) fn capture(package: &str, pid: u32, tid: u32) -> Result<Self, String> {
        let process = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if process.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let process = Handle(process);
        let thread = unsafe { OpenThread(THREAD_QUERY_INFORMATION, 0, tid) };
        if thread.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let held = Self {
            process,
            thread: Handle(thread),
            package: package.into(),
            pid,
            tid,
        };
        held.verify()?;
        Ok(held)
    }

    pub(crate) fn verify(&self) -> Result<(), String> {
        crate::windows_process::validate_debugged_package_process(
            &self.package,
            self.pid,
            self.tid,
        )
        .map_err(|error| error.to_string())?;
        let command = crate::windows_process::process_command_line(self.pid)
            .map_err(|error| error.to_string())?;
        if !crate::windows_update_repair::is_primary_package_process(&command)
            || crate::windows_activation::windows_debugger_route(&command)?
                != crate::windows_activation::WindowsDebuggerRoute::ResumeNormally
        {
            return Err("Windows prearm target is not the ordinary primary launch".into());
        }
        require_single_suspend(self.thread.0)
    }
}

fn require_single_suspend(thread: HANDLE) -> Result<(), String> {
    let mut count = 0u32;
    let result = unsafe {
        NtQueryInformationThread(
            thread,
            ThreadSuspendCount,
            (&mut count as *mut u32).cast(),
            size_of::<u32>() as u32,
            ptr::null_mut(),
        )
    };
    if result < 0 || count != 1 {
        return Err(format!(
            "Windows prearm target is not singly suspended (NTSTATUS 0x{:08X}, count {count})",
            result as u32
        ));
    }
    Ok(())
}

pub(crate) struct SuspendedLaunchGroup {
    primary: SuspendedLaunch,
    helper: PathBuf,
    operation: Option<String>,
    peers: RefCell<BTreeMap<u32, PeerLaunch>>,
}

struct PeerLaunch {
    launch: SuspendedLaunch,
    _helper: Handle,
    helper_pid: u32,
}

impl SuspendedLaunchGroup {
    fn retire_exited_peers(peers: &mut BTreeMap<u32, PeerLaunch>) -> Result<(), String> {
        let mut exited = Vec::new();
        for (pid, peer) in peers.iter() {
            match unsafe { WaitForSingleObject(peer.launch.process.0, 0) } {
                WAIT_OBJECT_0 => exited.push(*pid),
                WAIT_TIMEOUT => {}
                _ => return Err("Windows prearm peer exit evidence is unknown".into()),
            }
        }
        for pid in exited {
            peers.remove(&pid);
        }
        Ok(())
    }

    pub(crate) fn capture(
        package: &str,
        pid: u32,
        tid: u32,
        helper: &Path,
        operation: Option<&str>,
    ) -> Result<Self, String> {
        if operation.is_some_and(|id| id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("Windows prearm operation is invalid".into());
        }
        Ok(Self {
            primary: SuspendedLaunch::capture(package, pid, tid)?,
            helper: helper.into(),
            operation: operation.map(str::to_owned),
            peers: RefCell::new(BTreeMap::new()),
        })
    }

    pub(crate) fn verify(&self) -> Result<Vec<u32>, String> {
        self.primary.verify()?;
        let running =
            crate::windows_process::strict_running_codex_package_process_ids(&self.primary.package)
                .map_err(|e| e.to_string())?;
        if !running.contains(&self.primary.pid) {
            return Err("Windows prearm primary identity disappeared".into());
        }
        if single_thread_id(self.primary.pid)? != self.primary.tid {
            return Err("Windows prearm primary created another thread".into());
        }
        let mut peers = self.peers.borrow_mut();
        Self::retire_exited_peers(&mut peers)?;
        for pid in running
            .iter()
            .copied()
            .filter(|pid| *pid != self.primary.pid)
        {
            let operation = self
                .operation
                .as_deref()
                .ok_or("legacy Windows prearm cannot authorize another launch")?;
            if let std::collections::btree_map::Entry::Vacant(entry) = peers.entry(pid) {
                let tid = single_thread_id(pid)?;
                let launch = SuspendedLaunch::capture(&self.primary.package, pid, tid)?;
                let (helper_pid, helper_handle) = self.peer_helper(pid, tid, operation)?;
                entry.insert(PeerLaunch {
                    launch,
                    _helper: helper_handle,
                    helper_pid,
                });
            }
        }
        for peer in peers.values() {
            peer.launch.verify()?;
            if single_thread_id(peer.launch.pid)? != peer.launch.tid {
                return Err("Windows prearm peer created another thread".into());
            }
            let operation = self
                .operation
                .as_deref()
                .ok_or("Windows prearm operation is missing")?;
            if !self.helper_matches(peer.helper_pid, peer.launch.pid, peer.launch.tid, operation)? {
                return Err("Windows prearm peer authorization changed".into());
            }
        }
        Ok(running)
    }

    fn helper_matches(
        &self,
        helper_pid: u32,
        pid: u32,
        tid: u32,
        operation: &str,
    ) -> Result<bool, String> {
        let path =
            crate::windows_process::process_image_path(helper_pid).map_err(|e| e.to_string())?;
        if std::fs::canonicalize(path).map_err(|e| e.to_string())? != self.helper {
            return Ok(false);
        }
        let command =
            crate::windows_process::process_command_line(helper_pid).map_err(|e| e.to_string())?;
        Ok(peer_command_matches(
            &command,
            &self.primary.package,
            operation,
            pid,
            tid,
        ))
    }

    fn peer_helper(&self, pid: u32, tid: u32, operation: &str) -> Result<(u32, Handle), String> {
        let mut found = None;
        let name = self
            .helper
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Windows prearm helper name is invalid")?;
        for candidate in named_process_ids(name)? {
            if candidate == std::process::id() {
                continue;
            }
            let raw = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    candidate,
                )
            };
            if raw.is_null() {
                if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER {
                    continue;
                }
                return Err("cannot inspect a possible Windows prearm helper".into());
            }
            let handle = Handle(raw);
            match self.helper_matches(candidate, pid, tid, operation) {
                Ok(false) => {}
                Ok(true) => {
                    if found.replace((candidate, handle)).is_some() {
                        return Err("Windows prearm peer helper is ambiguous".into());
                    }
                }
                Err(_) if unsafe { WaitForSingleObject(handle.0, 0) } == WAIT_OBJECT_0 => {}
                Err(error) => {
                    return Err(format!("Windows prearm helper proof is unknown: {error}"))
                }
            }
        }
        found.ok_or_else(|| "Windows prearm peer has no matching authorized helper".into())
    }
}

fn named_process_ids(name: &str) -> Result<Vec<u32>, String> {
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let snapshot = Handle(raw);
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    if unsafe { Process32FirstW(snapshot.0, &mut entry) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut matches = Vec::new();
    loop {
        let length = entry
            .szExeFile
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(entry.szExeFile.len());
        if String::from_utf16_lossy(&entry.szExeFile[..length]).eq_ignore_ascii_case(name) {
            matches.push(entry.th32ProcessID);
        }
        if unsafe { Process32NextW(snapshot.0, &mut entry) } == 0 {
            if unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
                return Err(std::io::Error::last_os_error().to_string());
            }
            break;
        }
    }
    Ok(matches)
}

pub(crate) fn registration_operation(arguments: &[String]) -> Result<Option<&str>, String> {
    let mut matches = arguments
        .iter()
        .enumerate()
        .filter(|(_, value)| value.as_str() == "-r");
    let Some((index, _)) = matches.next() else {
        return Ok(None);
    };
    let value = arguments.get(index + 1).filter(|id| {
        id.len() == 32
            && id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    });
    if matches.next().is_some() || value.is_none() {
        return Err("Windows debugger operation argument is invalid".into());
    }
    Ok(value.map(String::as_str))
}

fn single_thread_id(pid: u32) -> Result<u32, String> {
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let snapshot = Handle(raw);
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    if unsafe { Thread32First(snapshot.0, &mut entry) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut thread = None;
    loop {
        if entry.th32OwnerProcessID == pid && thread.replace(entry.th32ThreadID).is_some() {
            return Err("Windows prearm process has multiple threads".into());
        }
        if unsafe { Thread32Next(snapshot.0, &mut entry) } == 0 {
            if unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
                return Err(std::io::Error::last_os_error().to_string());
            }
            break;
        }
    }
    thread.ok_or_else(|| "Windows prearm process has no thread".into())
}

fn peer_command_matches(command: &str, package: &str, operation: &str, pid: u32, tid: u32) -> bool {
    if command.contains('\0') {
        return false;
    }
    let wide = command.encode_utf16().chain([0]).collect::<Vec<_>>();
    let mut count = 0;
    let argv = unsafe { CommandLineToArgvW(wide.as_ptr(), &mut count) };
    if argv.is_null() {
        return false;
    }
    let mut args = Vec::new();
    for index in 0..count {
        let argument = unsafe { *argv.add(index as usize) };
        let mut length = 0;
        while unsafe { *argument.add(length) } != 0 {
            length += 1;
        }
        args.push(String::from_utf16_lossy(unsafe {
            std::slice::from_raw_parts(argument, length)
        }));
    }
    unsafe { LocalFree(argv.cast()) };
    args.get(1)
        .is_some_and(|s| s == "__incodex_windows_installed_debugger")
        && args.len() == 10
        && args[2] == "--package"
        && args[3] == package
        && args[4] == "-r"
        && args[5] == operation
        && args[6] == "-p"
        && args[7] == pid.to_string()
        && args[8] == "-tid"
        && args[9] == tid.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::System::Threading::{
        ResumeThread, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME,
    };

    #[test]
    fn peer_proof_requires_the_same_operation_package_pid_and_thread() {
        let package = "OpenAI.Codex_1.2.3.4_x64__2p2nqsd0c76g0";
        let operation = "0123456789abcdef0123456789abcdef";
        let command = format!(
            r#""C:\Users\Test User\.incodex\windows\i\0123456789abcdef\i.exe" __incodex_windows_installed_debugger --package {package} -r {operation} -p 42 -tid 43"#
        );
        assert!(peer_command_matches(&command, package, operation, 42, 43));
        assert!(!peer_command_matches(
            &command,
            package,
            "ffffffffffffffffffffffffffffffff",
            42,
            43
        ));
        assert!(!peer_command_matches(
            &command,
            "OpenAI.Codex_1.2.3.5_x64__2p2nqsd0c76g0",
            operation,
            42,
            43
        ));
        assert!(!peer_command_matches(&command, package, operation, 44, 43));
        assert!(!peer_command_matches(&command, package, operation, 42, 44));
        assert!(!peer_command_matches(
            &(command.clone() + " -r " + operation),
            package,
            operation,
            42,
            43
        ));
        assert!(!peer_command_matches(
            &command.replace(&format!(" -r {operation}"), ""),
            package,
            operation,
            42,
            43
        ));
    }

    #[test]
    fn legacy_operation_is_only_the_absence_of_an_argument() {
        let args = |values: &[&str]| values.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            registration_operation(&args(&["__incodex_windows_installed_debugger"])),
            Ok(None)
        );
        assert_eq!(
            registration_operation(&args(&["-r", "0123456789abcdef0123456789abcdef"])),
            Ok(Some("0123456789abcdef0123456789abcdef"))
        );
        for values in [
            vec!["-r"],
            vec!["-r", "bad"],
            vec!["-r", "ABCDEF0123456789ABCDEF0123456789"],
            vec![
                "-r",
                "0123456789abcdef0123456789abcdef",
                "-r",
                "0123456789abcdef0123456789abcdef",
            ],
        ] {
            assert!(registration_operation(&args(&values)).is_err());
        }
        let own_name = std::env::current_exe()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        assert!(named_process_ids(&own_name)
            .unwrap()
            .contains(&std::process::id()));
    }

    struct OwnedFixture(std::process::Child);
    impl Drop for OwnedFixture {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn kernel_proof_accepts_one_suspended_thread_and_rejects_it_after_resume() {
        let child = OwnedFixture(
            Command::new(std::env::current_exe().unwrap())
                .arg("--list")
                .creation_flags(CREATE_SUSPENDED)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let tid = single_thread_id(child.0.id()).unwrap();
        let raw = unsafe { OpenThread(THREAD_QUERY_INFORMATION | THREAD_SUSPEND_RESUME, 0, tid) };
        assert!(!raw.is_null());
        let thread = Handle(raw);
        require_single_suspend(thread.0).unwrap();
        assert_eq!(unsafe { ResumeThread(thread.0) }, 1);
        assert!(require_single_suspend(thread.0).is_err());
    }

    #[test]
    fn cached_peer_retires_only_after_its_stable_process_handle_signals_exit() {
        let mut child = OwnedFixture(
            Command::new(std::env::current_exe().unwrap())
                .arg("--list")
                .creation_flags(CREATE_SUSPENDED)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let pid = child.0.id();
        let tid = single_thread_id(pid).unwrap();
        let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        let thread = unsafe { OpenThread(THREAD_QUERY_INFORMATION, 0, tid) };
        let helper = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, std::process::id()) };
        assert!(!process.is_null() && !thread.is_null() && !helper.is_null());
        // 这里只测稳定句柄的缓存寿命；不把未带包身份的夹具冒充启动集合证明。
        let mut peers = BTreeMap::from([(
            pid,
            PeerLaunch {
                launch: SuspendedLaunch {
                    process: Handle(process),
                    thread: Handle(thread),
                    package: String::new(),
                    pid,
                    tid,
                },
                _helper: Handle(helper),
                helper_pid: std::process::id(),
            },
        )]);
        SuspendedLaunchGroup::retire_exited_peers(&mut peers).unwrap();
        assert!(
            peers.contains_key(&pid),
            "a live suspended peer must remain"
        );
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        SuspendedLaunchGroup::retire_exited_peers(&mut peers).unwrap();
        assert!(peers.is_empty(), "a positively exited peer must retire");
    }
}
