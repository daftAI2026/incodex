use std::io::{self, IsTerminal};

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Console::{
    GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
    STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
};

pub fn is_tty() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

pub fn enable_virtual_terminal() {
    enable_virtual_terminal_for(STD_OUTPUT_HANDLE);
    enable_virtual_terminal_for(STD_ERROR_HANDLE);
}

fn enable_virtual_terminal_for(stream: u32) {
    let handle = unsafe { GetStdHandle(stream) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return;
    }
    let mut mode = 0;
    if unsafe { GetConsoleMode(handle, &mut mode) } == 0 {
        return;
    }
    unsafe {
        SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
    }
}
