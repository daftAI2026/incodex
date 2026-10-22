/**
 * [INPUT]: 依赖显式临时目录、Launch参数与V4/V2入口夹具。
 * [OUTPUT]: 验证dry-run、确认删除、自卸载与外部入口保留。
 * [POS]: 移除流程的隔离文件系统合同；从不访问系统Applications。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::{print_self_uninstall_plan_at, remove_self_uninstall_entries, run_remove_at};
use crate::macos_launch_icon::publish_launcher_at_with_renderer;
use crate::macos_launch_paths::LauncherDirectories;
use crate::parse::{CliCommand, ParsedCli};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "incodex-launch-remove-{}-{}",
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

    fn owned_current(&self) -> PathBuf {
        let home = self.home();
        let app = self.0.join("Fixture.app");
        let cli = self.0.join("incodex");
        fs::create_dir_all(&app).unwrap();
        fs::write(&cli, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
        publish_launcher_at_with_renderer(&home, &cli, &app, &self.directories(), |_| {
            Ok(crate::macos_launch_icon::ICON_BYTES.to_vec())
        })
        .unwrap()
    }

    fn owned_legacy_v2(&self) -> PathBuf {
        let bundle = self.home().join("Applications/Incodex Launcher.app");
        let contents = bundle.join("Contents");
        let macos = contents.join("MacOS");
        let resources = contents.join("Resources");
        fs::create_dir_all(&macos).unwrap();
        fs::create_dir_all(&resources).unwrap();
        fs::write(contents.join("Info.plist"), LEGACY_V2_INFO_PLIST).unwrap();
        fs::write(
            macos.join("IncodexLauncher"),
            "#!/bin/sh\n# incodex-launcher-detached-v1\n/usr/bin/nohup '/tmp/incodex' launch --app '/Applications/ChatGPT.app' </dev/null >/dev/null 2>&1 &\n",
        )
        .unwrap();
        fs::set_permissions(
            macos.join("IncodexLauncher"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        fs::write(
            resources.join(".incodex-launcher-owner"),
            b"incodex-launcher-owner-v1\n",
        )
        .unwrap();
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/incodex-launcher.icns"),
            resources.join("IncodexLauncher.icns"),
        )
        .unwrap();
        bundle
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn launch_remove(yes: bool, dry_run: bool) -> ParsedCli {
    ParsedCli {
        command: CliCommand::Launch,
        help: false,
        clone: false,
        live: false,
        yes,
        dry_run,
        remove_launcher: true,
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

const LEGACY_V2_INFO_PLIST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>IncodexLauncher</string><key>CFBundleIdentifier</key><string>com.daftai.incodex.launcher</string><key>CFBundleName</key><string>Incodex Launcher</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>2</string><key>CFBundleIconFile</key><string>IncodexLauncher.icns</string><key>LSUIElement</key><true/></dict></plist>\n";

#[test]
fn remove_dry_run_keeps_both_exact_entries_and_private_state() {
    let fixture = Fixture::new();
    let current = fixture.owned_current();
    let legacy = fixture.owned_legacy_v2();
    let home = fixture.home();
    fs::remove_dir_all(home.join(".incodex")).unwrap();

    run_remove_at(&launch_remove(false, true), &home, &fixture.directories()).unwrap();

    assert!(current.exists());
    assert!(legacy.exists());
    assert!(!home.join(".incodex").exists());
    assert!(!home.join(".codex").exists());
}

#[test]
fn removal_requires_yes_then_deletes_only_owned_entries() {
    let fixture = Fixture::new();
    let current = fixture.owned_current();
    let legacy = fixture.owned_legacy_v2();
    let unrelated = fixture.0.join("System Applications/Unrelated.app");
    fs::create_dir_all(&unrelated).unwrap();
    fs::write(unrelated.join("keep.txt"), b"unrelated").unwrap();
    let home = fixture.home();
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::write(home.join(".codex/session.sqlite"), b"session").unwrap();
    fs::create_dir_all(home.join(".incodex/runtime")).unwrap();
    fs::write(home.join(".incodex/runtime/current.json"), b"runtime").unwrap();

    let refusal =
        run_remove_at(&launch_remove(false, false), &home, &fixture.directories()).unwrap_err();
    assert!(refusal.contains("requires --yes"), "{refusal}");
    assert!(current.exists());
    assert!(legacy.exists());

    run_remove_at(&launch_remove(true, false), &home, &fixture.directories()).unwrap();

    assert!(!current.exists());
    assert!(!legacy.exists());
    assert_eq!(fs::read(unrelated.join("keep.txt")).unwrap(), b"unrelated");
    assert_eq!(
        fs::read(home.join(".codex/session.sqlite")).unwrap(),
        b"session"
    );
    assert_eq!(
        fs::read(home.join(".incodex/runtime/current.json")).unwrap(),
        b"runtime"
    );
}

#[test]
fn remove_fails_closed_on_foreign_shared_entry() {
    let fixture = Fixture::new();
    let foreign = fixture.directories().primary_bundle();
    fs::create_dir_all(&foreign).unwrap();
    fs::write(foreign.join("keep.txt"), b"foreign").unwrap();

    let error = run_remove_at(
        &launch_remove(true, false),
        &fixture.home(),
        &fixture.directories(),
    )
    .unwrap_err();

    assert!(error.contains("unrecognized launcher"), "{error}");
    assert_eq!(fs::read(foreign.join("keep.txt")).unwrap(), b"foreign");
    assert!(!fixture.home().join(".incodex").exists());
}

#[test]
fn self_uninstall_plan_covers_shared_and_legacy_entries_only() {
    let fixture = Fixture::new();
    let current = fixture.owned_current();
    let legacy = fixture.owned_legacy_v2();
    let plan = print_self_uninstall_plan_at(&fixture.home(), &fixture.directories());

    assert_eq!(plan.owned, [current.clone(), legacy.clone()]);
    assert!(plan.unknown.is_empty());
    remove_self_uninstall_entries(&plan).unwrap();
    assert!(!current.exists());
    assert!(!legacy.exists());
}

#[test]
fn self_uninstall_keeps_unknown_entry_and_does_not_create_state_without_owned_entries() {
    let fixture = Fixture::new();
    let unknown = fixture.directories().primary_bundle();
    fs::create_dir_all(&unknown).unwrap();
    fs::write(unknown.join("keep.txt"), b"external app").unwrap();
    let plan = print_self_uninstall_plan_at(&fixture.home(), &fixture.directories());

    assert!(plan.owned.is_empty());
    assert_eq!(plan.unknown.len(), 1);
    remove_self_uninstall_entries(&plan).unwrap();

    assert_eq!(fs::read(unknown.join("keep.txt")).unwrap(), b"external app");
    assert!(!fixture.home().join(".incodex").exists());
}
