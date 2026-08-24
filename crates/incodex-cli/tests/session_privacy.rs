use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_cli::lifecycle::run_self_uninstall;
use incodex_cli::parse::{CliCommand, ParsedCli};

fn temp_root() -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("incodex-session-privacy-{n}"));
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn source_self_uninstall_refusal_does_not_touch_codex_home() {
    let root = temp_root();
    let source = root.join(".codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "source-auth\n").unwrap();
    fs::write(source.join("config.toml"), "source-config\n").unwrap();

    let parsed = ParsedCli {
        command: CliCommand::SelfUninstall,
        help: false,
        clone: false,
        live: false,
        yes: true,
        dry_run: false,
        json: false,
        deep: false,
        restore_app: false,
        app: None,
        transaction: None,
        mask: false,
        name: None,
        avatar: None,
    };
    let error =
        run_self_uninstall(&parsed).expect_err("source self-uninstall must not remove itself");
    assert!(error.contains("running from source"), "{error}");
    assert_eq!(
        fs::read(source.join("auth.json")).unwrap(),
        b"source-auth\n"
    );
    assert_eq!(
        fs::read(source.join("config.toml")).unwrap(),
        b"source-config\n"
    );
}
