//! [INPUT]: 依赖用户 HOME、已解析 CLI 路径、稳定安装入口与原始宿主 App 路径。
//! [OUTPUT]: 启动宿主前验证稳定CLI并原子发布短命Finder入口。
//! [POS]: 拒绝外部同名bundle；不复制宿主，入口运行同一CLI。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

const BUNDLE_ID: &str = "com.daftai.incodex.launcher";
const EXECUTABLE_NAME: &str = "IncodexLauncher";
const OWNER_MARKER: &str = ".incodex-launcher-owner";
const OWNER_MARKER_CONTENT: &[u8] = b"incodex-launcher-owner-v1\n";
const INFO_PLIST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>IncodexLauncher</string><key>CFBundleIdentifier</key><string>com.daftai.incodex.launcher</string><key>CFBundleName</key><string>Incodex Launcher</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>1</string><key>LSUIElement</key><true/></dict></plist>\n";
const SHELL_HEADER: &str = "#!/bin/sh\n";
const DETACHED_MARKER: &str = "# incodex-launcher-detached-v1";

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn launcher_path(home: &Path) -> PathBuf {
    home.join("Applications").join("Incodex Launcher.app")
}

pub(crate) fn publish_launcher(home: &Path, cli: &Path, app: &Path) -> Result<PathBuf, String> {
    let resolved_home = resolve_directory(home, "user HOME")?;
    let stable_cli = crate::macos_launch_cli::stable_entry(cli, &resolved_home)?;
    resolve_executable(&stable_cli)?;
    let resolved_app = resolve_directory(app, "official App")?;
    let script = launcher_script(&stable_cli, &resolved_app)?;
    let applications = ensure_applications_directory(&resolved_home)?;
    let destination = launcher_path(&resolved_home);
    let previous_exists = inspect_destination(&destination)?;
    let staging = create_staging_directory(&applications)?;

    if let Err(error) = populate_bundle(&staging, script.as_bytes()) {
        remove_created_path(&staging);
        return Err(error);
    }
    if let Err(error) = verify_owned_bundle(&staging) {
        remove_created_path(&staging);
        return Err(error);
    }

    let current_exists = match inspect_destination(&destination) {
        Ok(exists) => exists,
        Err(error) => {
            remove_created_path(&staging);
            return Err(error);
        }
    };
    if current_exists != previous_exists {
        remove_created_path(&staging);
        return Err("launcher destination changed during publication".into());
    }

    if previous_exists {
        let backup = match unique_sibling(&applications, "previous") {
            Ok(path) => path,
            Err(error) => {
                remove_created_path(&staging);
                return Err(error);
            }
        };
        if let Err(error) = fs::rename(&destination, &backup) {
            remove_created_path(&staging);
            return Err(format!(
                "cannot retain the existing Incodex Launcher before update: {error}"
            ));
        }
        if let Err(error) = fs::rename(&staging, &destination) {
            return Err(restore_previous_bundle(
                &destination,
                &backup,
                &staging,
                error,
            ));
        }
        remove_owned_bundle(&backup);
    } else if let Err(error) = fs::rename(&staging, &destination) {
        remove_created_path(&staging);
        return Err(format!("cannot publish the Incodex Launcher: {error}"));
    }

    Ok(launcher_path(home))
}

fn resolve_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    let resolved = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve {label} {}: {error}", path.display()))?;
    let metadata = fs::symlink_metadata(&resolved)
        .map_err(|error| format!("cannot inspect {label} {}: {error}", resolved.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "{label} is not a real directory: {}",
            resolved.display()
        ));
    }
    Ok(resolved)
}

fn resolve_executable(path: &Path) -> Result<PathBuf, String> {
    let resolved = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve Incodex CLI {}: {error}", path.display()))?;
    let metadata = fs::symlink_metadata(&resolved)
        .map_err(|error| format!("cannot inspect Incodex CLI {}: {error}", resolved.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "Incodex CLI is not a regular file: {}",
            resolved.display()
        ));
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(format!(
            "Incodex CLI is not executable: {}",
            resolved.display()
        ));
    }
    Ok(resolved)
}

fn launcher_script(cli: &Path, app: &Path) -> Result<String, String> {
    let cli = shell_quote(cli)?;
    let app = shell_quote(app)?;
    Ok(format!("{SHELL_HEADER}{DETACHED_MARKER}\n/usr/bin/nohup {cli} launch --app {app} </dev/null >/dev/null 2>&1 &\n"))
}

