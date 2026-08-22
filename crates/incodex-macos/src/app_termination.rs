use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_POLL_INTERVAL: Duration = Duration::from_millis(20);

const NORMAL_TERMINATION_SCRIPT: &str = r#"
ObjC.import("AppKit");

function run(argv) {
  const expected = String(argv[0]);
  for (const rawPid of argv.slice(1)) {
    const pid = Number(rawPid);
    const app = $.NSRunningApplication.runningApplicationWithProcessIdentifier(pid);
    if (!app || app.js === null) continue;
    const url = app.executableURL;
    const actual = url && url.js !== null ? ObjC.unwrap(url.path) : "";
    if (actual !== expected) {
      throw new Error(`process ${pid} executable changed; refusing termination request`);
    }
    app.terminate;
  }
  return "requested";
}
"#;

pub(crate) fn request_normal_termination(executable: &Path, pids: &[i32]) -> Result<(), String> {
    let args = termination_args(executable, pids);
    let mut child = Command::new("/usr/bin/osascript")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start normal app termination request: {error}"))?;
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    loop {
        if child
            .try_wait()
            .map_err(|error| format!("cannot observe normal app termination request: {error}"))?
            .is_some()
        {
            let output = child.wait_with_output().map_err(|error| {
                format!("cannot finish normal app termination request: {error}")
            })?;
            if output.status.success() {
                return Ok(());
            }
            let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if error.is_empty() {
                format!(
                    "normal app termination request exited with {}",
                    output.status
                )
            } else {
                error
            });
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "normal app termination request did not return within {} seconds",
                REQUEST_TIMEOUT.as_secs()
            ));
        }
        thread::sleep(REQUEST_POLL_INTERVAL);
    }
}

fn termination_args(executable: &Path, pids: &[i32]) -> Vec<String> {
    let mut args = vec![
        "-l".into(),
        "JavaScript".into(),
        "-e".into(),
        NORMAL_TERMINATION_SCRIPT.into(),
        "--".into(),
        executable.display().to_string(),
    ];
    args.extend(pids.iter().map(i32::to_string));
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_uses_normal_pid_targeted_termination() {
        assert!(NORMAL_TERMINATION_SCRIPT.contains("runningApplicationWithProcessIdentifier"));
        assert!(NORMAL_TERMINATION_SCRIPT.contains("app.terminate"));
        assert!(!NORMAL_TERMINATION_SCRIPT.contains("forceTerminate"));
        assert!(!NORMAL_TERMINATION_SCRIPT.contains("application id"));
        assert!(!NORMAL_TERMINATION_SCRIPT.contains("kill"));
    }

    #[test]
    fn arguments_include_the_exact_executable_and_every_pid() {
        let executable = Path::new("/Applications/ChatGPT.app/Contents/MacOS/ChatGPT");
        let args = termination_args(executable, &[42, 43]);
        assert_eq!(
            &args[5..],
            &[
                "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
                "42",
                "43"
            ]
        );
    }
}
