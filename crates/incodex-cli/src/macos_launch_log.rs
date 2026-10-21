/**
 * [INPUT]: 依赖 macOS Launch 的目标路径、闭集阶段与最终错误。
 * [OUTPUT]: 将无账号内容的 Launch 事件写入私有有界日志。
 * [POS]: Launch 诊断边界；写入失败不改变启动结果，不启动服务。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::macos_update_assets::{ensure_private_dir, read_regular_file, write_private_atomic};

const MAX_LOG_BYTES: usize = 4 * 1024;
const MAX_EVENT_COUNT: usize = 8;
const MAX_TARGET_BYTES: usize = 512;
const MAX_ERROR_BYTES: usize = 512;

#[derive(Clone, Copy)]
pub(crate) enum LaunchStage {
    Requested,
    InstalledRequested,
    CdpRequested,
    Ready,
    Failed,
}

impl LaunchStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::InstalledRequested => "installed-requested",
            Self::CdpRequested => "cdp-requested",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Event {
    at: u64,
    pid: u32,
    target: String,
    stage: String,
    error: Option<String>,
}

pub(crate) fn record(root: &Path, target: &Path, stage: LaunchStage, error: Option<&str>) {
    let _ = append(root, target, stage, error);
}

fn append(
    root: &Path,
    target: &Path,
    stage: LaunchStage,
    error: Option<&str>,
) -> Result<(), String> {
    let directory = root.join("launchers");
    ensure_private_dir(&directory)?;
    let path = directory.join("launch.log");
    let mut events = read_events(&path)?;
    events.push_back(Event {
        at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs()),
        pid: std::process::id(),
        target: bounded_text(&target.to_string_lossy(), MAX_TARGET_BYTES),
        stage: stage.as_str().to_string(),
        error: error.map(|value| bounded_text(value, MAX_ERROR_BYTES)),
    });
    while events.len() > MAX_EVENT_COUNT {
        events.pop_front();
    }

    let body = serialize_events(&mut events)?;
    write_private_atomic(&path, &body)
}

fn read_events(path: &Path) -> Result<VecDeque<Event>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err("refuse to use a symlink Launch log".into());
        }
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err("Launch log is not a regular file".into());
        }
        Ok(metadata) if metadata.len() > MAX_LOG_BYTES as u64 => return Ok(VecDeque::new()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(VecDeque::new());
        }
        Err(error) => return Err(format!("cannot inspect Launch log: {error}")),
    }
    let bytes = read_regular_file(path, "macOS Launch log")?;
    if bytes.len() > MAX_LOG_BYTES {
        return Ok(VecDeque::new());
    }
    let Ok(body) = std::str::from_utf8(&bytes) else {
        return Ok(VecDeque::new());
    };
    let mut events = VecDeque::new();
    for line in body.lines().filter(|line| !line.is_empty()) {
        let Ok(event) = serde_json::from_str::<Event>(line) else {
            return Ok(VecDeque::new());
        };
        if !is_known_stage(&event.stage)
            || event.target.len() > MAX_TARGET_BYTES
            || event
                .error
                .as_ref()
                .is_some_and(|value| value.len() > MAX_ERROR_BYTES)
        {
            return Ok(VecDeque::new());
        }
        events.push_back(event);
    }
    Ok(events)
}

fn serialize_events(events: &mut VecDeque<Event>) -> Result<Vec<u8>, String> {
    loop {
        let mut body = Vec::new();
        for event in events.iter() {
            serde_json::to_writer(&mut body, event)
                .map_err(|error| format!("cannot encode Launch log event: {error}"))?;
            body.push(b'\n');
        }
        if body.len() <= MAX_LOG_BYTES {
            return Ok(body);
        }
        if events.len() <= 1 {
            return Err("Launch log event exceeds its size limit".into());
        }
        events.pop_front();
    }
}

fn bounded_text(value: &str, max_bytes: usize) -> String {
    let mut bounded = String::new();
    for character in value.chars() {
        let character = if character.is_control() {
            ' '
        } else {
            character
        };
        if bounded.len() + character.len_utf8() > max_bytes {
            break;
        }
        bounded.push(character);
    }
    bounded
}

fn is_known_stage(stage: &str) -> bool {
    matches!(
        stage,
        "requested" | "installed-requested" | "cdp-requested" | "ready" | "failed"
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "incodex-macos-launch-log-{name}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn assert_event_fields(event: &serde_json::Value) {
        let mut fields = event
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        fields.sort_unstable();
        assert_eq!(fields, ["at", "error", "pid", "stage", "target"]);
    }

    #[test]
    fn event_records_target_stage_error_unix_time_and_pid() {
        use std::os::unix::fs::PermissionsExt;

        let root = Scratch::new("event");
        let target = Path::new("/Applications/ChatGPT.app");
        record(
            root.path(),
            target,
            LaunchStage::Failed,
            Some("LaunchServices could not open Codex"),
        );

        let path = root.path().join("launchers/launch.log");
        let body = fs::read_to_string(&path).expect("Launch event should be persisted");
        let event: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(event["target"], target.to_string_lossy().as_ref());
        assert_eq!(event["stage"], "failed");
        assert_eq!(event["error"], "LaunchServices could not open Codex");
        assert_eq!(event["pid"], std::process::id());
        assert!(event["at"].as_u64().unwrap() > 0);
        assert_event_fields(&event);
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn event_history_is_bounded_and_fields_are_sanitized() {
        let root = Scratch::new("bounds");
        let long_error = format!("failure\n{}", "e".repeat(MAX_ERROR_BYTES * 4));
        let unsafe_target =
            PathBuf::from(format!("/tmp/unsafe\n{}", "x".repeat(MAX_TARGET_BYTES * 2)));
        for index in 0..(MAX_EVENT_COUNT + 4) {
            record(
                root.path(),
                &unsafe_target,
                LaunchStage::Failed,
                Some(&long_error),
            );
            if index == 0 {
                record(
                    root.path(),
                    Path::new("/tmp/first.app"),
                    LaunchStage::Requested,
                    None,
                );
            }
        }

        let path = root.path().join("launchers/launch.log");
        let body = fs::read_to_string(path).expect("Launch event history should be persisted");
        assert!(body.len() <= MAX_LOG_BYTES);
        let events = body
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert!(events.len() <= MAX_EVENT_COUNT);
        assert_eq!(events.last().unwrap()["stage"], "failed");
        assert!(events.last().unwrap()["target"].as_str().unwrap().len() <= MAX_TARGET_BYTES);
        assert!(events.last().unwrap()["error"].as_str().unwrap().len() <= MAX_ERROR_BYTES);
        assert!(!events.last().unwrap()["target"]
            .as_str()
            .unwrap()
            .contains('\n'));
        assert!(!events.last().unwrap()["error"]
            .as_str()
            .unwrap()
            .contains('\n'));
        assert_eq!(events.last().unwrap().as_object().unwrap().len(), 5);
    }

    #[test]
    fn event_history_keeps_only_the_eight_most_recent_events() {
        let root = Scratch::new("recent-events");
        for index in 0..(MAX_EVENT_COUNT + 4) {
            record(
                root.path(),
                &PathBuf::from(format!("/tmp/target-{index}.app")),
                LaunchStage::Requested,
                None,
            );
        }

        let path = root.path().join("launchers/launch.log");
        let body = fs::read_to_string(path).expect("Launch event history should be persisted");
        let events = body
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(events.len(), MAX_EVENT_COUNT);
        assert_eq!(events.first().unwrap()["target"], "/tmp/target-4.app");
        assert_eq!(events.last().unwrap()["target"], "/tmp/target-11.app");
    }

    #[test]
    fn stage_names_are_closed_and_only_expected_fields_are_written() {
        let root = Scratch::new("closed-stages");
        let stages = [
            (LaunchStage::Requested, "requested"),
            (LaunchStage::InstalledRequested, "installed-requested"),
            (LaunchStage::CdpRequested, "cdp-requested"),
            (LaunchStage::Ready, "ready"),
            (LaunchStage::Failed, "failed"),
        ];
        for (stage, _) in stages {
            record(root.path(), Path::new("/tmp/target.app"), stage, None);
        }

        let path = root.path().join("launchers/launch.log");
        let body = fs::read_to_string(path).unwrap();
        let events = body
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        let recorded = events
            .iter()
            .map(|event| event["stage"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(recorded, stages.map(|(_, name)| name));
        for event in &events {
            assert_event_fields(event);
        }
    }

    #[test]
    fn symlink_log_is_left_untouched_and_does_not_change_the_caller() {
        use std::os::unix::fs::symlink;

        let root = Scratch::new("symlink");
        let directory = root.path().join("launchers");
        fs::create_dir_all(&directory).unwrap();
        let foreign = root.path().join("foreign.log");
        let original = b"foreign contents\n";
        fs::write(&foreign, original).unwrap();
        let path = directory.join("launch.log");
        symlink(&foreign, &path).unwrap();

        record(
            root.path(),
            Path::new("/tmp/target.app"),
            LaunchStage::Failed,
            Some("synthetic failure"),
        );

        assert!(fs::symlink_metadata(path).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(foreign).unwrap(), original);
    }

    #[test]
    fn corrupt_and_oversized_logs_reset_to_bounded_new_events() {
        let root = Scratch::new("invalid-history");
        let directory = root.path().join("launchers");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("launch.log");
        fs::write(&path, b"not-json\n").unwrap();
        record(
            root.path(),
            Path::new("/tmp/corrupt-reset.app"),
            LaunchStage::Requested,
            None,
        );
        let body = fs::read_to_string(&path).unwrap();
        let events = body.lines().collect::<Vec<_>>();
        assert_eq!(events.len(), 1);
        assert!(events[0].contains("corrupt-reset.app"));

        fs::write(&path, vec![b'x'; MAX_LOG_BYTES * 1024]).unwrap();
        record(
            root.path(),
            Path::new("/tmp/oversize-reset.app"),
            LaunchStage::Failed,
            Some("bounded failure"),
        );
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.len() <= MAX_LOG_BYTES);
        assert_eq!(body.lines().count(), 1);
        assert!(body.contains("oversize-reset.app"));
    }
}
