//! 挂起的 Store 进程只接受 Package Debugger 的调试器命令行；这里在恢复前
//! 用受支持的 x64 RTL_USER_PROCESS_PARAMETERS 布局追加 CDP 开关。

use std::ffi::c_void;
use std::io;
use std::mem::size_of;
use std::path::Path;

use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};

use crate::windows_process::process_command_line;

const X64_PEB_PROCESS_PARAMETERS_OFFSET: usize = 0x20;
const X64_PROCESS_PARAMETERS_COMMAND_LINE_OFFSET: usize = 0x70;
const WINDOWS_COMMAND_LINE_LIMIT_UTF16: usize = 32_767;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ProcessBasicInformation {
    exit_status: i32,
    peb_base_address: *mut c_void,
    affinity_mask: usize,
    base_priority: i32,
    unique_process_id: usize,
    inherited_from_unique_process_id: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RemoteUnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}

/// 在目标线程恢复前，将 localhost CDP 参数写入其既有命令行缓冲区。
pub fn rewrite_suspended_process_for_cdp(process_id: u32, debug_port: u16) -> io::Result<()> {
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (process_id, debug_port);
        return Err(io::Error::other(
            "suspended Windows command-line rewriting supports only x64 targets",
        ));
    }

    #[cfg(target_arch = "x86_64")]
    rewrite_suspended_x64_process_for_cdp(process_id, debug_port)
}

#[cfg(target_arch = "x86_64")]
fn rewrite_suspended_x64_process_for_cdp(process_id: u32, debug_port: u16) -> io::Result<()> {
    if debug_port == 0 {
        return Err(io::Error::other("Windows CDP port cannot be zero"));
    }
    let original = process_command_line(process_id)?;
    let access =
        PROCESS_QUERY_INFORMATION | PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE;
    let process = unsafe { OpenProcess(access, 0, process_id) };
    let process = OwnedHandle::from_nullable(process)?;
    let (command_line_address, current) = remote_command_line_descriptor(process.raw(), &original)?;
    let (executable, suffix) = split_executable_and_suffix(&original)?;
    let executable = Path::new(executable)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| {
            !name.is_empty()
                && !name
                    .chars()
                    .any(|character| character.is_whitespace() || matches!(character, '\0' | '"'))
        })
        .ok_or_else(|| io::Error::other("Windows process executable name is not safe"))?;
    let updated = format!(
        "{executable}{suffix} --remote-debugging-address=127.0.0.1 --remote-debugging-port={debug_port}"
    );
    let mut updated_wide = updated.encode_utf16().collect::<Vec<_>>();
    updated_wide.push(0);
    let byte_len = updated_wide
        .len()
        .checked_mul(size_of::<u16>())
        .ok_or_else(|| io::Error::other("Windows command line byte length overflowed"))?;
    if updated_wide.len() > WINDOWS_COMMAND_LINE_LIMIT_UTF16
        || byte_len > usize::from(current.maximum_length)
    {
        return Err(io::Error::other(
            "compacted Windows process command line does not fit its original buffer",
        ));
    }
    write_remote_bytes(
        process.raw(),
        current.buffer.cast(),
        updated_wide.as_ptr().cast(),
        byte_len,
        "compacted Windows process command line",
    )?;
    let descriptor = RemoteUnicodeString {
        length: ((updated_wide.len() - 1) * size_of::<u16>()) as u16,
        ..current
    };
    write_remote_bytes(
        process.raw(),
        command_line_address,
        (&descriptor as *const RemoteUnicodeString).cast(),
        size_of::<RemoteUnicodeString>(),
        "Windows process command line descriptor",
    )?;
    if process_command_line(process_id)? != updated {
        return Err(io::Error::other(
            "rewritten Windows process command line could not be verified",
        ));
    }
    Ok(())
}

#[cfg(target_arch = "x86_64")]
fn split_executable_and_suffix(command_line: &str) -> io::Result<(&str, &str)> {
    if let Some(rest) = command_line.strip_prefix('"') {
        let end = rest
            .find('"')
            .ok_or_else(|| io::Error::other("Windows process executable quote is unterminated"))?;
        return Ok((&rest[..end], &rest[end + 1..]));
    }
    let end = command_line
        .find(char::is_whitespace)
        .unwrap_or(command_line.len());
    if end == 0 {
        return Err(io::Error::other(
            "Windows process command line has no executable",
        ));
    }
    Ok((&command_line[..end], &command_line[end..]))
}

