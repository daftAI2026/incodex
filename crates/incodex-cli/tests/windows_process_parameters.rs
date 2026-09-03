#![cfg(target_os = "windows")]

use std::os::windows::process::CommandExt;
use std::process::Command;

use incodex_cli::windows_process::process_command_line;
use incodex_cli::windows_process_parameters::rewrite_suspended_process_for_cdp;
use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;

#[test]
fn rewrites_cdp_switches_inside_the_existing_process_parameter_buffer() {
    let executable = std::env::current_exe().expect("current test executable");
    let mut command = Command::new(&executable);
    command.arg("--list").creation_flags(CREATE_SUSPENDED);
    let mut child = command.spawn().expect("spawn suspended process");

    let rewritten = rewrite_suspended_process_for_cdp(child.id(), 43192);
    let after = process_command_line(child.id());
    let _ = child.kill();
    let _ = child.wait();

    rewritten.expect("rewrite suspended process for CDP");
    let after = after.expect("read rewritten command line");
    let file_name = executable
        .file_name()
        .and_then(|name| name.to_str())
        .expect("test executable file name");
    assert!(after.starts_with(file_name), "{after}");
    assert!(after.contains(" --list"), "{after}");
    assert!(
        after.ends_with(" --remote-debugging-address=127.0.0.1 --remote-debugging-port=43192"),
        "{after}"
    );
    assert!(!after.contains(&executable.display().to_string()), "{after}");
}
