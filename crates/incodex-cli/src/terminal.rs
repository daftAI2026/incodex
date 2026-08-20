use std::io::{self, IsTerminal};

const ESC: u8 = 0x1b;
const ESCAPE_BYTE_TIMEOUT_MS: i32 = 100;
const MAX_ESCAPE_SEQUENCE_BYTES: usize = 32;

pub fn is_tty() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

pub fn read_key() -> Result<Vec<u8>, String> {
    read_key_with_timeout(ESCAPE_BYTE_TIMEOUT_MS)
}

fn read_key_with_timeout(escape_timeout_ms: i32) -> Result<Vec<u8>, String> {
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

    let first = read_byte(fd)?;
    if first != ESC {
        return Ok(vec![first]);
    }
    read_escape_sequence(fd, escape_timeout_ms)
}

fn read_byte(fd: i32) -> Result<u8, String> {
    let mut byte = [0_u8; 1];
    loop {
        let count = unsafe { libc::read(fd, byte.as_mut_ptr().cast(), 1) };
        if count == 1 {
            return Ok(byte[0]);
        }
        if count == 0 {
            return Err(
                io::Error::new(io::ErrorKind::UnexpectedEof, "terminal closed").to_string(),
            );
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EINTR) {
            continue;
        }
        return Err(error.to_string());
    }
}

fn read_byte_with_timeout(fd: i32, timeout_ms: i32) -> Result<Option<u8>, String> {
    loop {
        let mut poll_fd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll_fd, 1, timeout_ms) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(error.to_string());
        }
        if ready == 0 {
            return Ok(None);
        }
        if poll_fd.revents & (libc::POLLIN | libc::POLLHUP) != 0 {
            return read_byte(fd).map(Some);
        }
    }
}

fn read_escape_sequence(fd: i32, timeout_ms: i32) -> Result<Vec<u8>, String> {
    let Some(introducer) = read_byte_with_timeout(fd, timeout_ms)? else {
        return Ok(vec![ESC]);
    };
    match introducer {
        b'[' => read_csi_sequence(fd, timeout_ms),
        b'O' => read_ss3_sequence(fd, timeout_ms),
        byte => Ok(vec![ESC, byte]),
    }
}

fn read_ss3_sequence(fd: i32, timeout_ms: i32) -> Result<Vec<u8>, String> {
    let Some(final_byte) = read_byte_with_timeout(fd, timeout_ms)? else {
        return Ok(vec![ESC, b'O']);
    };
    if matches!(final_byte, b'A' | b'B') {
        return Ok(vec![ESC, b'[', final_byte]);
    }
    Ok(vec![ESC, b'O', final_byte])
}

