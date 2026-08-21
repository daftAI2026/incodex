use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_incodex")
}

static HOME_SEQ: AtomicU64 = AtomicU64::new(0);

fn isolated_home() -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let seq = HOME_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("incodex-open-{}-{n}-{seq}", std::process::id()));
    fs::create_dir_all(&dir).expect("home");
    dir
}

fn run(args: &[&str], home: &Path) -> (i32, String, String) {
    let output = Command::new(bin())
        .args(args)
        .env("HOME", home)
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .env("SHELL", "/bin/zsh")
        .output()
        .expect("spawn incodex");
    (
        output.status.code().unwrap_or(1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn fake_app(root: &Path, script: &str) -> PathBuf {
    fake_app_with_executable(root, "ChatGPT", script)
}

fn fake_app_with_executable(root: &Path, executable: &str, script: &str) -> PathBuf {
    let app = root.join("ChatGPT.app");
    let mac = app.join("Contents").join("MacOS");
    fs::create_dir_all(&mac).unwrap();
    fs::write(
        app.join("Contents/Info.plist"),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>{executable}</string></dict></plist>\n"
        ),
    )
    .unwrap();
    let exe = mac.join(executable);
    fs::write(&exe, script).unwrap();
    let mut perms = fs::metadata(&exe).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&exe, perms).unwrap();
    app
}

fn open_process(app: &Path, home: &Path) -> Child {
    Command::new(bin())
        .args(["open", "--app", app.to_str().unwrap()])
        .env("HOME", home)
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .env("SHELL", "/bin/zsh")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn incodex open")
}

fn started_session(home: &Path, id: &str) -> PathBuf {
    let marker = home.join(format!("started-{id}"));
    for _ in 0..1_000 {
        if let Ok(body) = fs::read_to_string(&marker) {
            if let Some(root) = body
                .lines()
                .find_map(|line| line.strip_prefix("root=").map(PathBuf::from))
            {
                return root;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for complete session marker {}",
        marker.display()
    );
}

fn wait_for_exit(mut child: Child) -> Output {
    for _ in 0..1_000 {
        if child.try_wait().expect("poll open process").is_some() {
            return child.wait_with_output().expect("collect open output");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    child.kill().expect("kill hung open process");
    panic!("timed out waiting for open process");
}

fn incodex_paths(home: &Path) -> Vec<String> {
    let dir = home.join(".incodex");
    if !dir.exists() {
        return Vec::new();
    }
    let mut out = Vec::new();
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            out.push(path.strip_prefix(root).unwrap().display().to_string());
            if path.is_dir() {
                walk(&path, root, out);
            }
        }
    }
    walk(&dir, &dir, &mut out);
    out.sort();
    out
}

#[test]
fn open_help_contract_is_stable() {
    let home = isolated_home();
    let expected = "\
Usage:
  incodex open [--dry-run] [--app <path>]

Open an incognito window without patching Codex. Uses an isolated CODEX_HOME
and Chromium user-data-dir. The hat-glasses control and banner still appear
in that window. Closing the window burns that session.

Examples:
  incodex open
  incodex open --dry-run

";
    for flag in ["--help", "-h"] {
        let (status, stdout, stderr) = run(&["open", flag], &home);
        assert_eq!(status, 0, "{flag}");
        assert_eq!(stderr, "", "{flag}");
        assert_eq!(stdout, expected, "{flag}");
    }
}

#[test]
fn open_dry_run_does_not_create_a_session_or_touch_asar() {
    let home = isolated_home();
    let app = fake_app(&home, "#!/bin/sh\nexit 0\n");
    fs::write(app.join("marker"), "do-not-touch\n").unwrap();
    let (status, stdout, stderr) = run(
        &["open", "--dry-run", "--app", app.to_str().unwrap()],
        &home,
    );
    assert_eq!(status, 0);
    assert_eq!(stderr, "");
    assert!(stdout.contains("➤ Open incognito without patching Codex"));
    assert!(stdout.contains(&format!("  App          {}", app.display())));
    assert!(stdout.contains(&format!(
        "  Binary       {}",
        app.join("Contents/MacOS/ChatGPT").display()
    )));
    assert!(stdout.contains("  ! Dry run. No window opened."));
    assert_eq!(incodex_paths(&home), Vec::<String>::new());
    assert_eq!(fs::read_to_string(app.join("marker")).unwrap(), "do-not-touch\n");
    assert!(!app.join("Contents/Resources/app.asar").exists());
}

#[test]
fn open_missing_binary_does_not_leave_a_session() {
    let home = isolated_home();
    let app = home.join("Empty.app");
    fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    fs::write(
        app.join("Contents/Info.plist"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>Missing</string></dict></plist>\n",
    )
    .unwrap();
    let (status, stdout, stderr) = run(&["open", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 1);
    assert_eq!(stdout, "");
    assert!(stderr.contains("CFBundleExecutable not found"));
    assert!(
        incodex_paths(&home)
            .iter()
            .all(|path| !path.contains("sessions") && !path.contains("codex-home"))
    );
}

#[test]
fn open_rejects_an_app_without_a_valid_info_plist() {
    let home = isolated_home();
    let app = home.join("NoPlist.app");
    fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    fs::write(app.join("Contents/MacOS/ChatGPT"), "#!/bin/sh\nexit 0\n").unwrap();
    let (status, stdout, stderr) = run(&["open", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 1);
    assert_eq!(stdout, "");
    assert!(stderr.contains("Info.plist"), "{stderr}");
    assert!(incodex_paths(&home).is_empty());
}

#[test]
fn open_uses_cf_bundle_executable_instead_of_a_hardcoded_chatgpt_name() {
    let home = isolated_home();
    let app = fake_app_with_executable(
        &home,
        "CodexFixture",
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOME/open-argv.txt\"\nexit 0\n",
    );
    let source = home.join(".codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let (status, stdout, stderr) = run(&["open", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 3, "stdout={stdout}\nstderr={stderr}");
    assert!(stderr.contains("UI injection was not accepted"), "{stderr}");
    assert!(home.join("open-argv.txt").exists(), "binary from plist was not launched");
}

#[test]
fn open_waits_then_burns_and_does_not_patch_asar() {
    let home = isolated_home();
    let app = fake_app(&home, "#!/bin/sh\nexit 0\n");
    let source = home.join(".codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    fs::write(source.join("config.toml"), "model = \"test\"\n").unwrap();
    let asar = app.join("Contents/Resources/app.asar");
    assert!(!asar.exists());
    let (status, stdout, stderr) = run(&["open", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 3, "a window that never passes UI injection must fail");
    assert!(stderr.contains("UI injection was not accepted"), "{stderr}");
    assert!(stdout.contains("➤ Opening incognito Codex window"));
    assert!(!stdout.contains("Opened. Incognito Codex window is ready."));
    assert!(stdout.contains("Closed. Isolated session removed."));
    assert!(!asar.exists());
    let leftover: Vec<_> = incodex_paths(&home)
        .into_iter()
        .filter(|path| path.contains("codex-home") || path.contains("/chromium"))
        .collect();
    assert!(leftover.is_empty(), "{leftover:?}");
}

#[test]
fn open_spawns_official_binary_with_localhost_debug_port_and_does_not_patch() {
    let home = isolated_home();
    let app = fake_app(
        &home,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOME/open-argv.txt\"\nexit 0\n",
    );
    let source = home.join(".codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let official = PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/app.asar");
    let official_before = official.exists().then(|| fs::read(&official).ok()).flatten();
    let (status, stdout, stderr) = run(&["open", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 3, "stdout={stdout}\nstderr={stderr}");
    assert!(stderr.contains("UI injection was not accepted"), "{stderr}");
    let argv = fs::read_to_string(home.join("open-argv.txt")).unwrap_or_default();
    assert!(
        argv.lines().any(|line| line.starts_with("--remote-debugging-port=")),
        "missing debug port flag in {argv:?}"
    );
    assert!(
        argv.lines().any(|line| line.starts_with("--remote-allow-origins=")),
        "missing allow-origins flag in {argv:?}"
    );
    assert!(
        argv.lines().any(|line| line.starts_with("--user-data-dir=")),
        "missing user-data-dir in {argv:?}"
    );
    assert!(!app.join("Contents/Resources/app.asar").exists());
    if let Some(before) = official_before {
        assert_eq!(fs::read(&official).unwrap(), before);
    }
}

#[test]
fn open_spawn_error_still_burns() {
    let home = isolated_home();
    let app = fake_app(&home, "#!/bin/sh\nexit 0\n");
    let exe = app.join("Contents/MacOS/ChatGPT");
    fs::remove_file(&exe).unwrap();
    fs::write(&exe, "not executable\n").unwrap();
    let source = home.join(".codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let (status, stdout, stderr) = run(&["open", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 1, "spawn failure must be observable: stderr={stderr} stdout={stdout}");
    assert!(stderr.contains("Unable to start the incognito window"), "{stderr}");
    let leftover: Vec<_> = incodex_paths(&home)
        .into_iter()
        .filter(|path| path.contains("codex-home") || path.contains("/chromium"))
        .collect();
    assert!(leftover.is_empty(), "stdout={stdout} leftover={leftover:?}");
}

#[test]
fn open_child_nonzero_is_not_success() {
    let home = isolated_home();
    let app = fake_app(&home, "#!/bin/sh\nexit 7\n");
    let source = home.join(".codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let (status, stdout, stderr) = run(&["open", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 1, "child failure must be observable: stderr={stderr} stdout={stdout}");
    assert!(stderr.contains("exited with status 7"), "{stderr}");
    let leftover: Vec<_> = incodex_paths(&home)
        .into_iter()
        .filter(|path| path.contains("codex-home") || path.contains("/chromium"))
        .collect();
    assert!(leftover.is_empty(), "stdout={stdout} leftover={leftover:?}");
}

#[test]
fn independent_open_processes_keep_sessions_isolated_and_report_each_session() {
    let home = isolated_home();
    let app = fake_app(
        &home,
        "#!/bin/sh\n\
id=\"$INCODEX_SESSION_ID\"\n\
printf '%s\\n' \"id=$id\" \"root=$INCODEX_SESSION_ROOT\" > \"$HOME/started-$id\"\n\
while [ ! -e \"$HOME/release-$id\" ]; do sleep 0.01; done\n",
    );
    let source = home.join(".codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();

    let first = open_process(&app, &home);
    let first_id = wait_for_started_id(&home, None);
    let first_root = started_session(&home, &first_id);
    let second = open_process(&app, &home);
    let second_id = wait_for_started_id(&home, Some(&first_id));
    let second_root = started_session(&home, &second_id);

    assert_ne!(first_id, second_id);
    assert_ne!(first_root, second_root);
    assert!(first_root.is_dir());
    assert!(second_root.is_dir());

    fs::write(home.join(format!("release-{first_id}")), "").unwrap();
    let first_output = wait_for_exit(first);
    let first_removed_before_second = !first_root.exists();
    let second_survived_first = second_root.is_dir();

    fs::write(home.join(format!("release-{second_id}")), "").unwrap();
    let second_output = wait_for_exit(second);
    let second_removed_after_release = !second_root.exists();

    assert!(first_removed_before_second, "first session was not burned");
    assert!(second_survived_first, "first close burned the second session");
    assert!(second_removed_after_release, "second session was not burned");
    let first_stdout = String::from_utf8_lossy(&first_output.stdout);
    let second_stdout = String::from_utf8_lossy(&second_output.stdout);
    assert!(
        first_stdout.contains(first_id.as_str()),
        "first output must identify its session {first_id}: {first_stdout}"
    );
    assert!(
        first_stdout
            .lines()
            .any(|line| line.contains("Session") && line.contains(first_id.as_str())),
        "first output must label {first_id} as the session: {first_stdout}"
    );
    assert!(
        !first_stdout.contains(second_id.as_str()),
        "first output must not identify the second session {second_id}: {first_stdout}"
    );
    assert!(
        second_stdout.contains(second_id.as_str()),
        "second output must identify its session {second_id}: {second_stdout}"
    );
    assert!(
        second_stdout
            .lines()
            .any(|line| line.contains("Session") && line.contains(second_id.as_str())),
        "second output must label {second_id} as the session: {second_stdout}"
    );
    assert!(
        !second_stdout.contains(first_id.as_str()),
        "second output must not identify the first session {first_id}: {second_stdout}"
    );
}

fn wait_for_started_id(home: &Path, exclude: Option<&str>) -> String {
    for _ in 0..1_000 {
        let mut ids = fs::read_dir(home)
            .unwrap()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.strip_prefix("started-")
                    .map(str::to_string)
                    .filter(|id| exclude != Some(id.as_str()))
            })
            .collect::<Vec<_>>();
        ids.sort();
        if let Some(id) = ids.into_iter().next() {
            return id;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("timed out waiting for an open session marker");
}
