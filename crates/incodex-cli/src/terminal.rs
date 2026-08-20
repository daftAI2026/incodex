use std::io::{self, IsTerminal, Read};

pub fn is_tty() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

pub fn read_key() -> Result<Vec<u8>, String> {
    let fd = libc::STDIN_FILENO;
    let mut original = unsafe {
        let mut value = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut value) != 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        value
    };
    let mut raw = original;
    unsafe {
        libc::cfmakeraw(&mut raw);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        if libc::tcsetattr(fd, libc::TCSANOW, &raw) != 0 {
            return Err(io::Error::last_os_error().to_string());
        }
    }
    let _guard = RestoreTerminal {
        fd,
        original: &mut original,
    };

    let mut first = [0_u8; 1];
    io::stdin()
        .read_exact(&mut first)
        .map_err(|err| err.to_string())?;
    let mut key = vec![first[0]];
    if first[0] != 0x1b {
        return Ok(key);
    }

    let mut poll_fd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let ready = unsafe { libc::poll(&mut poll_fd, 1, 30) };
    if ready > 0 && poll_fd.revents & libc::POLLIN != 0 {
        let mut rest = [0_u8; 7];
        let count = io::stdin().read(&mut rest).map_err(|err| err.to_string())?;
        key.extend_from_slice(&rest[..count]);
    }
    Ok(key)
}

struct RestoreTerminal<'a> {
    fd: i32,
    original: &'a mut libc::termios,
}

impl Drop for RestoreTerminal<'_> {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSANOW, self.original);
        }
    }
}
