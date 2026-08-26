use std::io::{self, IsTerminal};

use crate::menu_controller::MenuKey;

use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Console::{
    GetConsoleMode, GetConsoleScreenBufferInfo, GetStdHandle, ReadConsoleInputW, SetConsoleMode,
    CONSOLE_SCREEN_BUFFER_INFO, ENABLE_PROCESSED_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
    INPUT_RECORD, KEY_EVENT, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_ESCAPE, VK_RETURN, VK_UP};

pub fn is_tty() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

pub(crate) fn stderr_columns() -> usize {
    let handle = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return 80;
    }
    let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
    if unsafe { GetConsoleScreenBufferInfo(handle, &mut info) } == 0 {
        return 80;
    }
    let columns = i32::from(info.srWindow.Right) - i32::from(info.srWindow.Left) + 1;
    usize::try_from(columns)
        .ok()
        .filter(|width| *width > 0)
        .unwrap_or(80)
}

pub fn enable_virtual_terminal() {
    enable_virtual_terminal_for(STD_OUTPUT_HANDLE);
    enable_virtual_terminal_for(STD_ERROR_HANDLE);
}

pub(crate) fn read_menu_key() -> Result<MenuKey, String> {
    let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    if input.is_null() || input == INVALID_HANDLE_VALUE {
        return Err("Windows console input is unavailable".to_string());
    }
    let _mode = ConsoleInputMode::without_processed_input(input)?;
    loop {
        let mut record = INPUT_RECORD::default();
        let mut read = 0u32;
        if unsafe { ReadConsoleInputW(input, &mut record, 1, &mut read) } == 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        if read == 0 || u32::from(record.EventType) != KEY_EVENT {
            continue;
        }
        let key = unsafe { record.Event.KeyEvent };
        if key.bKeyDown == 0 {
            continue;
        }
        match key.wVirtualKeyCode {
            VK_UP => return Ok(MenuKey::Up),
            VK_DOWN => return Ok(MenuKey::Down),
            VK_RETURN => return Ok(MenuKey::Activate),
            VK_ESCAPE => return Ok(MenuKey::Quit),
            _ => {}
        }
        let character = char::from_u32(u32::from(unsafe { key.uChar.UnicodeChar }));
        return Ok(match character {
            Some('k' | 'K') => MenuKey::Up,
            Some('j' | 'J') => MenuKey::Down,
            Some('q' | 'Q') => MenuKey::Quit,
            Some('u' | 'U') => MenuKey::Update,
            Some('v' | 'V') => MenuKey::Version,
            Some('\u{3}') => MenuKey::Interrupt,
            Some(character @ '1'..='9') => {
                MenuKey::Digit(character.to_digit(10).unwrap_or(0) as usize)
            }
            _ => MenuKey::Ignore,
        });
    }
}

struct ConsoleInputMode {
    handle: HANDLE,
    original: u32,
}

impl ConsoleInputMode {
    fn without_processed_input(handle: HANDLE) -> Result<Self, String> {
        let mut original = 0u32;
        if unsafe { GetConsoleMode(handle, &mut original) } == 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        if unsafe { SetConsoleMode(handle, original & !ENABLE_PROCESSED_INPUT) } == 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        Ok(Self { handle, original })
    }
}

impl Drop for ConsoleInputMode {
    fn drop(&mut self) {
        unsafe {
            SetConsoleMode(self.handle, self.original);
        }
    }
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
