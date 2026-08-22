use super::*;

fn run_tty(
    program: &str,
    prefix: &[&str],
    args: &[&str],
    home: &Path,
    wait_for: &str,
    keys: &str,
) -> CliResult {
    let result = support::tty::run(program, prefix, args, home, wait_for, keys);
    CliResult {
        status: result.status,
        stdout: result.stdout,
        stderr: result.stderr,
    }
}

fn run_tty_with_columns(
    program: &str,
    prefix: &[&str],
    args: &[&str],
    home: &Path,
    wait_for: &str,
    keys: &str,
    columns: u16,
) -> CliResult {
    let result =
        support::tty::run_with_columns(program, prefix, args, home, wait_for, keys, columns);
    CliResult {
        status: result.status,
        stdout: result.stdout,
        stderr: result.stderr,
    }
}

fn visible(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if code.is_ascii() && ('@'..='~').contains(&code) {
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    out.replace('\r', "")
}

fn count(text: &str, needle: &str) -> usize {
    text.match_indices(needle).count()
}

fn has_spinner_frame(text: &str, message: &str) -> bool {
    let text = visible(text);
    ["|", "/", "-", "\\"]
        .iter()
        .any(|frame| text.contains(&format!("  {frame} {message}")))
}

fn assert_menu_order(text: &str, expected: &[&str]) {
    let mut previous = 0;
    for item in expected.iter().copied() {
        let position = text
            .find(item)
            .unwrap_or_else(|| panic!("menu missing {item:?}: {text}"));
        assert!(position >= previous, "menu item order changed: {text}");
        previous = position;
    }
}

#[test]
fn native_tty_menu_prints_the_product_order_and_controls() {
    let home = scratch("menu");
    let rust = run_tty(rust_bin(), &[], &[], &home, "Quit", "q");
    assert_eq!(rust.status, 0, "{}", visible(&rust.stdout));
    assert_eq!(rust.stderr, "");
    let rust = visible(&rust.stdout);
    assert_menu_order(
        &rust,
        &[
            "1. Open",
            "2. Install",
            "3. Uninstall",
            "4. Status",
            "5. Doctor",
            "6. Quit",
        ],
    );
    for text in [
        "_____   _   _",
        "https://github.com/daftAI2026/incodex",
        "Incognito toggle for Codex desktop.",
        "4. Status",
        "5. Doctor",
        "6. Quit",
        "↑↓ | Enter | V Version | Q Quit | 1-6 Jump",
    ] {
        assert!(rust.contains(text), "Rust menu missing {text:?}: {rust}");
    }
}

#[test]
fn native_menu_shows_the_cached_update_notice_and_shortcut() {
    let rust_home = scratch("menu-update");
    let cache = rust_home.join(".incodex/cache/update_message");
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    fs::write(&cache, "Update 9.9.9 available, run incodex update\n").unwrap();
    let rust_install = rust_home.join("prefix/bin/incodex");
    fs::create_dir_all(rust_install.parent().unwrap()).unwrap();
    fs::copy(rust_bin(), &rust_install).unwrap();
    let rust = run_tty(
        rust_install.to_str().unwrap(),
        &[],
        &[],
        &rust_home,
        "Quit",
        "q",
    );
    let rust = visible(&rust.stdout);
    for text in [
        "Update 9.9.9 available, run incodex update",
        "↑↓ | Enter | U Update | V Version | Q Quit | 1-6 Jump",
    ] {
        assert!(rust.contains(text), "Rust menu missing {text:?}: {rust}");
    }
}

#[test]
fn native_open_animates_while_waiting_for_cdp_readiness_and_clears_its_line() {
    let home = scratch("open-spinner");
    let app = sleeping_open_app(&home);
    let args = ["open", "--app", app.to_str().unwrap()];
    let rust = run_tty(
        rust_bin(),
        &[],
        &args,
        &home,
        "Closed. Isolated session removed.",
        "",
    );
    assert_eq!(rust.status, 3, "{}", rust.stdout);
    assert!(
        rust.stdout.contains("Waiting for Codex UI to become ready"),
        "missing opening readiness animation: {}",
        visible(&rust.stdout)
    );
    assert!(
        has_spinner_frame(&rust.stdout, "Waiting for Codex UI to become ready"),
        "missing spinner frames: {:?}",
        rust.stdout
    );
    assert!(
        rust.stdout.contains("\r\u{1b}[2K"),
        "spinner must clear the current line: {:?}",
        rust.stdout
    );
}

#[test]
fn native_spinner_styles_the_glyph_and_avoids_repeated_line_erases() {
    let home = scratch("spinner-rendering");
    let app = sleeping_open_app(&home);
    let rust = run_tty(
        rust_bin(),
        &[],
        &["open", "--app", app.to_str().unwrap()],
        &home,
        "Closed. Isolated session removed.",
        "",
    );
    assert_eq!(rust.status, 3, "{}", visible(&rust.stdout));
    assert!(
        rust.stdout.contains("  \u{1b}[1;34m|\u{1b}[0m Waiting"),
        "spinner glyph must be blue and bold without styling the message: {:?}",
        rust.stdout
    );
    assert!(
        ["/", "-", "\\"].iter().any(|frame| rust
            .stdout
            .contains(&format!("\r  \u{1b}[1;34m{frame}\u{1b}[0m Waiting"))),
        "unchanged frames should return to column zero without erasing the line: {:?}",
        rust.stdout
    );
}

#[test]
fn native_spinner_truncates_long_messages_in_a_narrow_terminal() {
    let home = scratch("spinner-narrow");
    let app = sleeping_open_app(&home);
    let rust = run_tty_with_columns(
        rust_bin(),
        &[],
        &["open", "--app", app.to_str().unwrap()],
        &home,
        "Closed. Isolated session removed.",
        "",
        24,
    );
    assert_eq!(rust.status, 3, "{}", visible(&rust.stdout));
    let output = visible(&rust.stdout);
    assert!(
        output.contains("  | Waiting for Codex..."),
        "narrow spinner did not preserve a single visible line: {output:?}"
    );
    assert!(
        !output.contains("Waiting for Codex UI to become ready"),
        "narrow spinner leaked the unbounded message: {output:?}"
    );
}

#[test]
fn native_tty_uninstall_animates_immediately_after_confirmation() {
    let home = scratch("uninstall-progress-tty");
    let app = patchable_app(&home);
    let install = run_rust(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(install.status, 0, "{install:?}");

    let rust = run_tty(
        rust_bin(),
        &[],
        &["uninstall", "--app", app.to_str().unwrap()],
        &home,
        "Press Enter to confirm, ESC to cancel: ",
        "\r",
    );
    assert_eq!(rust.status, 0, "{}", visible(&rust.stdout));
    assert!(
        has_spinner_frame(&rust.stdout, "Restoring original app"),
        "confirmation was followed by a silent uninstall: {:?}",
        rust.stdout
    );
    assert!(
        rust.stdout.contains("\r\u{1b}[2K"),
        "uninstall spinner must clear the current line: {:?}",
        rust.stdout
    );
    assert!(
        visible(&rust.stdout).contains("App restored. App registration was refreshed."),
        "missing final uninstall result: {}",
        visible(&rust.stdout)
    );
}

#[test]
fn native_tty_install_animates_immediately_after_confirmation() {
    let home = scratch("install-progress-tty");
    let app = patchable_app(&home);
    let rust = run_tty(
        rust_bin(),
        &[],
        &["install", "--app", app.to_str().unwrap()],
        &home,
        "Press Enter to confirm, ESC to cancel: ",
        "\r",
    );
    assert_eq!(rust.status, 0, "{}", visible(&rust.stdout));
    assert!(
        has_spinner_frame(&rust.stdout, "Backing up original app"),
        "confirmation was followed by a silent install: {:?}",
        rust.stdout
    );
    assert!(
        rust.stdout.contains("\r\u{1b}[2K"),
        "install spinner must clear the current line: {:?}",
        rust.stdout
    );
    assert!(
        has_spinner_frame(&rust.stdout, "Replacing the app"),
        "install should expose a product phase instead of a transaction primitive: {:?}",
        rust.stdout
    );
    for internal in ["Preparing installation transaction", "Swapping application"] {
        assert!(
            !rust.stdout.contains(internal),
            "TTY leaked internal phase {internal:?}: {:?}",
            rust.stdout
        );
    }
}

#[test]
fn native_tty_failure_clears_progress_before_printing_the_error() {
    let home = scratch("failed-progress-tty");
    let app = patchable_app(&home);
    let rust = run_tty(
        rust_bin(),
        &[],
        &["uninstall", "--app", app.to_str().unwrap()],
        &home,
        "Press Enter to confirm, ESC to cancel: ",
        "\r",
    );
    assert_eq!(rust.status, 1, "{}", visible(&rust.stdout));
    let clear = rust
        .stdout
        .rfind("\r\u{1b}[2K")
        .expect("failed progress must clear its current line");
    let error = rust
        .stdout
        .rfind("no installation record for this target")
        .expect("missing explicit uninstall error");
    assert!(
        clear < error,
        "error was printed before progress cleanup: {:?}",
        rust.stdout
    );
    assert!(
        visible(&rust.stdout).contains("  ✗ no installation record for this target"),
        "error should follow the CLI body indentation and mark: {}",
        visible(&rust.stdout)
    );
}

#[test]
fn native_tty_runtime_animates_and_clears_its_line() {
    let home = scratch("runtime-progress-tty");
    let rust = run_tty(
        rust_bin(),
        &[],
        &["runtime"],
        &home,
        "Runtime updated. Codex was not modified.",
        "",
    );
    assert_eq!(rust.status, 0, "{}", visible(&rust.stdout));
    assert!(
        has_spinner_frame(&rust.stdout, "Publishing Runtime"),
        "runtime publish was silent: {:?}",
        rust.stdout
    );
    assert!(
        rust.stdout.contains("\r\u{1b}[2K"),
        "runtime spinner must clear the current line: {:?}",
        rust.stdout
    );
}

#[test]
fn native_tty_status_and_doctor_animate_without_changing_machine_output() {
    for (command, stage, result) in [
        ("status", "Inspecting installation status", "➤ Status"),
        ("doctor", "Running diagnostics", "➤ App"),
    ] {
        let home = scratch(&format!("{command}-progress-tty"));
        let app = marker_app(&home);
        let rust = run_tty(
            rust_bin(),
            &[],
            &[command, "--app", app.to_str().unwrap()],
            &home,
            result,
            "",
        );
        assert_eq!(rust.status, 0, "{command}: {}", visible(&rust.stdout));
        assert!(
            has_spinner_frame(&rust.stdout, stage),
            "{command} was silent: {:?}",
            rust.stdout
        );
        assert!(
            rust.stdout.contains("\r\u{1b}[2K"),
            "{command} spinner did not clear: {:?}",
            rust.stdout
        );

        let json = run_rust(&[command, "--json", "--app", app.to_str().unwrap()], &home);
        assert_eq!(json.status, 0, "{json:?}");
        assert!(
            serde_json::from_str::<serde_json::Value>(&json.stdout).is_ok(),
            "{command} progress corrupted JSON: {json:?}"
        );
        assert!(!json.stdout.contains(stage), "{json:?}");
    }
}

#[test]
fn native_tty_recover_animates_until_the_transaction_is_restored() {
    let home = scratch("recover-progress-tty");
    let app = patchable_app(&home);
    assert!(
        Command::new("codesign")
            .args(["--force", "--deep", "--sign", "-", "--"])
            .arg(&app)
            .status()
            .unwrap()
            .success(),
        "fixture must start with a verifiable signature"
    );
    let root = home.join(".incodex");
    let mut transaction = Engine::begin(&root, &app, "install").unwrap();
    let id = transaction.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&id)
        .join("original/ChatGPT.app");
    ditto(&app, &original).unwrap();
    transaction.mark_backup_committed().unwrap();
    let staged = root
        .join("scratch")
        .join(format!("ChatGPT.app.staged-{id}"));
    ditto(&app, &staged).unwrap();
    transaction.place_staging(&staged).unwrap();
    transaction.swap().unwrap();
    drop(transaction);

    let rust = run_tty(
        rust_bin(),
        &[],
        &["recover", "--transaction", &id],
        &home,
        "outgoing restored: true",
        "",
    );
    assert_eq!(rust.status, 0, "{}", visible(&rust.stdout));
    assert!(
        has_spinner_frame(&rust.stdout, "Recovering transaction"),
        "recover was silent: {:?}",
        rust.stdout
    );
    assert!(
        rust.stdout.contains("\r\u{1b}[2K"),
        "recover spinner must clear the current line: {:?}",
        rust.stdout
    );
}

#[test]
fn native_tty_self_uninstall_animates_while_removing_the_cli() {
    let home = scratch("self-uninstall-progress-tty");
    let bin = home.join("prefix/bin");
    fs::create_dir_all(&bin).unwrap();
    let installed = bin.join("incodex");
    fs::copy(rust_bin(), &installed).unwrap();
    fs::copy(rust_bin(), bin.join("inc")).unwrap();

    let rust = run_tty(
        installed.to_str().unwrap(),
        &[],
        &["self-uninstall"],
        &home,
        "Press Enter to confirm, ESC to cancel: ",
        "\r",
    );
    assert_eq!(rust.status, 0, "{}", visible(&rust.stdout));
    assert!(
        has_spinner_frame(&rust.stdout, "Removing Incodex CLI"),
        "self-uninstall was silent: {:?}",
        rust.stdout
    );
    assert!(
        rust.stdout.contains("\r\u{1b}[2K"),
        "self-uninstall spinner must clear the current line: {:?}",
        rust.stdout
    );
    assert!(!installed.exists());
    assert!(!bin.join("inc").exists());
}

#[test]
fn native_tty_install_and_uninstall_ask_once_and_escape_aborts() {
    for command in ["install", "uninstall"] {
        let home = scratch(command);
        let app = marker_app(&home);
        let args = [command, "--app", app.to_str().unwrap()];
        let rust = run_tty(
            rust_bin(),
            &[],
            &args,
            &home,
            "Press Enter to confirm, ESC to cancel: ",
            "\u{1b}",
        );
        let output = visible(&rust.stdout);
        assert_eq!(
            count(&output, "Press Enter to confirm, ESC to cancel: "),
            1,
            "{command}: {output}"
        );
        assert!(output.contains("aborted"), "{command}: {output}");
        assert_eq!(
            fs::read_to_string(app.join("marker")).unwrap(),
            "do-not-touch\n"
        );
    }
}
