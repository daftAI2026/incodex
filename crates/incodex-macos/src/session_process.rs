//! Exact-marker process cleanup for helpers inherited by an isolated session.
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

const TERM_WAIT_ROUNDS: usize = 10;
const KILL_WAIT_ROUNDS: usize = 10;
const WAIT_STEP: Duration = Duration::from_millis(50);

pub fn session_process_ids_from_ps(snapshot: &str, session_root: &Path) -> Vec<i32> {
    let Some(root) = session_root.to_str() else {
        return Vec::new();
    };
    if root.is_empty() || root.contains(['\n', '\r']) {
        return Vec::new();
    }
    let marker = format!("INCODEX_SESSION_ROOT={root}");
    snapshot
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            let pid_end = trimmed.find(char::is_whitespace)?;
            let pid = trimmed[..pid_end].parse::<i32>().ok()?;
            let command = &trimmed[pid_end..];
            contains_exact_environment_marker(command, &marker).then_some(pid)
        })
        .filter(|pid| *pid > 0 && *pid != std::process::id() as i32)
        .collect()
}

pub fn quiesce_session_processes(session_root: &Path) -> Result<(), String> {
    let Some(root) = session_root.to_str() else {
        return Err("session root is not valid UTF-8; refusing process cleanup".into());
    };
    if root.is_empty() || root.contains(['\n', '\r']) {
        return Err("session root cannot be represented safely in a process marker".into());
    }

    let mut pids = session_process_ids(session_root)?;
    signal_processes(&pids, libc::SIGTERM);
    pids = wait_for_exit(pids, TERM_WAIT_ROUNDS);
    if !pids.is_empty() {
        signal_processes(&pids, libc::SIGKILL);
        let survivors = wait_for_exit(pids, KILL_WAIT_ROUNDS);
        if !survivors.is_empty() {
            return Err(format!(
                "isolated helper processes still running: {}",
                format_pids(&survivors)
            ));
        }
    }

    // 再取一次完整快照，捕捉 TERM 窗口内刚派生、随后被 reparent 的辅助进程。
    let late = session_process_ids(session_root)?;
    if !late.is_empty() {
        signal_processes(&late, libc::SIGKILL);
        let survivors = wait_for_exit(late, KILL_WAIT_ROUNDS);
        if !survivors.is_empty() {
            return Err(format!(
                "isolated helper processes still running: {}",
                format_pids(&survivors)
            ));
        }
    }

    Ok(())
}

fn contains_exact_environment_marker(command: &str, marker: &str) -> bool {
    command.match_indices(marker).any(|(start, _)| {
        let before_ok = start == 0
            || command[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        let end = start + marker.len();
        let after_ok = end == command.len()
            || command[end..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace);
        before_ok && after_ok
    })
}

fn session_process_ids(session_root: &Path) -> Result<Vec<i32>, String> {
    let output = Command::new("/bin/ps")
        .args(["axeww", "-o", "pid=,command="])
        .env("LC_ALL", "C")
        .output()
        .map_err(|error| format!("unable to inspect isolated helpers: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "unable to inspect isolated helpers: ps exited with {}",
            output.status
        ));
    }
    Ok(session_process_ids_from_ps(
        &String::from_utf8_lossy(&output.stdout),
        session_root,
    ))
}

fn signal_processes(pids: &[i32], signal: i32) {
    for pid in pids {
        // 进程可能在快照与信号之间自然退出；ESRCH 因此不是失败。
        unsafe {
            libc::kill(*pid, signal);
        }
    }
}

fn wait_for_exit(mut pids: Vec<i32>, rounds: usize) -> Vec<i32> {
    for _ in 0..rounds {
        pids.retain(|pid| process_is_live(*pid));
        if pids.is_empty() {
            return pids;
        }
        thread::sleep(WAIT_STEP);
    }
    pids.retain(|pid| process_is_live(*pid));
    pids
}

fn process_is_live(pid: i32) -> bool {
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

fn format_pids(pids: &[i32]) -> String {
    pids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}
