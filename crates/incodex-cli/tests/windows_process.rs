#![cfg(target_os = "windows")]

use std::fs;
use std::net::{Ipv4Addr, TcpListener};
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use incodex_cli::windows_process::spawn_kill_on_drop;
use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

const FIXTURE_ENV: &str = "INCODEX_WINDOWS_JOB_FIXTURE";
const PID_FILE_ENV: &str = "INCODEX_WINDOWS_JOB_PID_FILE";
const PORT_FILE_ENV: &str = "INCODEX_WINDOWS_JOB_PORT_FILE";
const WAIT_LIMIT: Duration = Duration::from_secs(10);
const WAIT_STEP: Duration = Duration::from_millis(25);

fn scratch_pid_file() -> PathBuf {
    std::env::temp_dir().join(format!(
        "incodex-windows-job-{}-后代.pid",
        std::process::id()
    ))
}

#[test]
fn helper_process_tree_fixture() {
    if std::env::var_os(FIXTURE_ENV).is_none() {
        return;
    }

    let descendant = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 60",
        ])
        .spawn()
        .expect("spawn descendant");
    let pid_file = PathBuf::from(std::env::var_os(PID_FILE_ENV).expect("pid file"));
    fs::write(pid_file, descendant.id().to_string()).expect("publish descendant pid");
    drop(descendant);

    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

#[test]
fn owned_listener_fixture() {
    let Some(port_file) = std::env::var_os(PORT_FILE_ENV) else {
        return;
    };
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind owned listener");
    fs::write(
        PathBuf::from(port_file),
        listener
            .local_addr()
            .expect("listener address")
            .port()
            .to_string(),
    )
    .expect("publish listener port");
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

#[test]
fn closing_job_kills_the_root_and_its_descendant() {
    let pid_file = scratch_pid_file();
    let _ = fs::remove_file(&pid_file);
    let mut command = Command::new(std::env::current_exe().expect("current test binary"));
    command
        .args(["helper_process_tree_fixture", "--exact", "--nocapture"])
        .env(FIXTURE_ENV, "1")
        .env(PID_FILE_ENV, &pid_file);

    let process_tree = spawn_kill_on_drop(&mut command).expect("spawn contained process tree");
    let root_pid = process_tree.id();
    let descendant_pid = wait_for_descendant_pid(&pid_file);
    assert!(process_is_running(root_pid), "root exited before job close");
    assert!(
        process_is_running(descendant_pid),
        "descendant exited before job close"
    );

    drop(process_tree);

    assert!(wait_until_stopped(root_pid), "root survived job close");
    assert!(
        wait_until_stopped(descendant_pid),
        "descendant survived job close"
    );
    fs::remove_file(pid_file).expect("remove pid fixture");
}

fn wait_for_descendant_pid(path: &PathBuf) -> u32 {
    let deadline = Instant::now() + WAIT_LIMIT;
    while Instant::now() < deadline {
        if let Ok(raw) = fs::read_to_string(path) {
            return raw.trim().parse().expect("valid descendant pid");
        }
        thread::sleep(WAIT_STEP);
    }
    panic!("descendant did not publish its pid");
}

fn wait_until_stopped(pid: u32) -> bool {
    let deadline = Instant::now() + WAIT_LIMIT;
    while Instant::now() < deadline {
        if !process_is_running(pid) {
            return true;
        }
        thread::sleep(WAIT_STEP);
    }
    !process_is_running(pid)
}

fn process_is_running(pid: u32) -> bool {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let mut exit_code = 0;
    let queried = unsafe { GetExitCodeProcess(handle, &mut exit_code) } != 0;
    unsafe {
        CloseHandle(handle);
    }
    queried && exit_code == STILL_ACTIVE as u32
}

#[test]
fn proves_that_a_debug_listener_belongs_to_the_contained_job() {
    let port_file = scratch_pid_file().with_extension("port");
    let _ = fs::remove_file(&port_file);
    let mut command = Command::new(std::env::current_exe().expect("current test binary"));
    command
        .args(["owned_listener_fixture", "--exact", "--nocapture"])
        .env(PORT_FILE_ENV, &port_file);
    let process_tree = spawn_kill_on_drop(&mut command).expect("spawn listener job");
    let owned_port = wait_for_descendant_pid(&port_file) as u16;
    let unrelated = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind unrelated listener");
    let unrelated_port = unrelated.local_addr().expect("unrelated address").port();

    assert!(
        process_tree
            .listener_owner_is_in_job(owned_port)
            .expect("inspect owned listener"),
        "contained listener was not attributed to its job"
    );
    assert!(
        !process_tree
            .listener_owner_is_in_job(unrelated_port)
            .expect("inspect unrelated listener"),
        "unrelated listener was attributed to the job"
    );

    drop(process_tree);
    drop(unrelated);
    fs::remove_file(port_file).expect("remove port fixture");
}