fn shell_quote(path: &Path) -> Result<String, String> {
    let value = path
        .to_str()
        .ok_or_else(|| format!("launcher path is not valid UTF-8: {}", path.display()))?;
    if value.contains(['\n', '\r']) {
        return Err(format!("launcher path contains a line break: {value:?}"));
    }
    Ok(format!("'{}'", value.replace('\'', "'\\''")))
}

fn ensure_applications_directory(home: &Path) -> Result<PathBuf, String> {
    let applications = home.join("Applications");
    match fs::symlink_metadata(&applications) {
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => {}
        Ok(_) => {
            return Err(format!(
                "user Applications path is not a real directory: {}",
                applications.display()
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(&applications).map_err(|error| {
                format!(
                    "cannot create user Applications directory {}: {error}",
                    applications.display()
                )
            })?;
        }
        Err(error) => {
            return Err(format!(
                "cannot inspect user Applications directory {}: {error}",
                applications.display()
            ));
        }
    }
    let metadata = fs::symlink_metadata(&applications).map_err(|error| {
        format!(
            "cannot verify user Applications directory {}: {error}",
            applications.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "user Applications path changed during publication: {}",
            applications.display()
        ));
    }
    Ok(applications)
}

fn inspect_destination(destination: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "refusing symlink at launcher destination: {}",
            destination.display()
        )),
        Ok(metadata) if !metadata.is_dir() => Err(format!(
            "refusing non-bundle at launcher destination: {}",
            destination.display()
        )),
        Ok(_) => {
            verify_owned_bundle(destination)?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "cannot inspect launcher destination {}: {error}",
            destination.display()
        )),
    }
}

fn create_staging_directory(applications: &Path) -> Result<PathBuf, String> {
    for _ in 0..32 {
        let path = unique_sibling(applications, "staging")?;
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "cannot create private launcher staging directory {}: {error}",
                    path.display()
                ));
            }
        }
    }
    Err("cannot allocate a unique launcher staging directory".into())
}

fn unique_sibling(applications: &Path, role: &str) -> Result<PathBuf, String> {
    for _ in 0..32 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = applications.join(format!(
            ".incodex-launcher-{role}-{}-{sequence}",
            process::id()
        ));
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(path),
            Err(error) => {
                return Err(format!(
                    "cannot inspect launcher transaction path {}: {error}",
                    path.display()
                ));
            }
            Ok(_) => {}
        }
    }
    Err(format!("cannot allocate a unique launcher {role} path"))
}

fn populate_bundle(staging: &Path, script: &[u8]) -> Result<(), String> {
    let contents = staging.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    fs::create_dir(&contents)
        .map_err(|error| format!("cannot create launcher Contents: {error}"))?;
    fs::create_dir(&macos)
        .map_err(|error| format!("cannot create launcher MacOS directory: {error}"))?;
    fs::create_dir(&resources)
        .map_err(|error| format!("cannot create launcher Resources directory: {error}"))?;
    write_new_file(&contents.join("Info.plist"), INFO_PLIST.as_bytes(), 0o644)?;
    write_new_file(&macos.join(EXECUTABLE_NAME), script, 0o755)?;
    write_new_file(&resources.join(OWNER_MARKER), OWNER_MARKER_CONTENT, 0o600)?;
    Ok(())
}