#[cfg(target_arch = "x86_64")]
fn remote_command_line_descriptor(
    process: HANDLE,
    expected: &str,
) -> io::Result<(*const c_void, RemoteUnicodeString)> {
    let mut basic = ProcessBasicInformation::default();
    let mut returned = 0u32;
    let status = unsafe {
        NtQueryInformationProcess(
            process,
            ProcessBasicInformation,
            (&mut basic as *mut ProcessBasicInformation).cast(),
            size_of::<ProcessBasicInformation>() as u32,
            &mut returned,
        )
    };
    if status < 0
        || returned < size_of::<ProcessBasicInformation>() as u32
        || basic.peb_base_address.is_null()
    {
        return Err(io::Error::other(format!(
            "cannot locate suspended Windows process parameters (NTSTATUS 0x{:08X})",
            status as u32
        )));
    }
    let process_parameters_address =
        (basic.peb_base_address as usize + X64_PEB_PROCESS_PARAMETERS_OFFSET) as *const c_void;
    let process_parameters = read_remote::<usize>(
        process,
        process_parameters_address,
        "Windows process parameters pointer",
    )?;
    if process_parameters == 0 {
        return Err(io::Error::other(
            "suspended Windows process has no process parameters",
        ));
    }
    let command_line_address =
        (process_parameters + X64_PROCESS_PARAMETERS_COMMAND_LINE_OFFSET) as *const c_void;
    let current = read_remote::<RemoteUnicodeString>(
        process,
        command_line_address,
        "Windows process command line descriptor",
    )?;
    verify_command_line_layout(process, &current, expected)?;
    Ok((command_line_address, current))
}

#[cfg(target_arch = "x86_64")]
fn verify_command_line_layout(
    process: HANDLE,
    descriptor: &RemoteUnicodeString,
    expected: &str,
) -> io::Result<()> {
    if descriptor.buffer.is_null()
        || !descriptor.length.is_multiple_of(2)
        || descriptor.length > descriptor.maximum_length
    {
        return Err(io::Error::other(
            "suspended Windows command line descriptor is invalid",
        ));
    }
    let mut actual = vec![0u16; usize::from(descriptor.length) / size_of::<u16>()];
    read_remote_bytes(
        process,
        descriptor.buffer.cast(),
        actual.as_mut_ptr().cast(),
        actual.len() * size_of::<u16>(),
        "suspended Windows command line",
    )?;
    if actual != expected.encode_utf16().collect::<Vec<_>>() {
        return Err(io::Error::other(
            "suspended Windows command line layout did not match the supported x64 contract",
        ));
    }
    Ok(())
}

#[cfg(target_arch = "x86_64")]
fn read_remote<T: Copy>(process: HANDLE, address: *const c_void, label: &str) -> io::Result<T> {
    let mut value = std::mem::MaybeUninit::<T>::uninit();
    read_remote_bytes(
        process,
        address,
        value.as_mut_ptr().cast(),
        size_of::<T>(),
        label,
    )?;
    Ok(unsafe { value.assume_init() })
}

#[cfg(target_arch = "x86_64")]
fn read_remote_bytes(
    process: HANDLE,
    address: *const c_void,
    target: *mut c_void,
    length: usize,
    label: &str,
) -> io::Result<()> {
    let mut read = 0usize;
    if unsafe { ReadProcessMemory(process, address, target, length, &mut read) } == 0
        || read != length
    {
        return Err(io::Error::other(format!(
            "cannot read {label}: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(())
}

#[cfg(target_arch = "x86_64")]
fn write_remote_bytes(
    process: HANDLE,
    address: *const c_void,
    source: *const c_void,
    length: usize,
    label: &str,
) -> io::Result<()> {
    let mut written = 0usize;
    if unsafe { WriteProcessMemory(process, address, source, length, &mut written) } == 0
        || written != length
    {
        return Err(io::Error::other(format!(
            "cannot write {label}: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(())
}

struct OwnedHandle(HANDLE);

impl OwnedHandle {
    fn from_nullable(handle: HANDLE) -> io::Result<Self> {
        if handle.is_null() {
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
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
