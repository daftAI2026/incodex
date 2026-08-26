use std::ffi::c_void;
use std::io;
use std::mem::size_of;
use std::net::Ipv4Addr;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, ExitStatus};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows_sys::Win32::Networking::WinSock::AF_INET;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, OpenThread, ResumeThread, CREATE_SUSPENDED, PROCESS_QUERY_LIMITED_INFORMATION,
    THREAD_SUSPEND_RESUME,
};

#[derive(Debug)]
pub struct WindowsProcessTree {
    child: Child,
    _job: OwnedHandle,
}

impl WindowsProcessTree {
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    pub fn terminate(&mut self) -> io::Result<ExitStatus> {
        if let Some(status) = self.child.try_wait()? {
            return Ok(status);
        }
        if unsafe { TerminateJobObject(self._job.raw(), 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        self.child.wait()
    }

    pub fn listener_owner_is_in_job(&self, port: u16) -> io::Result<bool> {
        let Some(owner_pid) = ipv4_listener_owner(port)? else {
            return Ok(false);
        };
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, owner_pid) };
        let process = OwnedHandle::from_nullable(process)?;
        let mut contained = 0;
        if unsafe { IsProcessInJob(process.raw(), self._job.raw(), &mut contained) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(contained != 0)
    }
}

fn ipv4_listener_owner(port: u16) -> io::Result<Option<u32>> {
    let mut bytes = 0;
    let first = unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut bytes,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
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
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let table = unsafe { &*storage.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>() };
    let rows =
        unsafe { std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize) };
    Ok(rows.iter().find_map(|row: &MIB_TCPROW_OWNER_PID| {
        let local_address = Ipv4Addr::from(u32::from_be(row.dwLocalAddr));
        let local_port = u16::from_be(row.dwLocalPort as u16);
        (local_address == Ipv4Addr::LOCALHOST && local_port == port).then_some(row.dwOwningPid)
    }))
}

pub fn spawn_kill_on_drop(command: &mut Command) -> io::Result<WindowsProcessTree> {
    let job = create_kill_on_close_job()?;
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

    Ok(WindowsProcessTree { child, _job: job })
}

fn create_kill_on_close_job() -> io::Result<OwnedHandle> {
    let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
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

#[derive(Debug)]
struct OwnedHandle(HANDLE);

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