fn write_new_file(path: &Path, contents: &[u8], mode: u32) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("cannot create launcher file {}: {error}", path.display()))?;
    file.write_all(contents)
        .map_err(|error| format!("cannot write launcher file {}: {error}", path.display()))?;
    file.sync_all()
        .map_err(|error| format!("cannot flush launcher file {}: {error}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| format!("cannot set launcher file mode {}: {error}", path.display()))?;
    Ok(())
}

fn verify_owned_bundle(bundle: &Path) -> Result<(), String> {
    require_directory(bundle, "launcher bundle")?;
    require_entries(bundle, &["Contents"])?;
    let contents = bundle.join("Contents");
    require_directory(&contents, "launcher Contents")?;
    require_entries(&contents, &["Info.plist", "MacOS", "Resources"])?;

    let info = contents.join("Info.plist");
    require_regular_file(&info, "launcher Info.plist")?;
    let info_bytes = fs::read(&info).map_err(|error| {
        format!(
            "cannot read launcher Info.plist {}: {error}",
            info.display()
        )
    })?;
    if info_bytes != INFO_PLIST.as_bytes() || !INFO_PLIST.contains(BUNDLE_ID) {
        return Err(format!(
            "launcher Info.plist is not Incodex-owned: {}",
            info.display()
        ));
    }

    let macos = contents.join("MacOS");
    require_directory(&macos, "launcher executable directory")?;
    require_entries(&macos, &[EXECUTABLE_NAME])?;
    let executable = macos.join(EXECUTABLE_NAME);
    require_regular_file(&executable, "launcher executable")?;
    let script = fs::read_to_string(&executable).map_err(|error| {
        format!(
            "cannot read launcher executable {}: {error}",
            executable.display()
        )
    })?;
    if !is_generated_launcher_script(&script) {
        return Err(format!(
            "launcher executable is not Incodex-owned: {}",
            executable.display()
        ));
    }
    if fs::metadata(&executable)
        .map_err(|error| format!("cannot inspect launcher executable: {error}"))?
        .permissions()
        .mode()
        & 0o111
        == 0
    {
        return Err(format!(
            "launcher executable lost its execute bit: {}",
            executable.display()
        ));
    }

    let resources = contents.join("Resources");
    require_directory(&resources, "launcher Resources")?;
    require_entries(&resources, &[OWNER_MARKER])?;
    let marker = resources.join(OWNER_MARKER);
    require_regular_file(&marker, "launcher ownership marker")?;
    let marker_bytes = fs::read(&marker)
        .map_err(|error| format!("cannot read launcher ownership marker: {error}"))?;
    if marker_bytes != OWNER_MARKER_CONTENT {
        return Err(format!(
            "launcher ownership marker is invalid: {}",
            marker.display()
        ));
    }
    Ok(())
}

fn require_directory(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {label} {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "{label} is not a real directory: {}",
            path.display()
        ));
    }
    Ok(())
}

fn require_regular_file(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {label} {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("{label} is not a regular file: {}", path.display()));
    }
    Ok(())
}

fn require_entries(directory: &Path, expected: &[&str]) -> Result<(), String> {
    let mut actual = fs::read_dir(directory)
        .map_err(|error| {
            format!(
                "cannot list launcher directory {}: {error}",
                directory.display()
            )
        })?
        .map(|entry| {
            entry
                .map_err(|error| format!("cannot read launcher directory entry: {error}"))?
                .file_name()
                .into_string()
                .map_err(|_| {
                    format!(
                        "launcher directory has a non-UTF-8 entry: {}",
                        directory.display()
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut expected = expected
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    actual.sort();
    expected.sort();
    if actual != expected {
        return Err(format!(
            "launcher directory contents changed: {}",
            directory.display()
        ));
    }
    Ok(())
}

fn is_generated_launcher_script(script: &str) -> bool {
    let mut lines = script.lines();
    if lines.next() != Some(SHELL_HEADER.trim_end()) {
        return false;
    }
    let Some(mut command) = lines.next() else {
        return false;
    };
    if command == DETACHED_MARKER {
        let Some(detached) = lines.next() else {
            return false;
        };
        command = detached;
        return lines.next().is_none()
            && command.starts_with("/usr/bin/nohup '")
            && command.contains("' launch --app '")
            && command.ends_with("' </dev/null >/dev/null 2>&1 &");
    }
    lines.next().is_none()
        && command.starts_with("exec '")
        && command.contains("' launch --app '")
        && command.ends_with('\'')
}

fn restore_previous_bundle(
    destination: &Path,
    backup: &Path,
    staging: &Path,
    publish_error: io::Error,
) -> String {
    remove_created_path(staging);
    match fs::rename(backup, destination) {
        Ok(()) => format!(
            "cannot publish the Incodex Launcher; previous bundle restored: {publish_error}"
        ),
        Err(restore_error) => format!(
            "cannot publish the Incodex Launcher: {publish_error}; previous bundle remains at {}: {restore_error}",
            backup.display()
        ),
    }
}

fn remove_owned_bundle(bundle: &Path) {
    if verify_owned_bundle(bundle).is_ok() {
        let _ = fs::remove_dir_all(bundle);
    }
}

fn remove_created_path(path: &Path) {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            let _ = fs::remove_file(path);
        }
        Ok(_) => {
            let _ = fs::remove_dir_all(path);
        }
        Err(_) => {}
    }
}

#[cfg(test)]
mod tests {
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
            home.join("Applications").join("Incodex Launcher.app")
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
            fs::read_to_string(launcher_path(&home).join("Contents/MacOS/IncodexLauncher"))
                .unwrap();
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
}
