//! [INPUT]: 依赖 macos_launch_icon 的发布、校验与旧版升级合同。
//! [OUTPUT]: 验证 Finder 图标、ICNS 完整性、精确旧版升级和拒绝边界。
//! [POS]: macOS launcher 的无宿主文件系统回归；不调用 LaunchServices。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use super::{launcher_path, publish_launcher};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "incodex-launch-icon-{}-{} 'quoted'",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create isolated fixture root");
        Self(path)
    }

    fn home(&self) -> PathBuf {
        let home = self.0.join("user HOME 'with spaces'");
        fs::create_dir_all(&home).expect("create isolated user HOME");
        home
    }

    fn app(&self) -> PathBuf {
        let app = self.0.join("Official 'Codex' Host.app");
        fs::create_dir_all(app.join("Contents/MacOS")).expect("create source app fixture");
        fs::write(
            app.join("Contents/MacOS/Host"),
            b"official app must not be copied",
        )
        .expect("write source app sentinel");
        app
    }

    fn cli(&self, version: &str) -> PathBuf {
        let cli = self.0.join(format!("fake CLI '{version}'"));
        fs::write(
                &cli,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$INCODEX_LAUNCH_ARGS\"\nprintf '%s' \"$HOME\" > \"$INCODEX_LAUNCH_HOME\"\nprintf '%s' '{version}' > \"$INCODEX_LAUNCH_VERSION\"\n"
                ),
            )
            .expect("write fake CLI");
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o755))
            .expect("make fake CLI executable");
        cli
    }

    fn run_launcher(&self, home: &Path) -> (Vec<String>, String, String) {
        let launcher = launcher_path(home).join("Contents/MacOS/IncodexLauncher");
        let args_log = self.0.join("launch args.txt");
        let home_log = self.0.join("launch HOME.txt");
        let version_log = self.0.join("launch version.txt");
        for path in [&args_log, &home_log, &version_log] {
            let _ = fs::remove_file(path);
        }
        let output = Command::new(&launcher)
            .env("HOME", home)
            .env("INCODEX_LAUNCH_ARGS", &args_log)
            .env("INCODEX_LAUNCH_HOME", &home_log)
            .env("INCODEX_LAUNCH_VERSION", &version_log)
            .output()
            .expect("run generated launcher against fake CLI");
        assert!(
            output.status.success(),
            "launcher failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while fs::metadata(&version_log).map_or(true, |m| m.len() == 0)
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let args = fs::read_to_string(args_log)
            .expect("read fake CLI args")
            .lines()
            .map(str::to_owned)
            .collect();
        (
            args,
            fs::read_to_string(home_log).expect("read inherited HOME"),
            fs::read_to_string(version_log).expect("read fake CLI generation"),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn launcher_path_is_the_user_applications_bundle() {
    let home = Path::new("/tmp/test-home");
    assert_eq!(
        launcher_path(home),
        home.join("Applications").join("ChatGPT - Inc.app")
    );
}

#[test]
fn generated_app_runs_only_launch_with_exact_paths_and_inherited_home() {
    let fixture = Fixture::new();
    let home = fixture.home();
    let cli = fixture.cli("one");
    let app = fixture.app();
    let published = publish_launcher(&home, &cli, &app).expect("publish own launcher");
    let resolved_app = fs::canonicalize(&app).unwrap();

    assert_eq!(published, launcher_path(&home));
    let (args, inherited_home, version) = fixture.run_launcher(&home);
    assert_eq!(args, ["launch", "--app", resolved_app.to_str().unwrap()]);
    assert_eq!(inherited_home, home.to_str().unwrap());
    assert_eq!(version, "one");
    assert!(published.join("Contents/Info.plist").is_file());
    let plist = fs::read_to_string(published.join("Contents/Info.plist")).unwrap();
    assert!(plist.contains("com.daftai.incodex.launcher"));
    assert!(plist.contains("<key>CFBundleName</key><string>ChatGPT - Inc</string>"));
    assert!(plist.contains("<key>LSUIElement</key><true/>"));
    assert!(!home.join(".codex").exists());
    assert!(!home.join(".incodex").exists());
    assert!(!published.join("Contents/MacOS/Host").exists());
}

#[test]
fn finder_entry_exits_while_the_same_cli_remains_app_scoped() {
    let fixture = Fixture::new();
    let home = fixture.home();
    let cli = fixture.cli("background");
    fs::write(&cli, "#!/bin/sh\nprintf started > \"$INCODEX_LAUNCH_STARTED\"\nwhile [ ! -e \"$INCODEX_LAUNCH_RELEASE\" ]; do sleep 0.02; done\nprintf finished > \"$INCODEX_LAUNCH_FINISHED\"\n").unwrap();
    let app = fixture.app();
    let published = publish_launcher(&home, &cli, &app).unwrap();
    let started = fixture.0.join("started");
    let release = fixture.0.join("release");
    let finished = fixture.0.join("finished");
    let mut entry = Command::new(published.join("Contents/MacOS/IncodexLauncher"))
        .env("HOME", &home)
        .env("INCODEX_LAUNCH_STARTED", &started)
        .env("INCODEX_LAUNCH_RELEASE", &release)
        .env("INCODEX_LAUNCH_FINISHED", &finished)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    let returned = loop {
        if let Some(status) = entry.try_wait().unwrap() {
            break Some(status);
        }
        if std::time::Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !started.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let owned_cli_is_running = started.exists() && !finished.exists();
    // 先释放自有子进程，再断言；红测也不能遗留后台夹具。
    fs::write(&release, b"release").unwrap();
    if returned.is_none() {
        entry.wait().unwrap();
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !finished.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(finished.exists(), "fixture CLI did not finish");
    assert!(
        owned_cli_is_running,
        "entry did not retain its app-scoped CLI"
    );
    assert!(
        returned.is_some_and(|status| status.success()),
        "Finder entry stays alive and cannot accept a second launch"
    );
}

#[test]
fn foreign_same_name_app_is_never_modified() {
    let fixture = Fixture::new();
    let home = fixture.home();
    let cli = fixture.cli("one");
    let app = fixture.app();
    let destination = launcher_path(&home);
    fs::create_dir_all(&destination).expect("create foreign app");
    let sentinel = destination.join("do-not-touch.txt");
    fs::write(&sentinel, b"foreign owner").expect("write foreign sentinel");

    assert!(publish_launcher(&home, &cli, &app).is_err());
    assert_eq!(fs::read(&sentinel).unwrap(), b"foreign owner");
    assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
}

#[test]
fn symlink_at_launcher_path_is_rejected_without_touching_its_target() {
    let fixture = Fixture::new();
    let home = fixture.home();
    let cli = fixture.cli("one");
    let app = fixture.app();
    let outside = fixture.0.join("external app");
    fs::create_dir(&outside).expect("create external directory");
    let sentinel = outside.join("sentinel");
    fs::write(&sentinel, b"external owner").expect("write external sentinel");
    let destination = launcher_path(&home);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    symlink(&outside, &destination).expect("link destination to external app");

    assert!(publish_launcher(&home, &cli, &app).is_err());
    assert!(fs::symlink_metadata(&destination)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read(&sentinel).unwrap(), b"external owner");
}

#[test]
fn own_bundle_update_preserves_old_launcher_on_preflight_failure() {
    let fixture = Fixture::new();
    let home = fixture.home();
    let cli_one = fixture.cli("one");
    let cli_two = fixture.cli("two");
    let app = fixture.app();
    let destination = publish_launcher(&home, &cli_one, &app).expect("publish first launcher");
    let launcher = destination.join("Contents/MacOS/IncodexLauncher");
    let first_script = fs::read(&launcher).expect("read first launcher");
    let missing_cli = fixture.0.join("missing CLI");

    assert!(publish_launcher(&home, &missing_cli, &app).is_err());
    assert_eq!(fs::read(&launcher).unwrap(), first_script);

    publish_launcher(&home, &cli_two, &app).expect("publish replacement launcher");
    let (args, inherited_home, version) = fixture.run_launcher(&home);
    assert_eq!(
        args,
        [
            "launch",
            "--app",
            fs::canonicalize(app).unwrap().to_str().unwrap()
        ]
    );
    assert_eq!(inherited_home, home.to_str().unwrap());
    assert_eq!(version, "two");
    assert_eq!(
        fs::read_dir(destination.parent().unwrap()).unwrap().count(),
        1
    );
}

#[test]
fn launcher_keeps_stable_cli_path_across_cellar_generation_switch() {
    let fixture = Fixture::new();
    let home = fixture.home();
    let cli_one = fixture.cli("one");
    let cli_two = fixture.cli("two");
    let app = fixture.app();
    let stable_directory = fs::canonicalize(&home).unwrap().join(".local/bin");
    fs::create_dir_all(&stable_directory).unwrap();
    let stable_cli = stable_directory.join("incodex");
    symlink(&cli_one, &stable_cli).unwrap();

    publish_launcher(&home, &cli_one, &app).expect("publish launcher from active CLI");
    let script =
        fs::read_to_string(launcher_path(&home).join("Contents/MacOS/IncodexLauncher")).unwrap();
    assert!(
        script.contains(&super::shell_quote(&stable_cli).unwrap()),
        "{script}"
    );

    fs::remove_file(&stable_cli).unwrap();
    symlink(&cli_two, &stable_cli).unwrap();
    let (args, inherited_home, version) = fixture.run_launcher(&home);

    assert_eq!(
        args,
        [
            "launch",
            "--app",
            fs::canonicalize(app).unwrap().to_str().unwrap()
        ]
    );
    assert_eq!(inherited_home, home.to_str().unwrap());
    assert_eq!(version, "two");
}

// 测试旧版 bundle 的精确 Info.plist。只有完整匹配旧合同才能迁移。
const LEGACY_V1_INFO_PLIST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>IncodexLauncher</string><key>CFBundleIdentifier</key><string>com.daftai.incodex.launcher</string><key>CFBundleName</key><string>Incodex Launcher</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>1</string><key>LSUIElement</key><true/></dict></plist>\n";
const LEGACY_V2_INFO_PLIST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>IncodexLauncher</string><key>CFBundleIdentifier</key><string>com.daftai.incodex.launcher</string><key>CFBundleName</key><string>Incodex Launcher</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>2</string><key>CFBundleIconFile</key><string>IncodexLauncher.icns</string><key>LSUIElement</key><true/></dict></plist>\n";

fn legacy_launcher_path(home: &Path) -> PathBuf {
    home.join("Applications").join("Incodex Launcher.app")
}

fn create_legacy_bundle(home: &Path, cli: &Path, app: &Path) -> PathBuf {
    let bundle = legacy_launcher_path(home);
    let contents = bundle.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    fs::create_dir_all(&macos).unwrap();
    fs::create_dir(&resources).unwrap();
    fs::write(contents.join("Info.plist"), LEGACY_V1_INFO_PLIST).unwrap();
    let script = super::launcher_script(
        &fs::canonicalize(cli).unwrap(),
        &fs::canonicalize(app).unwrap(),
    )
    .unwrap();
    let executable = macos.join(super::EXECUTABLE_NAME);
    fs::write(&executable, script).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        resources.join(super::OWNER_MARKER),
        super::OWNER_MARKER_CONTENT,
    )
    .unwrap();
    bundle
}

fn create_legacy_v2_bundle(home: &Path, cli: &Path, app: &Path) -> PathBuf {
    let bundle = create_legacy_bundle(home, cli, app);
    let contents = bundle.join("Contents");
    fs::write(contents.join("Info.plist"), LEGACY_V2_INFO_PLIST).unwrap();
    fs::write(
        contents.join("Resources/IncodexLauncher.icns"),
        super::ICON_BYTES,
    )
    .unwrap();
    bundle
}

fn assert_published_icon(bundle: &Path) {
    let contents = bundle.join("Contents");
    let plist = fs::read_to_string(contents.join("Info.plist")).unwrap();
    assert!(
        plist.contains("<key>CFBundleIconFile</key><string>IncodexLauncher.icns</string>"),
        "launcher Info.plist must declare its Incodex icon"
    );
    assert!(
        plist.contains("<key>CFBundleVersion</key><string>3</string>"),
        "renaming the launcher must advance its bundle version"
    );
    let icon_path = contents.join("Resources/IncodexLauncher.icns");
    let metadata = fs::symlink_metadata(&icon_path).expect("launcher icon must exist");
    assert!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "launcher icon must be a regular file"
    );
    let bytes = fs::read(icon_path).unwrap();
    assert!(
        bytes.len() >= 8,
        "launcher icon must contain an ICNS header"
    );
    assert_eq!(&bytes[..4], b"icns", "launcher icon must be ICNS data");
    let declared = u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize;
    assert_eq!(declared, bytes.len(), "ICNS length must match the resource");
}

#[test]
fn published_launcher_declares_a_real_regular_icns_resource() {
    let fixture = Fixture::new();
    let home = fixture.home();
    let cli = fixture.cli("one");
    let app = fixture.app();
    let published = publish_launcher(&home, &cli, &app).expect("publish launcher");

    assert_published_icon(&published);
}

#[test]
fn exact_v1_owned_launcher_migrates_atomically_to_the_new_name() {
    let fixture = Fixture::new();
    let home = fixture.home();
    let cli_one = fixture.cli("one");
    let cli_two = fixture.cli("two");
    let app = fixture.app();
    let old = create_legacy_bundle(&home, &cli_one, &app);
    assert!(super::verify_owned_bundle(&old).is_err());

    let published =
        publish_launcher(&home, &cli_two, &app).expect("migrate exact V1 Incodex launcher");
    assert_eq!(published, launcher_path(&home));
    assert!(
        !old.exists(),
        "exact legacy V1 entry must be retired after publish"
    );
    assert_published_icon(&published);
    let (_, inherited_home, version) = fixture.run_launcher(&home);
    assert_eq!(inherited_home, home.to_str().unwrap());
    assert_eq!(version, "two");
    assert_eq!(
        fs::read_dir(published.parent().unwrap()).unwrap().count(),
        1,
        "migration must not leave a second Finder entry"
    );
}

#[test]
fn exact_v2_owned_launcher_migrates_without_copying_the_old_bundle() {
    let fixture = Fixture::new();
    let home = fixture.home();
    let cli = fixture.cli("two");
    let app = fixture.app();
    let old = create_legacy_v2_bundle(&home, &cli, &app);
    assert!(super::verify_replaceable_bundle(&old).is_ok());

    let published = publish_launcher(&home, &cli, &app).expect("migrate exact V2 launcher");

    assert_eq!(published, launcher_path(&home));
    assert!(
        !old.exists(),
        "exact legacy V2 entry must be retired after publish"
    );
    assert_published_icon(&published);
    assert_eq!(
        fs::read_dir(published.parent().unwrap()).unwrap().count(),
        1
    );
}

#[test]
fn icon_verification_rejects_missing_modified_and_symlink_resources() {
    let fixture = Fixture::new();
    let home = fixture.home();
    let cli = fixture.cli("one");
    let app = fixture.app();
    let published = publish_launcher(&home, &cli, &app).expect("publish launcher");
    let icon = published.join("Contents/Resources/IncodexLauncher.icns");
    let original = fs::read(&icon).expect("published icon exists");

    fs::remove_file(&icon).unwrap();
    assert!(super::verify_owned_bundle(&published).is_err());
    fs::write(&icon, &original).unwrap();

    let mut modified = original.clone();
    let last = modified.len() - 1;
    modified[last] ^= 1;
    fs::write(&icon, modified).unwrap();
    assert!(super::verify_owned_bundle(&published).is_err());
    fs::write(&icon, &original).unwrap();

    let external = fixture.0.join("external icon.icns");
    fs::write(&external, &original).unwrap();
    fs::remove_file(&icon).unwrap();
    symlink(&external, &icon).unwrap();
    assert!(super::verify_owned_bundle(&published).is_err());
    assert_eq!(fs::read(&external).unwrap(), original);
    assert!(fs::symlink_metadata(&icon)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(publish_launcher(&home, &cli, &app).is_err());
    assert!(fs::symlink_metadata(&icon)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn legacy_bundle_with_extra_icon_entry_is_not_eligible_for_upgrade() {
    let fixture = Fixture::new();
    let home = fixture.home();
    let cli = fixture.cli("one");
    let app = fixture.app();
    let bundle = create_legacy_bundle(&home, &cli, &app);
    let extra = bundle.join("Contents/Resources/IncodexLauncher.icns");
    fs::write(&extra, b"not an owned legacy bundle").unwrap();

    assert!(publish_launcher(&home, &cli, &app).is_err());
    assert_eq!(fs::read(&extra).unwrap(), b"not an owned legacy bundle");
    assert!(extra.exists());
}
