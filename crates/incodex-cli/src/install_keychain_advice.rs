use std::path::Path;

use incodex_core::canonical::is_official_app;
use incodex_core::format_warn;
use incodex_macos::{read_plist_info, OFFICIAL_BUNDLE_IDENTIFIER};

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

    for message in [
        "Keychain: On next launch, macOS may ask this patched Codex app to access Codex Storage Key.",
        "Confirm the dialog names this app and the Codex Storage Key item.",
        "If both match, enter your Mac login password (not your ChatGPT password) and choose Always Allow.",
        "Allow or Allow Once grants only that access and may prompt again later.",
        "If the details do not match, choose Deny; Incodex and Terminal never need that password.",
    ] {
        println!("{}", format_warn(message, None));
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
}
