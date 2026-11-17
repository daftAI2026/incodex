/*
 * [INPUT]: 依赖真实匿名管道、可继承写端和自有 GUI 子进程夹具。
 * [OUTPUT]: 验证监听进程存活时发布管道仍能 EOF，防止 updater 等待后台进程。
 * [POS]: 观察器启动边界的低层回归；不启动官方 App，也不修改产品状态。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;
use std::fs;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{
    GetLastError, SetHandleInformation, ERROR_BROKEN_PIPE, HANDLE_FLAG_INHERIT,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::Pipes::{CreatePipe, PeekNamedPipe};

struct Fixture {
    root: std::path::PathBuf,
    image: std::path::PathBuf,
    child: Option<ObserverProcess>,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "incodex-observer-pipe-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let directory = root.join("path with spaces");
        fs::create_dir(&directory).unwrap();
        let image = directory.join("observer fixture.exe");
        let output = Command::new("rustc")
            .args(["--edition=2021"])
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/windows-observer-pipe-child.rs"),
            )
            .arg("-o")
            .arg(&image)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Self {
            root,
            image,
            child: None,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::write(self.image.parent().unwrap().join("release"), b"release");
        if let Some(child) = self.child.as_mut() {
            let deadline = Instant::now() + Duration::from_secs(2);
            while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        self.child.take();
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn live_observer_does_not_inherit_an_unrelated_update_pipe() {
    let mut fixture = Fixture::new();
    let security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let (mut read, mut write) = (std::ptr::null_mut(), std::ptr::null_mut());
    assert_ne!(
        unsafe { CreatePipe(&mut read, &mut write, &security, 0) },
        0
    );
    let reader = unsafe { OwnedHandle::from_raw_handle(read) };
    let writer = unsafe { OwnedHandle::from_raw_handle(write) };
    assert_ne!(
        unsafe { SetHandleInformation(read, HANDLE_FLAG_INHERIT, 0) },
        0
    );
    // Cargo 的自有 Job 不准 breakaway；这里只隔离句柄继承，不冒充真实观察器的 Job 验收。
    fixture.child = Some(
        spawn_with_flags(
            &fixture.image,
            super::super::MODE,
            windows_sys::Win32::System::Threading::CREATE_NO_WINDOW,
        )
        .unwrap(),
    );
    let ready = fixture.image.parent().unwrap().join("ready");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() && Instant::now() < deadline {
        assert!(fixture
            .child
            .as_mut()
            .unwrap()
            .try_wait()
            .unwrap()
            .is_none());
        thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists(), "fixture did not reach its live gate");
    assert!(fixture
        .child
        .as_mut()
        .unwrap()
        .try_wait()
        .unwrap()
        .is_none());
    drop(writer);
    let deadline = Instant::now() + Duration::from_secs(1);
    let eof = loop {
        let result = unsafe {
            PeekNamedPipe(
                reader.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if result == 0 {
            assert_eq!(unsafe { GetLastError() }, ERROR_BROKEN_PIPE);
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert!(
        fixture
            .child
            .as_mut()
            .unwrap()
            .try_wait()
            .unwrap()
            .is_none(),
        "EOF must not be explained by observer exit"
    );
    assert!(
        eof,
        "a live observer retained the updater's unrelated pipe writer"
    );
}
