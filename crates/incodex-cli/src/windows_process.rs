use std::collections::HashSet;
use std::ffi::c_void;
use std::io;
use std::mem::size_of;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Wdk::System::Threading::{
    NtQueryInformationProcess, ProcessCommandLineInformation,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, DuplicateHandle, APPMODEL_ERROR_NO_PACKAGE, DUPLICATE_SAME_ACCESS,
    ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA,
    ERROR_NO_MORE_FILES, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, RECT, UNICODE_STRING,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID, MIB_TCP_STATE_ESTAB,
    TCP_TABLE_CLASS, TCP_TABLE_OWNER_PID_ALL, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows_sys::Win32::Networking::WinSock::AF_INET;
use windows_sys::Win32::Storage::Packaging::Appx::GetPackageFullName;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, Thread32First, Thread32Next,
    PROCESSENTRY32W, TH32CS_SNAPPROCESS, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
    JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, OpenJobObjectW, QueryInformationJobObject,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, OpenProcess, OpenThread, ResumeThread, TerminateProcess,
    WaitForSingleObject, CREATE_SUSPENDED, INFINITE, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SET_QUOTA, PROCESS_TERMINATE, THREAD_SUSPEND_RESUME,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible,
    GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOPMOST,
};

use crate::windows_activation_capability::WindowsActivationCapability;

#[derive(Debug)]
pub struct WindowsProcessTree {
    process: ProcessHandle,
    process_id: u32,
    _job: OwnedHandle,
}

#[derive(Debug)]
pub(crate) struct WindowsPendingJob {
    job: OwnedHandle,
    name: String,
}

#[derive(Debug)]
pub struct WindowsCdpOwnershipGuard {
    job: ThreadSafeOwnedHandle,
    debug_port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsCdpListenerStatus {
    Owned,
    Missing,
    Foreign,
}

#[derive(Debug)]
enum ProcessHandle {
    Child(Child),
    Activated(OwnedHandle),
}

const JOB_TERMINATION_TIMEOUT: Duration = Duration::from_secs(5);
const JOB_TERMINATION_POLL: Duration = Duration::from_millis(10);
const PROCESS_COMMAND_LINE_LIMIT: u32 = 64 * 1024;

pub fn process_command_line(process_id: u32) -> io::Result<String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    let process = OwnedHandle::from_nullable(process)?;
    let mut required = 0;
    let _ = unsafe {
        NtQueryInformationProcess(
            process.raw(),
            ProcessCommandLineInformation,
            std::ptr::null_mut(),
            0,
            &mut required,
        )
    };
    if required < size_of::<UNICODE_STRING>() as u32 || required > PROCESS_COMMAND_LINE_LIMIT {
        return Err(io::Error::other(
            "Windows process command line length is invalid",
        ));
    }
    let mut buffer = vec![0u8; required as usize];
    let status = unsafe {
        NtQueryInformationProcess(
            process.raw(),
            ProcessCommandLineInformation,
            buffer.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    };
    if status < 0 {
        return Err(io::Error::other(format!(
            "NtQueryInformationProcess failed with NTSTATUS 0x{:08X}",
            status as u32
        )));
    }
    let command = unsafe { &*buffer.as_ptr().cast::<UNICODE_STRING>() };
    if command.Length % 2 != 0 || command.Length > command.MaximumLength || command.Buffer.is_null()
    {
        return Err(io::Error::other(
            "Windows process command line metadata is invalid",
        ));
    }
    let value = unsafe {
        std::slice::from_raw_parts(
            command.Buffer,
            usize::from(command.Length) / size_of::<u16>(),
        )
    };
    String::from_utf16(value)
        .map_err(|_| io::Error::other("Windows process command line is not valid UTF-16"))
}

pub(crate) struct VisibleWindowLifecycle {
    grace: Duration,
    seen_visible: bool,
    missing_since: Option<Instant>,
}

impl VisibleWindowLifecycle {
    pub(crate) fn new(grace: Duration) -> Self {
        Self {
            grace,
            seen_visible: false,
            missing_since: None,
        }
    }

