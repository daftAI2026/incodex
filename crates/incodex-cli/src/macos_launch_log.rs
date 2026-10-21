/**
 * [INPUT]: 依赖 macOS Launch 的目标路径、闭集阶段与最终错误。
 * [OUTPUT]: 将无账号内容的 Launch 事件写入私有有界日志。
 * [POS]: Launch 诊断边界；写入失败不改变启动结果，不启动服务。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::path::Path;

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

pub(crate) fn record(_root: &Path, _target: &Path, _stage: LaunchStage, _error: Option<&str>) {}

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

    #[test]
    fn event_records_target_stage_error_unix_time_and_pid() {
        let root = Scratch::new("event");
        let target = Path::new("/Applications/ChatGPT.app");
        record(
            root.path(),
            target,
            LaunchStage::Failed,
            Some("LaunchServices could not open Codex"),
        );

        let path = root.path().join("launchers/launch.log");
        let body = fs::read_to_string(path).expect("Launch event should be persisted");
        let event: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(event["target"], target.to_string_lossy().as_ref());
        assert_eq!(event["stage"], "failed");
        assert_eq!(event["error"], "LaunchServices could not open Codex");
        assert_eq!(event["pid"], std::process::id());
        assert!(event["at"].as_u64().unwrap() > 0);
        assert_eq!(event.as_object().unwrap().len(), 5);
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
}
