#![cfg(target_os = "windows")]

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use incodex_cli::windows_runtime_open::WindowsRuntimeOwnerClaim;

struct BytePipeServer(Child);

impl Drop for BytePipeServer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn server(reply: &str) -> BytePipeServer {
    // 与 Electron 的 node:net 服务端使用同一字节管道协议，不模拟 Win32 返回值。
    let script = format!(
        r#"const net = require('node:net');
const server = net.createServer(socket => {{
  socket.on('error', () => {{}});
  let input = '';
  socket.on('data', chunk => {{
    input += chunk;
    if (input !== 'raise\n') return;
    {reply}
  }});
}});
server.listen('\\\\.\\pipe\\Incodex-Runtime-Raise', () => console.log('listening'));
"#
    );
    let child = Command::new("bun")
        .args(["-e", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start test-owned byte pipe server");
    let mut server = BytePipeServer(child);
    let mut line = String::new();
    BufReader::new(server.0.stdout.take().expect("server stdout"))
        .read_line(&mut line)
        .expect("read server readiness");
    assert_eq!(line.trim(), "listening");
    server
}

#[test]
fn existing_runtime_raise_uses_bounded_byte_stream_framing() {
    let _owner = match WindowsRuntimeOwnerClaim::acquire().expect("claim fixture owner") {
        WindowsRuntimeOwnerClaim::Owned(owner) => owner,
        WindowsRuntimeOwnerClaim::Existing => panic!("close the real incognito window before testing"),
    };
    for (reply, succeeds) in [
        ("socket.end('raised\\n');", true),
        ("socket.write('rai'); setTimeout(() => socket.end('sed\\n'), 50);", true),
        ("socket.end('refused\\n');", false),
        ("socket.end('raised');", false),
        ("/* 保持连接但不回复，客户端必须在既有时限内结束。 */", false),
    ] {
        let _server = server(reply);
        let start = Instant::now();
        let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
            .args([
                "__incodex_windows_runtime_open",
                "--source-home",
                r"C:\incodex-unused-test-source",
                "--source-bounds",
                "0,0,960,720",
            ])
            .output()
            .expect("run native raise against the existing owner");
        assert_eq!(
            output.status.success(),
            succeeds,
            "reply={reply}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(start.elapsed() < Duration::from_secs(5), "raise must stay bounded");
        if succeeds {
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ready");
        } else {
            assert!(!String::from_utf8_lossy(&output.stdout).contains("ready"));
        }
    }
}