fn read_csi_sequence(fd: i32, timeout_ms: i32) -> Result<Vec<u8>, String> {
    let mut sequence = vec![ESC, b'['];
    for _ in 0..MAX_ESCAPE_SEQUENCE_BYTES {
        let Some(byte) = read_byte_with_timeout(fd, timeout_ms)? else {
            return Ok(sequence);
        };
        sequence.push(byte);
        // CSI final bytes are 0x40..=0x7e. Parameters and intermediates are
        // consumed one byte at a time, so a following key remains unread.
        if (0x40..=0x7e).contains(&byte) {
            if matches!(byte, b'A' | b'B') {
                return Ok(vec![ESC, b'[', byte]);
            }
            return Ok(sequence);
        }
    }
    Ok(sequence)
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
    use super::{read_key_with_timeout, ESCAPE_BYTE_TIMEOUT_MS};

    const PTY_ESCAPE_BYTE_TIMEOUT_MS: i32 = 2_000;

    // 通过真实伪终端驱动 read_key，避免只测字符串解析而漏掉 termios/poll 行为。
    fn read_key_from_pty(input: &[u8], gap_us: Option<u32>, timeout_ms: i32) -> Vec<u8> {
        unsafe {
            let mut master = -1;
            let mut report = [-1; 2];
            let mut ready = [-1; 2];
            assert_eq!(libc::pipe(report.as_mut_ptr()), 0);
            assert_eq!(libc::pipe(ready.as_mut_ptr()), 0);
            let pid = libc::forkpty(
                &mut master,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            assert!(
                pid >= 0,
                "forkpty failed: {}",
                std::io::Error::last_os_error()
            );
            if pid == 0 {
                libc::close(report[0]);
                libc::close(ready[0]);
                // 先把从端设为 raw，再通知父进程写入，避免 runner 调度让
                // 输入在 read_key 设置 termios 之前进入规范模式。
                let mut raw = std::mem::zeroed();
                let raw_ready = libc::tcgetattr(libc::STDIN_FILENO, &mut raw) == 0;
                if raw_ready {
                    libc::cfmakeraw(&mut raw);
                    raw.c_cc[libc::VMIN] = 1;
                    raw.c_cc[libc::VTIME] = 0;
                }
                let raw_ready =
                    raw_ready && libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) == 0;
                let signal = [u8::from(raw_ready)];
                let _ = libc::write(ready[1], signal.as_ptr().cast(), 1);
                libc::close(ready[1]);
                if !raw_ready {
                    libc::close(report[1]);
                    libc::_exit(1);
                }
                let key = read_key_with_timeout(timeout_ms).expect("child read_key");
                let length = [u8::try_from(key.len()).expect("key length")];
                let _ = libc::write(report[1], length.as_ptr().cast(), 1);
                let _ = libc::write(report[1], key.as_ptr().cast(), key.len());
                libc::close(report[1]);
                libc::_exit(0);
            }

            libc::close(report[1]);
            libc::close(ready[1]);
            let mut signal = [0_u8; 1];
            assert_eq!(libc::read(ready[0], signal.as_mut_ptr().cast(), 1), 1);
            assert_eq!(signal[0], 1, "child failed to enter raw mode");
            libc::close(ready[0]);
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
            read_key_from_pty(&[0x1b, b'O', b'A'], None, ESCAPE_BYTE_TIMEOUT_MS),
            vec![0x1b, b'[', b'A']
        );
        assert_eq!(
            read_key_from_pty(&[0x1b, b'O', b'B'], None, ESCAPE_BYTE_TIMEOUT_MS),
            vec![0x1b, b'[', b'B']
        );
    }

    #[test]
    fn modified_csi_arrows_are_normalized_to_menu_events() {
        assert_eq!(
            read_key_from_pty(
                &[0x1b, b'[', b'1', b';', b'2', b'A'],
                None,
                ESCAPE_BYTE_TIMEOUT_MS,
            ),
            vec![0x1b, b'[', b'A']
        );
        assert_eq!(
            read_key_from_pty(
                &[0x1b, b'[', b'1', b';', b'5', b'B'],
                None,
                ESCAPE_BYTE_TIMEOUT_MS,
            ),
            vec![0x1b, b'[', b'B']
        );
    }

    #[test]
    fn fragmented_escape_sequences_survive_byte_delays() {
        assert_eq!(
            read_key_from_pty(
                &[0x1b, b'[', b'A'],
                Some(50_000),
                PTY_ESCAPE_BYTE_TIMEOUT_MS,
            ),
            vec![0x1b, b'[', b'A']
        );
    }

    #[test]
    fn one_read_does_not_swallow_the_next_independent_arrow() {
        assert_eq!(
            read_key_from_pty(
                &[0x1b, b'[', b'A', 0x1b, b'[', b'B'],
                None,
                ESCAPE_BYTE_TIMEOUT_MS,
            ),
            vec![0x1b, b'[', b'A']
        );
    }

    #[test]
    fn production_escape_timeout_stays_at_100ms() {
        assert_eq!(ESCAPE_BYTE_TIMEOUT_MS, 100);
    }
}
