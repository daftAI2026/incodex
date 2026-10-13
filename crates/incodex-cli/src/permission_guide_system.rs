/*
 * [INPUT]: 依赖当前 App 的 AX/录屏探针、进程身份与有界更新日志。
 * [OUTPUT]: 提供共用 GuideOps、同进程快照与封闭权限转场日志。
 * [POS]: 原生引导的 macOS 适配；日志不改权限，测试只清理自建目录。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;

pub(super) struct SystemGuideOps<'a> {
    pub(super) root: &'a Path,
    pub(super) app: &'a Path,
    pub(super) screen: Option<crate::permission_screen_probe::InstalledScreenProbe>,
}

impl GuideOps for SystemGuideOps<'_> {
    fn launch(&mut self) -> Result<(), String> {
        bounded_command(Command::new("/usr/bin/open").arg(self.app))
    }

    fn probe(&mut self) -> AccessibilityStatus {
        incodex_macos::inspect_accessibility_for_app(self.app).status
    }

    fn probe_screen(&mut self) -> Option<ScreenPermissionStatus> {
        Some(
            self.screen
                .as_mut()
                .map_or(ScreenPermissionStatus::Unknown, |probe| {
                    probe.probe(self.app)
                }),
        )
    }

    fn observe(&mut self, event: GuideTraceEvent) {
        let line = format!("{} worker_pid={}", event.log_event(), std::process::id());
        crate::macos_update_log::log_coordinator_event(self.root, &line);
    }

    fn snapshot_after_ax(&mut self, _: AccessibilityStatus) -> PermissionSnapshot {
        self.snapshot()
    }

    fn snapshot(&mut self) -> PermissionSnapshot {
        let app = self.app;
        coherent_snapshot(
            || {
                let report = incodex_macos::inspect_accessibility_for_app(app);
                let identity = report
                    .pid
                    .and_then(incodex_core::session::process_start_identity);
                (report, identity)
            },
            || self.probe_screen(),
        )
    }

    fn record_screen_intent(&mut self) -> Result<(), String> {
        self.screen
            .as_ref()
            .map_or(Ok(()), |probe| probe.record_screen_intent(self.app))
    }

    fn open_screen_settings(&mut self) -> Result<(), String> {
        bounded_command(&mut open_settings_command("x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_ScreenCapture"))
    }

    fn wait_for_window(&mut self) -> Result<(), String> {
        let target = incodex_macos::AppQuiescence::for_app(self.app)?;
        for _ in 0..80 {
            if incodex_macos::live_main_window_bounds(target.executable())?.is_some() {
                return Ok(());
            }
            self.wait(INITIAL_PROBE_INTERVAL);
        }
        Err("The target app's window did not appear; no reset was performed. Open ChatGPT and check with incodex doctor.".into())
    }

    fn lock_target(&mut self) -> Result<Option<incodex_transaction::TargetLock>, String> {
        incodex_transaction::acquire_target_lock(self.root, self.app, "permission-reset", None)
            .map(Some)
    }

    fn reset(&mut self) -> Result<(), String> {
        bounded_command(Command::new("/usr/bin/tccutil").args([
            "reset",
            "Accessibility",
            incodex_macos::OFFICIAL_BUNDLE_IDENTIFIER,
        ]))
    }

    fn open_settings(&mut self) -> Result<(), String> {
        use incodex_core::format_kv;
        let mut command = open_settings_command(ACCESSIBILITY_SETTINGS_URL);
        bounded_command(&mut command)?;
        println!(
            "{}",
            format_kv(
                "Accessibility",
                "Complete the Accessibility approval in System Settings. This guide checks automatically until you finish or close it.",
                None,
            )
        );
        Ok(())
    }

    fn wait(&mut self, duration: Duration) {
        thread::sleep(duration);
    }
}

#[cfg(test)]
mod trace_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn trace_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "incodex-permission-trace-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn inherited_trace_path_cannot_replace_existing_update_state() {
        let root = std::env::temp_dir().join(format!(
            "incodex-trace-ownership-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let directory = root.join("macos-update");
        fs::create_dir_all(&directory).unwrap();
        let sentinel = directory.join("unowned-state");
        fs::write(&sentinel, b"retain-existing-state").unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "accessibility_guide_host::system::trace_tests::trace_writes_only_closed_events_with_worker_pid_to_existing_log",
                "--nocapture",
            ])
            .env("INCX_PERMISSION_TRACE_LOG_ROOT", &root)
            .output()
            .unwrap();
        assert!(child.status.success(), "{child:?}");
        assert!(
            String::from_utf8_lossy(&child.stdout).contains("1 passed"),
            "the child must execute the real logging test: {child:?}"
        );
        let retained = fs::read(&sentinel).ok();
        fs::remove_dir_all(root).unwrap();
        assert_eq!(
            retained.as_deref(),
            Some(b"retain-existing-state".as_slice())
        );
    }

    #[test]
    fn trace_writes_only_closed_events_with_worker_pid_to_existing_log() {
        let root = trace_root();
        let events = [
            GuideTraceEvent::PreflightGranted,
            GuideTraceEvent::PreflightUnknown,
            GuideTraceEvent::CardReady,
            GuideTraceEvent::UserSkip,
            GuideTraceEvent::UserClose,
            GuideTraceEvent::HostEof,
            GuideTraceEvent::MachineInterrupted,
            GuideTraceEvent::CardCompleted,
        ];
        let mut ops = SystemGuideOps {
            root: &root,
            app: Path::new("/not-launched-by-this-test"),
            screen: None,
        };
        for event in events {
            ops.observe(event);
        }

        let path = root.join("macos-update/coordinator.log");
        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), events.len());
        for (line, event) in lines.iter().zip(events) {
            assert!(line.ends_with(&format!(
                "{} worker_pid={}",
                event.log_event(),
                std::process::id()
            )));
        }
        assert!(text.len() < 4 * 1024);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn trace_log_failure_does_not_escape_the_observer() {
        let root = std::env::temp_dir().join(format!(
            "incodex-permission-trace-file-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&root, b"not a directory").unwrap();
        let mut ops = SystemGuideOps {
            root: &root,
            app: Path::new("/not-launched-by-this-test"),
            screen: None,
        };
        ops.observe(GuideTraceEvent::CardReady);
        assert_eq!(fs::read(&root).unwrap(), b"not a directory");
        fs::remove_file(root).unwrap();
    }
}

// ---- 单次读数的进程边界 ----
fn coherent_snapshot<H, S>(mut host: H, mut screen: S) -> PermissionSnapshot
where
    H: FnMut() -> (incodex_macos::AccessibilityReport, Option<String>),
    S: FnMut() -> Option<ScreenPermissionStatus>,
{
    let (before, identity) = host();
    let screen = screen();
    let (after, current_identity) = host();
    if before.pid.is_none()
        || before.pid != after.pid
        || identity.is_none()
        || identity != current_identity
    {
        return PermissionSnapshot {
            accessibility: AccessibilityStatus::Unknown,
            screen: Some(ScreenPermissionStatus::Unknown),
        };
    }
    PermissionSnapshot {
        accessibility: after.status,
        screen,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    fn snapshot(generations: &[(Option<i32>, Option<&str>)]) -> PermissionSnapshot {
        let mut generations: VecDeque<_> = generations.iter().copied().collect();
        coherent_snapshot(
            || {
                let (pid, start) = generations.pop_front().unwrap();
                (
                    incodex_macos::AccessibilityReport {
                        status: AccessibilityStatus::Granted,
                        pid,
                        reason: None,
                    },
                    start.map(str::to_owned),
                )
            },
            || Some(ScreenPermissionStatus::Granted),
        )
    }

    #[test]
    fn restart_discards_mixed_snapshot_but_accepts_a_fresh_new_process_read() {
        let mixed = snapshot(&[(Some(41), Some("start-a")), (Some(42), Some("start-b"))]);
        assert_eq!(mixed.accessibility, AccessibilityStatus::Unknown);
        assert_eq!(mixed.screen, Some(ScreenPermissionStatus::Unknown));
        let fresh = snapshot(&[(Some(42), Some("start-b")), (Some(42), Some("start-b"))]);
        assert_eq!(fresh.accessibility, AccessibilityStatus::Granted);
        assert_eq!(fresh.screen, Some(ScreenPermissionStatus::Granted));
    }

    #[test]
    fn reused_pid_does_not_reuse_the_old_process_permission() {
        let mixed = snapshot(&[(Some(41), Some("start-a")), (Some(41), Some("start-b"))]);
        assert_eq!(mixed.accessibility, AccessibilityStatus::Unknown);
        assert_eq!(mixed.screen, Some(ScreenPermissionStatus::Unknown));
    }

    #[test]
    fn missing_process_or_start_identity_cannot_be_granted() {
        for generation in [(None, None), (Some(41), None)] {
            let missing = snapshot(&[generation, generation]);
            assert_eq!(missing.accessibility, AccessibilityStatus::Unknown);
            assert_eq!(missing.screen, Some(ScreenPermissionStatus::Unknown));
        }
    }
}
