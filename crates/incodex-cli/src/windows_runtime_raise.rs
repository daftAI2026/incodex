//! 与 Electron node:net 的字节型唤回管道通信；连接、写入和分段回复共用一个截止时间。

use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{ERROR_IO_PENDING, ERROR_PIPE_BUSY, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile, FILE_FLAG_OVERLAPPED};
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

const RAISE_PIPE: &str = r"\\.\pipe\Incodex-Runtime-Raise";
const TIMEOUT: Duration = Duration::from_secs(3);
const CONNECT_RETRY: Duration = Duration::from_millis(25);

pub(crate) fn raise_existing() -> Result<(), String> {
    exchange().map_err(|error| format!("cannot raise the existing Windows Runtime: {error}"))
}

fn exchange() -> io::Result<()> {
    let deadline = Instant::now() + TIMEOUT;
    let pipe = loop {
        match OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(FILE_FLAG_OVERLAPPED)
            .open(RAISE_PIPE)
        {
            Ok(pipe) => break pipe,
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    || error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) =>
            {
                let remaining = remaining(deadline)?;
                thread::sleep(CONNECT_RETRY.min(remaining));
            }
            Err(error) => return Err(error),
        }
    };
    let mut request = *b"raise\n";
    let mut written = 0;
    while written < request.len() {
        written += transfer(&pipe, &mut request[written..], true, deadline)?;
    }
    let expected = b"raised\n";
    let mut response = [0u8; 7];
    let mut read = 0;
    while read < response.len() {
        read += transfer(&pipe, &mut response[read..], false, deadline)?;
        if response[..read] != expected[..read] {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid raise response",
            ));
        }
    }
    Ok(())
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|value| !value.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "raise deadline exceeded"))
}

fn transfer(pipe: &File, buffer: &mut [u8], write: bool, deadline: Instant) -> io::Result<usize> {
    remaining(deadline)?;
    let raw_event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
    if raw_event.is_null() {
        return Err(io::Error::last_os_error());
    }
    // 每次 I/O 独占事件；取消完成前，缓冲区、OVERLAPPED 和事件始终存活。
    let event = unsafe { OwnedHandle::from_raw_handle(raw_event) };
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    overlapped.hEvent = event.as_raw_handle() as HANDLE;
    let handle = pipe.as_raw_handle() as HANDLE;
    let started = unsafe {
        if write {
            WriteFile(
                handle,
                buffer.as_ptr(),
                buffer.len() as u32,
                ptr::null_mut(),
                &mut overlapped,
            )
        } else {
            ReadFile(
                handle,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                ptr::null_mut(),
                &mut overlapped,
            )
        }
    };
    if started == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
            return Err(error);
        }
        let wait = remaining(deadline).map(|duration| unsafe {
            WaitForSingleObject(overlapped.hEvent, duration.as_millis().max(1) as u32)
        });
        if !matches!(wait, Ok(WAIT_OBJECT_0)) {
            // OVERLAPPED 不能在内核仍引用它时离开栈；只取消本次精确操作。
            let mut ignored = 0;
            unsafe {
                CancelIoEx(handle, &overlapped);
                GetOverlappedResult(handle, &overlapped, &mut ignored, 1);
            }
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "raise deadline exceeded",
            ));
        }
    }
    let mut transferred = 0;
    if unsafe { GetOverlappedResult(handle, &overlapped, &mut transferred, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if transferred == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "raise pipe closed early",
        ));
    }
    Ok(transferred as usize)
}
