/*
 * [INPUT]: 依赖当前 App 的 AX 检查、录屏 responder 与进程启动身份。
 * [OUTPUT]: 提供共用 GuideOps 与同进程权限快照；只有行操作执行设置或 reset。
 * [POS]: 原生引导的 macOS 能力适配；更新检查不调用 launch。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;

pub(super) struct SystemGuideOps<'a> {
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
                "Complete the Accessibility approval in System Settings. Checking automatically for up to two minutes.",
                None,
            )
        );
        Ok(())
    }

    fn wait(&mut self, duration: Duration) {
        thread::sleep(duration);
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