    pub(crate) fn should_close(&mut self, visible: bool, now: Instant) -> bool {
        if visible {
            self.seen_visible = true;
            self.missing_since = None;
            return false;
        }
        if !self.seen_visible {
            return false;
        }
        let missing_since = self.missing_since.get_or_insert(now);
        now.saturating_duration_since(*missing_since) >= self.grace
    }
}

impl WindowsProcessTree {
    pub fn id(&self) -> u32 {
        self.process_id
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let root_status = match &mut self.process {
            ProcessHandle::Child(child) => child.try_wait(),
            ProcessHandle::Activated(process) => try_wait_handle(process.raw()),
        }?;
        if root_status.is_some() && self.job_has_active_processes()? {
            Ok(None)
        } else {
            Ok(root_status)
        }
    }

    pub fn terminate(&mut self) -> io::Result<ExitStatus> {
        self.terminate_with_code(1)
    }

    pub fn terminate_successfully(&mut self) -> io::Result<ExitStatus> {
        self.terminate_with_code(0)
    }

    fn terminate_with_code(&mut self, exit_code: u32) -> io::Result<ExitStatus> {
        if let Some(status) = self.try_wait()? {
            return Ok(status);
        }
        if unsafe { TerminateJobObject(self._job.raw(), exit_code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let status = match &mut self.process {
            ProcessHandle::Child(child) => child.wait(),
            ProcessHandle::Activated(process) => wait_handle(process.raw()),
        }?;
        self.wait_for_empty_job()?;
        Ok(status)
    }

    pub fn cdp_ownership_guard(&self, port: u16) -> io::Result<Option<WindowsCdpOwnershipGuard>> {
        let Some(owner_pid) = ipv4_listener_owner(port)? else {
            return Ok(None);
        };
        if !process_is_in_job(owner_pid, self._job.raw())? {
            return Ok(None);
        }
        Ok(Some(WindowsCdpOwnershipGuard {
            job: duplicate_thread_safe_handle(self._job.raw())?,
            debug_port: port,
        }))
    }

    pub fn listener_owner_is_in_job(&self, port: u16) -> io::Result<bool> {
        let Some(owner_pid) = ipv4_listener_owner(port)? else {
            return Ok(false);
        };
        process_is_in_job(owner_pid, self._job.raw())
    }

    pub fn has_visible_window(&self) -> io::Result<bool> {
        let process_ids = job_process_ids(self._job.raw())?;
        has_visible_window_for_processes(&process_ids)
    }

    pub fn contains_process(&self, process_id: u32) -> io::Result<bool> {
        process_is_in_job(process_id, self._job.raw())
    }

    fn job_has_active_processes(&self) -> io::Result<bool> {
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        if unsafe {
            QueryInformationJobObject(
                self._job.raw(),
                JobObjectBasicAccountingInformation,
                (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(accounting.ActiveProcesses != 0)
    }

    fn wait_for_empty_job(&self) -> io::Result<()> {
        let deadline = Instant::now() + JOB_TERMINATION_TIMEOUT;
        while self.job_has_active_processes()? {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Windows Job Object still has active processes after termination",
                ));
            }
            thread::sleep(JOB_TERMINATION_POLL);
        }
        Ok(())
    }
}

fn job_process_ids(job: HANDLE) -> io::Result<HashSet<u32>> {
    const HEADER_BYTES: usize = size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() - size_of::<usize>();
    let mut capacity = 16usize;
    loop {
        let byte_len = HEADER_BYTES + capacity * size_of::<usize>();
        let word_len = byte_len.div_ceil(size_of::<usize>());
        let mut storage = vec![0usize; word_len];
        let queried = unsafe {
            QueryInformationJobObject(
                job,
                JobObjectBasicProcessIdList,
                storage.as_mut_ptr().cast(),
                byte_len as u32,
                std::ptr::null_mut(),
            )
        };
        if queried != 0 {
            let list = unsafe { &*storage.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
            let count = list.NumberOfProcessIdsInList as usize;
            let first = unsafe {
                storage
                    .as_ptr()
                    .cast::<u8>()
                    .add(HEADER_BYTES)
                    .cast::<usize>()
            };
            let ids = unsafe { std::slice::from_raw_parts(first, count) };
            return Ok(ids.iter().map(|id| *id as u32).collect());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_MORE_DATA as i32) {
            capacity = capacity
                .checked_mul(2)
                .ok_or_else(|| io::Error::other("Windows Job process list capacity overflowed"))?;
            continue;
        }
        return Err(error);
    }
}

struct VisibleWindowSearch<'a> {
    process_ids: &'a HashSet<u32>,
    found: bool,
}

fn has_visible_window_for_processes(process_ids: &HashSet<u32>) -> io::Result<bool> {
    let mut search = VisibleWindowSearch {
        process_ids,
        found: false,
    };
    let completed = unsafe {
        EnumWindows(
            Some(find_visible_process_window),
            (&mut search as *mut VisibleWindowSearch<'_>) as LPARAM,
        )
    };
    if completed == 0 && !search.found {
        return Err(io::Error::last_os_error());
    }
    Ok(search.found)
}

unsafe extern "system" fn find_visible_process_window(window: HWND, context: LPARAM) -> i32 {
    let mut process_id = 0u32;
    unsafe { GetWindowThreadProcessId(window, &mut process_id) };
    let search = unsafe { &mut *(context as *mut VisibleWindowSearch<'_>) };
    if !search.process_ids.contains(&process_id) {
        return 1;
    }
    let mut bounds = RECT::default();
    if unsafe { GetWindowRect(window, &mut bounds) } == 0 {
        return 1;
    }
    let extended_style = unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } as u32;
    if is_primary_visible_window(
        unsafe { IsWindowVisible(window) } != 0,
        bounds.right.saturating_sub(bounds.left),
        bounds.bottom.saturating_sub(bounds.top),
        extended_style,
    ) {
        search.found = true;
        return 0;
    }
    1
}

fn is_primary_visible_window(visible: bool, width: i32, height: i32, extended_style: u32) -> bool {
    if !visible || width <= 0 || height <= 0 {
        return false;
    }
    let topmost = extended_style & WS_EX_TOPMOST != 0;
    let no_activate = extended_style & WS_EX_NOACTIVATE != 0;
    !(topmost && no_activate)
}

impl WindowsCdpOwnershipGuard {
    pub fn listener_status(&self) -> Result<WindowsCdpListenerStatus, String> {
        let Some(owner_pid) = ipv4_listener_owner(self.debug_port)
            .map_err(|error| format!("cannot inspect Windows CDP listener owner: {error}"))?
        else {
            return Ok(WindowsCdpListenerStatus::Missing);
        };
        match process_is_in_job(owner_pid, self.job.raw()) {
            Ok(true) => Ok(WindowsCdpListenerStatus::Owned),
            Ok(false) => Ok(WindowsCdpListenerStatus::Foreign),
            Err(error) => Err(format!(
                "cannot prove Windows CDP listener owner belongs to the isolated Job Object: {error}"
            )),
        }
    }

    pub fn require_connection_owner(&self, stream: &TcpStream) -> Result<(), String> {
        let owner_pid = ipv4_connection_server_owner(stream)
            .map_err(|error| format!("cannot inspect Windows CDP connection owner: {error}"))?
            .ok_or_else(|| {
                "cannot identify the established Windows CDP connection owner".to_string()
            })?;
        self.require_job_process(owner_pid, "connection")
    }

    fn require_job_process(&self, owner_pid: u32, surface: &str) -> Result<(), String> {
        match process_is_in_job(owner_pid, self.job.raw()) {
            Ok(true) => Ok(()),
            Ok(false) => Err(format!(
                "Windows CDP {surface} owner is outside the isolated Job Object"
            )),
            Err(error) => Err(format!(
                "cannot prove Windows CDP {surface} owner belongs to the isolated Job Object: {error}"
            )),
        }
    }
}

fn ipv4_listener_owner(port: u16) -> io::Result<Option<u32>> {
    ipv4_tcp_owner(TCP_TABLE_OWNER_PID_LISTENER, |row| {
        let local = row_local_addr(row);
        *local.ip() == Ipv4Addr::LOCALHOST && local.port() == port
    })
}

fn ipv4_connection_server_owner(stream: &TcpStream) -> io::Result<Option<u32>> {
    let client = require_ipv4_addr(stream.local_addr()?)?;
    let server = require_ipv4_addr(stream.peer_addr()?)?;
    ipv4_tcp_owner(TCP_TABLE_OWNER_PID_ALL, |row| {
        row.dwState == MIB_TCP_STATE_ESTAB as u32
            && row_local_addr(row) == server
            && row_remote_addr(row) == client
    })
}

fn require_ipv4_addr(address: SocketAddr) -> io::Result<SocketAddrV4> {
    match address {
        SocketAddr::V4(address) => Ok(address),
        SocketAddr::V6(_) => Err(io::Error::other(
            "Windows CDP ownership proof requires an IPv4 connection",
        )),
    }
}

fn row_local_addr(row: &MIB_TCPROW_OWNER_PID) -> SocketAddrV4 {
    SocketAddrV4::new(
        Ipv4Addr::from(u32::from_be(row.dwLocalAddr)),
        u16::from_be(row.dwLocalPort as u16),
    )
}

fn row_remote_addr(row: &MIB_TCPROW_OWNER_PID) -> SocketAddrV4 {
    SocketAddrV4::new(
        Ipv4Addr::from(u32::from_be(row.dwRemoteAddr)),
        u16::from_be(row.dwRemotePort as u16),
    )
}

fn ipv4_tcp_owner<F>(table_class: TCP_TABLE_CLASS, matches: F) -> io::Result<Option<u32>>
where
    F: Fn(&MIB_TCPROW_OWNER_PID) -> bool,
{
    let mut bytes = 0;
    let first = unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut bytes,
            0,
            AF_INET as u32,
            table_class,
            0,
        )
    };
    if first != ERROR_INSUFFICIENT_BUFFER || bytes < size_of::<MIB_TCPTABLE_OWNER_PID>() as u32 {
        return Err(io::Error::from_raw_os_error(first as i32));
    }
    let mut storage = vec![0u32; (bytes as usize).div_ceil(size_of::<u32>())];
    let status = unsafe {
        GetExtendedTcpTable(
            storage.as_mut_ptr().cast(),
            &mut bytes,
            0,
            AF_INET as u32,
            table_class,
            0,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let table = unsafe { &*storage.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>() };
    let rows =
        unsafe { std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize) };
    Ok(rows
        .iter()
        .find_map(|row| matches(row).then_some(row.dwOwningPid)))
}

fn process_is_in_job(process_id: u32, job: HANDLE) -> io::Result<bool> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    let process = OwnedHandle::from_nullable(process)?;
    let mut contained = 0;
    if unsafe { IsProcessInJob(process.raw(), job, &mut contained) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(contained != 0)
}

fn duplicate_thread_safe_handle(handle: HANDLE) -> io::Result<ThreadSafeOwnedHandle> {
    let process = unsafe { GetCurrentProcess() };
    let mut duplicate = std::ptr::null_mut();
    if unsafe {
        DuplicateHandle(
            process,
            handle,
            process,
            &mut duplicate,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(ThreadSafeOwnedHandle(duplicate as usize))
}

pub fn spawn_kill_on_drop(command: &mut Command) -> io::Result<WindowsProcessTree> {
    let job = create_kill_on_close_job(None)?;
    command.creation_flags(CREATE_SUSPENDED);
    let mut child = command.spawn()?;

    let process = child.as_raw_handle().cast::<c_void>();
    if unsafe { AssignProcessToJobObject(job.raw(), process) } == 0 {
        let error = io::Error::last_os_error();
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }

    if let Err(error) = resume_process_primary_thread(child.id()) {
        drop(job);
        let _ = child.wait();
        return Err(error);
    }

    Ok(WindowsProcessTree {
        process_id: child.id(),
        process: ProcessHandle::Child(child),
        _job: job,
    })
}

impl WindowsPendingJob {
    pub(crate) fn create_for_capability(
        capability: &WindowsActivationCapability,
    ) -> io::Result<Self> {
        let name = capability.job_name().to_string();
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        Ok(Self {
            job: create_kill_on_close_job(Some(&wide))?,
            name,
        })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn attach(
        self,
        process_id: u32,
        expected_package_full_name: &str,
    ) -> io::Result<WindowsProcessTree> {
        const REQUIRED_ACCESS: u32 =
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | 0x0010_0000;
        let process = unsafe { OpenProcess(REQUIRED_ACCESS, 0, process_id) };
        let process = OwnedHandle::from_nullable(process)?;
        require_process_package_identity(process.raw(), expected_package_full_name)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let mut contained = 0;
            if unsafe { IsProcessInJob(process.raw(), self.job.raw(), &mut contained) } == 0 {
                let error = io::Error::last_os_error();
                return Err(terminate_exact_process_after_attach_failure(
                    process.raw(),
                    error,
                ));
            }
            if contained != 0 {
                return Ok(WindowsProcessTree {
                    process: ProcessHandle::Activated(process),
                    process_id,
                    _job: self.job,
                });
            }
            if try_wait_handle(process.raw())?.is_some() {
                return Err(io::Error::other(
                    "activated Windows Codex process exited before Job containment",
                ));
            }
            if std::time::Instant::now() >= deadline {
                return Err(terminate_exact_process_after_attach_failure(
                    process.raw(),
                    io::Error::other(
                        "timed out waiting for the Windows package debugger to assign the Job",
                    ),
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

pub(crate) fn assign_debugged_process_to_job(
    job_name: &str,
    expected_package_full_name: &str,
    process_id: u32,
    thread_id: u32,
) -> io::Result<()> {
    let job_name: Vec<u16> = job_name.encode_utf16().chain([0]).collect();
    const JOB_ASSIGN_AND_QUERY: u32 = 0x0001 | 0x0004;
    let job = unsafe { OpenJobObjectW(JOB_ASSIGN_AND_QUERY, 0, job_name.as_ptr()) };
    let job = OwnedHandle::from_nullable(job)?;
    const REQUIRED_ACCESS: u32 =
        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA | PROCESS_TERMINATE | 0x0010_0000;
    let process = unsafe { OpenProcess(REQUIRED_ACCESS, 0, process_id) };
    let process = OwnedHandle::from_nullable(process)?;
    require_process_package_identity(process.raw(), expected_package_full_name)?;
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    let snapshot = OwnedHandle::from_snapshot(snapshot)?;
    require_thread_owner(snapshot.raw(), thread_id, process_id)?;
    if unsafe { AssignProcessToJobObject(job.raw(), process.raw()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
    let thread = OwnedHandle::from_nullable(thread)?;
    if unsafe { ResumeThread(thread.raw()) } == u32::MAX {
        let error = io::Error::last_os_error();
        let _ = unsafe { TerminateProcess(process.raw(), 1) };
        return Err(error);
    }
    Ok(())
}

pub fn resume_debugged_package_process(
    expected_package_full_name: &str,
    process_id: u32,
    thread_id: u32,
) -> io::Result<()> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    let process = OwnedHandle::from_nullable(process)?;
    require_process_package_identity(process.raw(), expected_package_full_name)?;
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    let snapshot = OwnedHandle::from_snapshot(snapshot)?;
    require_thread_owner(snapshot.raw(), thread_id, process_id)?;
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
    let thread = OwnedHandle::from_nullable(thread)?;
    let previous = unsafe { ResumeThread(thread.raw()) };
    if previous == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    if previous == 0 {
        return Err(io::Error::other(
            "Windows package debugger target thread was not suspended",
        ));
    }
    Ok(())
}

pub fn running_package_process_ids(expected_package_full_name: &str) -> io::Result<Vec<u32>> {
    let mut matches = Vec::new();
    for process_id in snapshot_process_ids()? {
        let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if raw.is_null() {
            let error = io::Error::last_os_error();
            if matches!(
                error.raw_os_error(),
                Some(code) if code == ERROR_ACCESS_DENIED as i32 || code == ERROR_INVALID_PARAMETER as i32
            ) {
                continue;
            }
            return Err(error);
        }
        let process = OwnedHandle(raw);
        match process_package_full_name(process.raw()) {
            Ok(Some(package)) if package == expected_package_full_name => matches.push(process_id),
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(code) if code == ERROR_ACCESS_DENIED as i32 || code == ERROR_INVALID_PARAMETER as i32
                ) => {}
            Err(error) => return Err(error),
        }
    }
    matches.sort_unstable();
    Ok(matches)
}

pub fn authenticate_process_in_named_job(
    job_name: &str,
    expected_package_full_name: &str,
    process_id: u32,
) -> io::Result<()> {
    let job_name = job_name.encode_utf16().chain([0]).collect::<Vec<_>>();
    let job = unsafe { OpenJobObjectW(0x0004, 0, job_name.as_ptr()) };
    let job = OwnedHandle::from_nullable(job)?;
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    let process = OwnedHandle::from_nullable(process)?;
    require_process_package_identity(process.raw(), expected_package_full_name)?;
    let mut contained = 0;
    if unsafe { IsProcessInJob(process.raw(), job.raw(), &mut contained) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if contained == 0 {
        return Err(io::Error::other(
            "Windows activation process is outside the expected Job",
        ));
    }
    Ok(())
}

pub(crate) fn snapshot_process_ids() -> io::Result<HashSet<u32>> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    let snapshot = OwnedHandle::from_snapshot(snapshot)?;
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..PROCESSENTRY32W::default()
    };
    if unsafe { Process32FirstW(snapshot.raw(), &mut entry) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut process_ids = HashSet::new();
    loop {
        process_ids.insert(entry.th32ProcessID);
        if unsafe { Process32NextW(snapshot.raw(), &mut entry) } == 0 {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                Ok(process_ids)
            } else {
                Err(error)
            };
        }
    }
}

fn require_process_package_identity(
    process: HANDLE,
    expected_package_full_name: &str,
) -> io::Result<()> {
    let actual = process_package_full_name(process)?
        .ok_or_else(|| io::Error::other("activated Windows process has no package identity"))?;
    if actual != expected_package_full_name {
        return Err(io::Error::other(format!(
            "activated Windows process package identity mismatch: {actual}"
        )));
    }
    Ok(())
}

fn process_package_full_name(process: HANDLE) -> io::Result<Option<String>> {
    let mut length = 0;
    let first = unsafe { GetPackageFullName(process, &mut length, std::ptr::null_mut()) };
    if first == APPMODEL_ERROR_NO_PACKAGE {
        return Ok(None);
    }
    if first != ERROR_INSUFFICIENT_BUFFER || length == 0 {
        return Err(io::Error::from_raw_os_error(first as i32));
    }
    let mut wide = vec![0u16; length as usize];
    let status = unsafe { GetPackageFullName(process, &mut length, wide.as_mut_ptr()) };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let end = wide
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(wide.len());
    let actual = String::from_utf16(&wide[..end])
        .map_err(|_| io::Error::other("activated Windows process package identity is invalid"))?;
    Ok(Some(actual))
}

fn terminate_exact_process_after_attach_failure(process: HANDLE, primary: io::Error) -> io::Error {
    if unsafe { TerminateProcess(process, 1) } == 0 {
        return io::Error::other(format!(
            "{primary}; cannot terminate the exact activated process: {}",
            io::Error::last_os_error()
        ));
    }
    match wait_handle(process) {
        Ok(_) => primary,
        Err(error) => io::Error::other(format!(
            "{primary}; cannot wait for the exact activated process: {error}"
        )),
    }
}

fn try_wait_handle(process: HANDLE) -> io::Result<Option<ExitStatus>> {
    match unsafe { WaitForSingleObject(process, 0) } {
        WAIT_OBJECT_0 => exit_status(process).map(Some),
        WAIT_TIMEOUT => Ok(None),
        _ => Err(io::Error::last_os_error()),
    }
}

fn wait_handle(process: HANDLE) -> io::Result<ExitStatus> {
    if unsafe { WaitForSingleObject(process, INFINITE) } != WAIT_OBJECT_0 {
        return Err(io::Error::last_os_error());
    }
    exit_status(process)
}

fn exit_status(process: HANDLE) -> io::Result<ExitStatus> {
    let mut code = 0;
    if unsafe { GetExitCodeProcess(process, &mut code) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(ExitStatus::from_raw(code))
}

fn create_kill_on_close_job(name: Option<&[u16]>) -> io::Result<OwnedHandle> {
    let name = name.map_or(std::ptr::null(), |value| value.as_ptr());
    let raw = unsafe { CreateJobObjectW(std::ptr::null(), name) };
    let job = OwnedHandle::from_nullable(raw)?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job.raw(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if configured == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

fn resume_process_primary_thread(process_id: u32) -> io::Result<()> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    let snapshot = OwnedHandle::from_snapshot(snapshot)?;
    let thread_id = find_process_thread(snapshot.raw(), process_id)?;
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
    let thread = OwnedHandle::from_nullable(thread)?;
    if unsafe { ResumeThread(thread.raw()) } == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn find_process_thread(snapshot: HANDLE, process_id: u32) -> io::Result<u32> {
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    if unsafe { Thread32First(snapshot, &mut entry) } == 0 {
        return Err(io::Error::last_os_error());
    }
    loop {
        if entry.th32OwnerProcessID == process_id {
            return Ok(entry.th32ThreadID);
        }
        if unsafe { Thread32Next(snapshot, &mut entry) } == 0 {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("cannot find the suspended primary thread for process {process_id}"),
            ));
        }
    }
}

fn require_thread_owner(snapshot: HANDLE, thread_id: u32, process_id: u32) -> io::Result<()> {
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    if unsafe { Thread32First(snapshot, &mut entry) } == 0 {
        return Err(io::Error::last_os_error());
    }
    loop {
        if entry.th32ThreadID == thread_id {
            return if entry.th32OwnerProcessID == process_id {
                Ok(())
            } else {
                Err(io::Error::other(
                    "Windows package debugger thread does not belong to its process",
                ))
            };
        }
        if unsafe { Thread32Next(snapshot, &mut entry) } == 0 {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("cannot find Windows package debugger thread {thread_id}"),
            ));
        }
    }
}

#[derive(Debug)]
struct OwnedHandle(HANDLE);

#[derive(Debug)]
struct ThreadSafeOwnedHandle(usize);

impl OwnedHandle {
    fn from_nullable(handle: HANDLE) -> io::Result<Self> {
        if handle.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }

    fn from_snapshot(handle: HANDLE) -> io::Result<Self> {
        if handle == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }

    fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

impl ThreadSafeOwnedHandle {
    fn raw(&self) -> HANDLE {
        self.0 as HANDLE
    }
}

impl Drop for ThreadSafeOwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.raw());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, TcpListener, TcpStream};

    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    use crate::windows_activation_capability::WindowsActivationCapability;

    use super::{
        ipv4_connection_server_owner, is_primary_visible_window, require_process_package_identity,
        WindowsPendingJob,
    };

    #[test]
    fn input_method_status_window_is_not_a_primary_codex_window() {
        assert!(!is_primary_visible_window(true, 150, 150, 0x0808_0088));
        assert!(is_primary_visible_window(true, 1280, 820, 0x0004_0100));
        assert!(!is_primary_visible_window(false, 1280, 820, 0x0004_0100));
    }

    #[test]
    fn pending_job_uses_the_exact_activation_capability_name() {
        let capability = WindowsActivationCapability::create().expect("create capability");
        let pending =
            WindowsPendingJob::create_for_capability(&capability).expect("create capability Job");

        assert_eq!(pending.name(), capability.job_name());
    }

    #[test]
    fn established_connection_proof_identifies_the_server_process() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind fixture server");
        let port = listener.local_addr().expect("fixture address").port();
        let client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connect fixture");
        let (_server, _) = listener.accept().expect("accept fixture");

        let owner = ipv4_connection_server_owner(&client)
            .expect("query TCP owner")
            .expect("established server owner");

        assert_eq!(owner, std::process::id());
    }

    #[test]
    fn an_unrelated_pid_cannot_be_cleaned_up_as_the_activated_codex_package() {
        let error = require_process_package_identity(
            unsafe { GetCurrentProcess() },
            "OpenAI.Codex_26.820.7780.0_x64__2p2nqsd0c76g0",
        )
        .expect_err("test process is not the packaged Codex app");

        assert!(error.to_string().contains("package identity"), "{error}");
    }
}
