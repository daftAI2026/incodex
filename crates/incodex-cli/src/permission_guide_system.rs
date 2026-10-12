/*
 * [INPUT]: 依赖当前 App 的 AX 检查、录屏 responder 与有界系统命令。
 * [OUTPUT]: 提供共用 GuideOps；仅显式行操作能打开设置或 reset AX。
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
