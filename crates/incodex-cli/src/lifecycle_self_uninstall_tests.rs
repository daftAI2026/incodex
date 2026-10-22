/**
 * [INPUT]: 依赖临时CLI路径、隔离目录与显式self-uninstall计划。
 * [OUTPUT]: 验证dry-run、确认、未知入口保护与用户数据保留。
 * [POS]: 自卸载文件动作的隔离适配；不运行产品二进制。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::finish_self_uninstall;
use crate::macos_launch_icon::publish_launcher_at_with_renderer;
use crate::macos_launch_paths::LauncherDirectories;
use crate::macos_launch_remove::{print_self_uninstall_plan_at, remove_self_uninstall_entries};
use crate::parse::{CliCommand, ParsedCli};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "incodex-self-uninstall-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("System Applications")).unwrap();
        fs::create_dir_all(root.join("home")).unwrap();
        Self(root)
    }

    fn home(&self) -> PathBuf {
        self.0.join("home")
    }

    fn directories(&self) -> LauncherDirectories {
        LauncherDirectories::isolated(
            &self.0.join("System Applications"),
            &self.home().join("Applications"),
        )
    }

    fn installed_cli(&self) -> [PathBuf; 2] {
        let bin = self.home().join("prefix/bin");
        fs::create_dir_all(&bin).unwrap();
        let cli = bin.join("incodex");
        let alias = bin.join("inc");
        for path in [&cli, &alias] {
            fs::write(path, b"fake installed CLI").unwrap();
        }
        [cli, alias]
    }

    fn owned_launcher(&self) -> PathBuf {
        let app = self.0.join("Fixture.app");
        let cli = self.0.join("source-incodex");
        fs::create_dir_all(&app).unwrap();
        fs::write(&cli, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
        publish_launcher_at_with_renderer(&self.home(), &cli, &app, &self.directories(), |_| {
            Ok(crate::macos_launch_icon::ICON_BYTES.to_vec())
        })
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn parsed(yes: bool, dry_run: bool) -> ParsedCli {
    ParsedCli {
        command: CliCommand::SelfUninstall,
        help: false,
        clone: false,
        live: false,
        yes,
        dry_run,
        remove_launcher: false,
        json: false,
        deep: false,
        restore_app: false,
        app: None,
        transaction: None,
        mask: false,
        name: None,
        avatar: None,
    }
}

#[test]
fn dry_run_and_confirmation_keep_all_owned_files() {
    let fixture = Fixture::new();
    let launcher = fixture.owned_launcher();
    let cli_paths = fixture.installed_cli();
    let plan = print_self_uninstall_plan_at(&fixture.home(), &fixture.directories());

    finish_self_uninstall(&parsed(false, true), cli_paths.clone(), || {
        remove_self_uninstall_entries(&plan)
    })
    .unwrap();
    assert!(launcher.exists());
    assert!(cli_paths.iter().all(|path| path.exists()));

    let plan = print_self_uninstall_plan_at(&fixture.home(), &fixture.directories());
    let error = finish_self_uninstall(&parsed(false, false), cli_paths.clone(), || {
        remove_self_uninstall_entries(&plan)
    })
    .expect_err("non-TTY self-uninstall requires --yes");
    assert!(error.contains("requires --yes"), "{error}");
    assert!(launcher.exists());
    assert!(cli_paths.iter().all(|path| path.exists()));
}

#[test]
fn confirmed_self_uninstall_removes_owned_entry_and_cli_but_keeps_state() {
    let fixture = Fixture::new();
    let launcher = fixture.owned_launcher();
    let cli_paths = fixture.installed_cli();
    let home = fixture.home();
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::write(home.join(".codex/session.sqlite"), b"keep session").unwrap();
    fs::create_dir_all(home.join(".incodex/runtime")).unwrap();
    fs::write(home.join(".incodex/runtime/current.json"), b"keep Runtime").unwrap();
    let plan = print_self_uninstall_plan_at(&home, &fixture.directories());

    finish_self_uninstall(&parsed(true, false), cli_paths.clone(), || {
        remove_self_uninstall_entries(&plan)
    })
    .unwrap();

    assert!(!launcher.exists());
    assert!(cli_paths.iter().all(|path| !path.exists()));
    assert_eq!(
        fs::read(home.join(".codex/session.sqlite")).unwrap(),
        b"keep session"
    );
    assert_eq!(
        fs::read(home.join(".incodex/runtime/current.json")).unwrap(),
        b"keep Runtime"
    );
}

#[test]
fn confirmed_self_uninstall_keeps_unknown_same_name_entry() {
    let fixture = Fixture::new();
    let cli_paths = fixture.installed_cli();
    let unknown = fixture.directories().primary_bundle();
    fs::create_dir_all(&unknown).unwrap();
    fs::write(unknown.join("keep.txt"), b"external app").unwrap();
    let plan = print_self_uninstall_plan_at(&fixture.home(), &fixture.directories());

    finish_self_uninstall(&parsed(true, false), cli_paths.clone(), || {
        remove_self_uninstall_entries(&plan)
    })
    .unwrap();

    assert!(cli_paths.iter().all(|path| !path.exists()));
    assert_eq!(fs::read(unknown.join("keep.txt")).unwrap(), b"external app");
    assert!(!fixture.home().join(".incodex").exists());
}
