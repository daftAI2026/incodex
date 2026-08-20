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

#[cfg(test)]
mod tests {
    use super::read_key;

    // 通过真实伪终端驱动 read_key，避免只测字符串解析而漏掉 termios/poll 行为。
    fn read_key_from_pty(input: &[u8], gap_us: Option<u32>) -> Vec<u8> {
        unsafe {
            let mut master = -1;
            let mut report = [-1; 2];
            assert_eq!(libc::pipe(report.as_mut_ptr()), 0);
            let pid = libc::forkpty(
                &mut master,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            assert!(pid >= 0, "forkpty failed: {}", std::io::Error::last_os_error());
            if pid == 0 {
                libc::close(report[0]);
                let key = read_key().expect("child read_key");
                let length = [u8::try_from(key.len()).expect("key length")];
                let _ = libc::write(report[1], length.as_ptr().cast(), 1);
                let _ = libc::write(report[1], key.as_ptr().cast(), key.len());
                libc::close(report[1]);
                libc::_exit(0);
            }

            libc::close(report[1]);
            for (index, byte) in input.iter().enumerate() {
                let written = libc::write(master, std::slice::from_ref(byte).as_ptr().cast(), 1);
                assert_eq!(written, 1, "pty write failed");
                if gap_us.is_some() && index + 1 < input.len() {
                    libc::usleep(gap_us.unwrap());
                }
            }
            let mut length = [0_u8; 1];
            assert_eq!(libc::read(report[0], length.as_mut_ptr().cast(), 1), 1);
            let mut key = vec![0_u8; usize::from(length[0])];
            let mut offset = 0;
            while offset < key.len() {
                let count = libc::read(
                    report[0],
                    key[offset..].as_mut_ptr().cast(),
                    key.len() - offset,
                );
                assert!(count > 0, "child report truncated");
                offset += usize::try_from(count).unwrap();
            }
            libc::close(report[0]);
            libc::close(master);
            let mut status = 0;
            assert_eq!(libc::waitpid(pid, &mut status, 0), pid);
            assert!(libc::WIFEXITED(status), "child did not exit normally");
            key
        }
    }

    #[test]
    fn application_cursor_arrows_are_owned_by_the_menu() {
        assert_eq!(
            read_key_from_pty(&[0x1b, b'O', b'A'], None),
            vec![0x1b, b'[', b'A']
        );
        assert_eq!(
            read_key_from_pty(&[0x1b, b'O', b'B'], None),
            vec![0x1b, b'[', b'B']
        );
    }

    #[test]
    fn modified_csi_arrows_are_normalized_to_menu_events() {
        assert_eq!(
            read_key_from_pty(&[0x1b, b'[', b'1', b';', b'2', b'A'], None),
            vec![0x1b, b'[', b'A']
        );
        assert_eq!(
            read_key_from_pty(&[0x1b, b'[', b'1', b';', b'5', b'B'], None),
            vec![0x1b, b'[', b'B']
        );
    }

    #[test]
    fn fragmented_escape_sequences_survive_byte_delays() {
        assert_eq!(
            read_key_from_pty(&[0x1b, b'[', b'A'], Some(50_000)),
            vec![0x1b, b'[', b'A']
        );
    }

    #[test]
    fn one_read_does_not_swallow_the_next_independent_arrow() {
        assert_eq!(
            read_key_from_pty(&[0x1b, b'[', b'A', 0x1b, b'[', b'B'], None),
            vec![0x1b, b'[', b'A']
        );
    }
}
