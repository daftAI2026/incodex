use std::io;
use std::io::{ErrorKind, Read, Write};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::WebSocket;

use super::{send_cdp_with_deadline, send_guarded_cdp};

struct PartialWouldBlockStream {
    written: Vec<u8>,
    write_calls: usize,
    response: Vec<u8>,
    response_offset: usize,
}

impl PartialWouldBlockStream {
    fn new(response: Vec<u8>) -> Self {
        Self {
            written: Vec::new(),
            write_calls: 0,
            response,
            response_offset: 0,
        }
    }
}

impl Read for PartialWouldBlockStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.response_offset == self.response.len() {
            return Err(io::Error::new(ErrorKind::WouldBlock, "response pending"));
        }
        let remaining = self.response.len() - self.response_offset;
        let length = remaining.min(buffer.len());
        buffer[..length]
            .copy_from_slice(&self.response[self.response_offset..self.response_offset + length]);
        self.response_offset += length;
        Ok(length)
    }
}

impl Write for PartialWouldBlockStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.write_calls += 1;
        match self.write_calls {
            1 => {
                let length = buffer.len().min(7);
                self.written.extend_from_slice(&buffer[..length]);
                Ok(length)
            }
            2 => Err(io::Error::new(ErrorKind::WouldBlock, "flush pending")),
            _ => {
                self.written.extend_from_slice(buffer);
                Ok(buffer.len())
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn decode_client_text_frames(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        assert!(
            bytes.len() - offset >= 6,
            "truncated client WebSocket frame"
        );
        assert_eq!(bytes[offset] & 0x8f, 0x81, "expected one final text frame");
        let length = (bytes[offset + 1] & 0x7f) as usize;
        assert!(length < 126, "test frame should use the short payload form");
        assert!(
            bytes.len() - offset >= 6 + length,
            "truncated frame payload"
        );
        assert_ne!(bytes[offset + 1] & 0x80, 0, "client frame must be masked");
        let mask = &bytes[offset + 2..offset + 6];
        let payload = &bytes[offset + 6..offset + 6 + length];
        frames.push(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4])
                .collect(),
        );
        offset += 6 + length;
    }
    frames
}

#[test]
fn cdp_command_partial_flush_does_not_duplicate_frame() {
    let response = br#"{"id":1,"result":{}}"#;
    let frame = std::iter::once(0x81_u8)
        .chain(std::iter::once(response.len() as u8))
        .chain(response.iter().copied())
        .collect::<Vec<_>>();
    let stream = PartialWouldBlockStream::new(frame);
    let mut socket = WebSocket::from_raw_socket(stream, tungstenite::protocol::Role::Client, None);

    let result = send_cdp_with_deadline(
        &mut socket,
        1,
        "Runtime.evaluate",
        json!({}),
        Instant::now() + Duration::from_secs(1),
    );
    assert!(
        result.is_ok(),
        "partial flush should be retried: {result:?}"
    );

    let frames = decode_client_text_frames(&socket.get_ref().written);
    assert_eq!(frames.len(), 1, "one CDP command must produce one frame");
    let command: Value = serde_json::from_slice(&frames[0]).unwrap();
    assert_eq!(command.get("id").and_then(Value::as_u64), Some(1));
    assert_eq!(
        command.get("method").and_then(Value::as_str),
        Some("Runtime.evaluate")
    );
}

#[test]
fn guarded_cdp_command_writes_nothing_after_listener_ownership_changes() {
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let client = std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    server
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let mut socket = WebSocket::from_raw_socket(client, tungstenite::protocol::Role::Client, None);

    let result = send_guarded_cdp(
        &mut socket,
        5,
        "Runtime.evaluate",
        json!({ "expression": "sensitive profile mask" }),
        &|_| Err("Windows CDP connection ownership changed".to_string()),
    );

    assert!(result.is_err());
    let mut byte = [0_u8; 1];
    assert!(
        server.read(&mut byte).is_err(),
        "ownership must be proven immediately before a CDP command is written"
    );
}
