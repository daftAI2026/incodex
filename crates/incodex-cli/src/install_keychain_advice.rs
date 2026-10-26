/**
 * [INPUT]: 依赖官方 App 身份与本次安装是否为默认新安装。
 * [OUTPUT]: 为默认官方新安装打印 Storage Key 授权说明。
 * [POS]: CLI 安装收尾的只读提示；不检查 Keychain，也不发起授权。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::path::Path;

use incodex_core::canonical::is_official_app;
use incodex_core::format_warn;
use incodex_macos::{read_plist_info, OFFICIAL_BUNDLE_IDENTIFIER};

const STORAGE_KEY_ADVICE: &[&str] = &[
    "Keychain: On next launch, macOS may ask this patched Codex app to access Codex Storage Key.",
    "Confirm the dialog names this app and the Codex Storage Key item.",
    "If both match, enter your Mac login password (not your ChatGPT password) and choose Always Allow.",
    "Allow or Allow Once grants only that access and may prompt again later.",
    "If the details do not match, choose Deny; Incodex and Terminal never need that password.",
];

const ADVICE_SECTIONS: &[&[&str]] = &[STORAGE_KEY_ADVICE];

fn advice_sections() -> &'static [&'static [&'static str]] {
    ADVICE_SECTIONS
}

pub(crate) fn print_if_applicable(app: &Path, new_install: bool, explicit_target: bool) {
    let codex_bundle = read_plist_info(app)
        .is_some_and(|info| info.bundle_identifier == OFFICIAL_BUNDLE_IDENTIFIER);
    if !advice_is_allowed(
        new_install,
        is_official_app(app, None),
        codex_bundle,
        explicit_target,
    ) {
        return;
    }

    for section in advice_sections() {
        for message in section.iter().copied() {
            println!("{}", format_warn(message, None));
        }
    }
}

fn advice_is_allowed(
    new_install: bool,
    official_target: bool,
    codex_bundle: bool,
    explicit_target: bool,
) -> bool {
    cfg!(target_os = "macos") && new_install && official_target && codex_bundle && !explicit_target
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advice_requires_a_new_implicit_official_codex_install() {
        assert_eq!(
            advice_is_allowed(true, true, true, false),
            cfg!(target_os = "macos")
        );
        assert!(!advice_is_allowed(true, true, true, true));
        assert!(!advice_is_allowed(false, true, true, false));
        assert!(!advice_is_allowed(true, false, true, false));
        assert!(!advice_is_allowed(true, true, false, false));
    }

    #[test]
    fn device_key_advice_is_separate_and_keeps_the_password_in_the_system_dialog() {
        let storage = STORAGE_KEY_ADVICE.join(" ");
        let device_section = advice_sections()
            .iter()
            .find(|section| !section.is_empty() && section[0].starts_with("Device-key helper:"))
            .expect("the device-key helper needs its own user-facing guidance");
        let device_key = device_section.join(" ");
        assert!(device_key.contains("incodex-device-key-helper"));
        assert!(device_key.contains("Codex device key"));
        assert!(device_key.contains("macOS dialog"));
        assert!(device_key.contains("Mac login password"));
        assert!(device_key.contains("Always Allow"));
        assert!(device_key.contains("Deny"));
        assert!(device_key.contains("Terminal"));
        assert!(device_key.contains("may ask again"));
        assert!(!storage.contains("incodex-device-key-helper"));
    }
}
