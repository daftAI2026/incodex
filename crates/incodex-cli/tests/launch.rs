#![cfg(target_os = "macos")]
//! [INPUT]: 依赖真实CLI与临时假App；不启动系统宿主。
//! [OUTPUT]: 约束launch命令的发布、精确移除、自卸载清理与无效目标行为。
//! [POS]: launch的外部命令合同；隔离测试不替代真实App验收。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "incodex-launch-contract-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_incodex"))
            .args(args)
            .env("HOME", &self.0)
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .output()
            .unwrap()
    }

    fn run_binary(&self, binary: &Path, args: &[&str]) -> Output {
        Command::new(binary)
            .args(args)
            .env("HOME", &self.0)
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .output()
            .unwrap()
    }
    fn app(&self) -> PathBuf {
        let app = self.0.join("Official Host.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::write(app.join("Contents/Info.plist"),
            "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>Host</string></dict></plist>").unwrap();
        let executable = app.join("Contents/MacOS/Host");
        fs::write(
            &executable,
            "#!/bin/sh\nprintf 'must not run' > \"$HOME/host-started\"\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        app
    }

    fn owned_launcher(&self, name: &str, version: u8) -> PathBuf {
        let bundle = self.0.join("Applications").join(name);
        let contents = bundle.join("Contents");
        let macos = contents.join("MacOS");
        let resources = contents.join("Resources");
        fs::create_dir_all(&macos).unwrap();
        fs::create_dir_all(&resources).unwrap();
        let info = match version {
            1 => "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>IncodexLauncher</string><key>CFBundleIdentifier</key><string>com.daftai.incodex.launcher</string><key>CFBundleName</key><string>Incodex Launcher</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>1</string><key>LSUIElement</key><true/></dict></plist>\n",
            2 => "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>IncodexLauncher</string><key>CFBundleIdentifier</key><string>com.daftai.incodex.launcher</string><key>CFBundleName</key><string>Incodex Launcher</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>2</string><key>CFBundleIconFile</key><string>IncodexLauncher.icns</string><key>LSUIElement</key><true/></dict></plist>\n",
            3 => "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>IncodexLauncher</string><key>CFBundleIdentifier</key><string>com.daftai.incodex.launcher</string><key>CFBundleName</key><string>ChatGPT - Inc</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>3</string><key>CFBundleIconFile</key><string>IncodexLauncher.icns</string><key>LSUIElement</key><true/></dict></plist>\n",
            _ => panic!("unsupported launcher fixture generation"),
        };
        fs::write(contents.join("Info.plist"), info).unwrap();
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
        if version >= 2 {
            fs::copy(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/incodex-launcher.icns"),
                resources.join("IncodexLauncher.icns"),
            )
            .unwrap();
        }
        bundle
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
#[test]
fn launch_help_describes_normal_cdp_without_installing() {
    let fixture = Fixture::new();
    let output = fixture.run(&["launch", "--help"]);
    assert!(output.status.success(), "{}", text(&output));
    let help = text(&output);
    assert!(help.contains("normal") && help.contains("CDP"), "{help}");
    assert!(
        help.contains("WindowServer observation for close/minimize"),
        "CDP private open must explain its native close/minimize observer: {help}"
    );
    assert!(
        help.contains("Accessibility permission is not required"),
        "CDP private open must not claim an Accessibility prerequisite: {help}"
    );
    assert!(
        help.contains("before launching Codex"),
        "the Finder entry must be published before host startup: {help}"
    );
    assert!(
        help.contains("remains if launch fails"),
        "the published entry remains available if host startup fails: {help}"
    );
    assert!(!fixture.0.join(".incodex").exists());
}
#[test]
fn launch_dry_run_keeps_official_app_and_home_untouched() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let executable = app.join("Contents/MacOS/Host");
    let before = fs::read(&executable).unwrap();
    let output = fixture.run(&["launch", "--dry-run", "--app", app.to_str().unwrap()]);
    assert!(output.status.success(), "{}", text(&output));
    let plan = text(&output);
    assert!(
        plan.contains("CDP") && plan.contains(app.to_str().unwrap()),
        "{plan}"
    );
    assert_eq!(fs::read(&executable).unwrap(), before);
    assert!(!fixture.0.join("host-started").exists());
    assert!(!fixture.0.join(".incodex").exists());
}
#[test]
fn launch_missing_target_fails_before_creating_state() {
    let fixture = Fixture::new();
    let missing = fixture.0.join("Missing.app");
    let output = fixture.run(&["launch", "--dry-run", "--app", missing.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(
        !text(&output).contains("unknown command: launch"),
        "{}",
        text(&output)
    );
    assert!(!fixture.0.join(".incodex").exists());
    assert!(!Path::new(&missing).exists());
}

#[test]
fn launch_rejects_mutation_and_unimplemented_output_flags() {
    let fixture = Fixture::new();
    for flag in ["--clone", "--live", "--yes", "--json", "--restore-app"] {
        let output = fixture.run(&["launch", "--dry-run", flag]);
        assert!(!output.status.success(), "{flag}: {}", text(&output));
        assert!(
            text(&output).contains("launch accepts only"),
            "{flag}: {}",
            text(&output)
        );
        assert!(!fixture.0.join(".incodex").exists());
    }
}

#[test]
fn launch_remove_dry_run_is_host_independent_and_lists_exact_entries() {
    let fixture = Fixture::new();
    let current = fixture.owned_launcher("ChatGPT - Inc.app", 3);
    let legacy = fixture.owned_launcher("Incodex Launcher.app", 2);

    let output = fixture.run(&["launch", "--remove", "--dry-run"]);

    assert!(output.status.success(), "{}", text(&output));
    let plan = text(&output);
    assert!(plan.contains("ChatGPT - Inc.app"), "{plan}");
    assert!(plan.contains("Incodex Launcher.app"), "{plan}");
    assert!(plan.contains("no changes made"), "{plan}");
    assert!(current.exists(), "dry run removed the current entry");
    assert!(legacy.exists(), "dry run removed the legacy entry");
    assert!(!fixture.0.join(".incodex").exists());
    assert!(!fixture.0.join(".codex").exists());
}

#[test]
fn launch_remove_requires_yes_and_removes_only_owned_entries() {
    let fixture = Fixture::new();
    let current = fixture.owned_launcher("ChatGPT - Inc.app", 3);
    let legacy = fixture.owned_launcher("Incodex Launcher.app", 1);
    let unrelated = fixture.0.join("Applications/Unrelated.app");
    fs::create_dir_all(&unrelated).unwrap();
    fs::write(unrelated.join("owner.txt"), b"foreign").unwrap();
    let host = fixture.app();
    let host_executable = host.join("Contents/MacOS/Host");
    let host_before = fs::read(&host_executable).unwrap();
    fs::create_dir_all(fixture.0.join(".codex")).unwrap();
    fs::write(fixture.0.join(".codex/session.sqlite"), b"keep session").unwrap();
    fs::create_dir_all(fixture.0.join(".incodex/runtime")).unwrap();
    fs::write(
        fixture.0.join(".incodex/runtime/current.json"),
        b"keep runtime",
    )
    .unwrap();

    let refused = fixture.run(&["launch", "--remove"]);
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        text(&refused).contains("requires --yes"),
        "{}",
        text(&refused)
    );
    assert!(current.exists());
    assert!(legacy.exists());

    let removed = fixture.run(&["launch", "--remove", "--yes"]);
    assert!(removed.status.success(), "{}", text(&removed));
    assert!(!current.exists(), "current own entry remains");
    assert!(!legacy.exists(), "exact legacy entry remains");
    assert_eq!(fs::read(unrelated.join("owner.txt")).unwrap(), b"foreign");
    assert_eq!(fs::read(&host_executable).unwrap(), host_before);
    assert_eq!(
        fs::read(fixture.0.join(".codex/session.sqlite")).unwrap(),
        b"keep session"
    );
    assert_eq!(
        fs::read(fixture.0.join(".incodex/runtime/current.json")).unwrap(),
        b"keep runtime"
    );
}

#[test]
fn launch_remove_fails_closed_on_a_foreign_same_name_bundle() {
    let fixture = Fixture::new();
    let foreign = fixture.0.join("Applications/ChatGPT - Inc.app");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(foreign.join("do-not-touch.txt"), b"foreign owner").unwrap();

    let output = fixture.run(&["launch", "--remove", "--yes"]);

    assert!(!output.status.success(), "{}", text(&output));
    assert_eq!(
        fs::read(foreign.join("do-not-touch.txt")).unwrap(),
        b"foreign owner"
    );
    assert!(!fixture.0.join(".incodex").exists());
}

#[test]
fn launch_remove_rejects_an_app_override() {
    let fixture = Fixture::new();
    let app = fixture.app();

    let output = fixture.run(&[
        "launch",
        "--remove",
        "--dry-run",
        "--app",
        app.to_str().unwrap(),
    ]);

    assert!(!output.status.success(), "{}", text(&output));
    assert!(
        text(&output).contains("launch --remove does not accept --app"),
        "{}",
        text(&output)
    );
    assert!(!fixture.0.join("host-started").exists());
}

#[test]
fn launch_remove_is_not_a_global_removal_flag() {
    let fixture = Fixture::new();

    let output = fixture.run(&["open", "--remove", "--dry-run"]);

    assert!(!output.status.success(), "{}", text(&output));
    assert!(
        text(&output).contains("--remove is only valid for launch"),
        "{}",
        text(&output)
    );
    assert!(!fixture.0.join(".incodex").exists());
}

#[test]
fn script_self_uninstall_removes_owned_finder_entries_with_the_cli() {
    let fixture = Fixture::new();
    let current = fixture.owned_launcher("ChatGPT - Inc.app", 3);
    let legacy = fixture.owned_launcher("Incodex Launcher.app", 2);
    let bin = fixture.0.join("prefix/bin");
    fs::create_dir_all(&bin).unwrap();
    let installed = bin.join("incodex");
    let alias = bin.join("inc");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &alias).unwrap();
    fs::create_dir_all(fixture.0.join(".codex")).unwrap();
    fs::write(fixture.0.join(".codex/session.sqlite"), b"keep session").unwrap();
    fs::create_dir_all(fixture.0.join(".incodex/runtime")).unwrap();
    fs::write(
        fixture.0.join(".incodex/runtime/current.json"),
        b"keep runtime",
    )
    .unwrap();

    let plan = fixture.run_binary(&installed, &["self-uninstall", "--dry-run"]);
    assert!(plan.status.success(), "{}", text(&plan));
    let plan_text = text(&plan);
    assert!(plan_text.contains("ChatGPT - Inc.app"), "{plan_text}");
    assert!(plan_text.contains("Incodex Launcher.app"), "{plan_text}");
    assert!(current.exists());
    assert!(legacy.exists());

    let removed = fixture.run_binary(&installed, &["self-uninstall", "--yes"]);
    assert!(removed.status.success(), "{}", text(&removed));
    assert!(
        !current.exists(),
        "self-uninstall left the current Finder entry"
    );
    assert!(
        !legacy.exists(),
        "self-uninstall left the legacy Finder entry"
    );
    assert!(!installed.exists());
    assert!(!alias.exists());
    assert_eq!(
        fs::read(fixture.0.join(".codex/session.sqlite")).unwrap(),
        b"keep session"
    );
    assert_eq!(
        fs::read(fixture.0.join(".incodex/runtime/current.json")).unwrap(),
        b"keep runtime"
    );
}

#[test]
fn script_self_uninstall_without_an_owned_launcher_does_not_create_state() {
    let fixture = Fixture::new();
    let bin = fixture.0.join("prefix/bin");
    fs::create_dir_all(&bin).unwrap();
    let installed = bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    let output = fixture.run_binary(&installed, &["self-uninstall", "--yes"]);
    assert!(output.status.success(), "{}", text(&output));
    assert!(!installed.exists());
    assert!(!fixture.0.join(".incodex").exists(), "CLI removal must not create a launcher lock tree when no launcher exists");
}

#[test]
fn script_self_uninstall_warns_and_preserves_an_external_same_name_app() {
    let fixture = Fixture::new();
    let foreign = fixture.0.join("Applications/ChatGPT - Inc.app");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(foreign.join("keep.txt"), b"external app").unwrap();
    let bin = fixture.0.join("prefix/bin");
    fs::create_dir_all(&bin).unwrap();
    let installed = bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    let output = fixture.run_binary(&installed, &["self-uninstall", "--yes"]);
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("Leaving unrecognized launcher"));
    assert!(!installed.exists());
    assert_eq!(fs::read(foreign.join("keep.txt")).unwrap(), b"external app");
    assert!(!fixture.0.join(".incodex").exists());
}
