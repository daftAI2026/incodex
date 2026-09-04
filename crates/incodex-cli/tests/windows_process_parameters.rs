#![cfg(target_os = "windows")]

use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_cli::windows_process::process_command_line;
use incodex_cli::windows_process_parameters::rewrite_suspended_process_for_cdp;
use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;

struct LongPathExecutable {
    root: PathBuf,
    path: PathBuf,
}

impl LongPathExecutable {
    fn copy_from(source: &Path) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "incodex-process-parameters-{}-{unique}",
            std::process::id()
        ));
        let parent = root.join("x".repeat(96));
        fs::create_dir_all(&parent).expect("create long-path executable fixture");
        let path = parent.join(source.file_name().expect("test executable file name"));
        fs::copy(source, &path).expect("copy long-path executable fixture");
        Self { root, path }
    }
}

impl Drop for LongPathExecutable {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn rewrites_cdp_switches_inside_the_existing_process_parameter_buffer() {
    let source = std::env::current_exe().expect("current test executable");
    let executable = LongPathExecutable::copy_from(&source);
    let mut command = Command::new(&executable.path);
    command.arg("--list").creation_flags(CREATE_SUSPENDED);
    let mut child = command.spawn().expect("spawn suspended process");

    let rewritten = rewrite_suspended_process_for_cdp(child.id(), 43192);
    let after = process_command_line(child.id());
    let _ = child.kill();
    let _ = child.wait();

    rewritten.expect("rewrite suspended process for CDP");
    let after = after.expect("read rewritten command line");
    let file_name = executable
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("test executable file name");
    assert!(after.starts_with(file_name), "{after}");
    assert!(after.contains(" --list"), "{after}");
    assert!(
        after.ends_with(" --remote-debugging-address=127.0.0.1 --remote-debugging-port=43192"),
        "{after}"
    );
    assert!(
        !after.contains(&executable.path.display().to_string()),
        "{after}"
    );
}
