use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
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
    let app = root.join("ChatGPT.app");
    let mac = app.join("Contents").join("MacOS");
    fs::create_dir_all(&mac).unwrap();
    let exe = mac.join("ChatGPT");
    fs::write(&exe, script).unwrap();
    let mut perms = fs::metadata(&exe).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&exe, perms).unwrap();
    app
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
fn open_help_matches_golden() {
    let home = isolated_home();
    let expected = "\
Usage:
  incodex open [--dry-run] [--app <path>]

Open an incognito window without patching Codex. Uses an isolated CODEX_HOME
and Chromium user-data-dir. Closing the window burns that session.

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
    let app = home.join("Marker.app");
    fs::create_dir_all(&app).unwrap();
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
    fs::create_dir_all(&app).unwrap();
    let (status, stdout, stderr) = run(&["open", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 1);
    assert_eq!(stdout, "");
    assert!(stderr.contains("Codex binary not found"));
    assert!(
        incodex_paths(&home)
            .iter()
            .all(|path| !path.contains("sessions") && !path.contains("codex-home"))
    );
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
    assert_eq!(stderr, "", "{stdout}");
    assert_eq!(status, 0);
    assert!(stdout.contains("➤ Opening incognito window"));
    assert!(stdout.contains("Closed. Isolated session removed."));
    assert!(!asar.exists());
    let leftover: Vec<_> = incodex_paths(&home)
        .into_iter()
        .filter(|path| path.contains("codex-home") || path.contains("/chromium"))
        .collect();
    assert!(leftover.is_empty(), "{leftover:?}");
}

#[test]
fn open_spawn_error_still_burns() {
    let home = isolated_home();
    let app = fake_app(&home, "#!/bin/sh\nexit 0\n");
    let exe = app.join("Contents/MacOS/ChatGPT");
    fs::remove_file(&exe).unwrap();
    fs::create_dir(&exe).unwrap();
    let source = home.join(".codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let (status, stdout, stderr) = run(&["open", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0, "stderr={stderr} stdout={stdout}");
    let leftover: Vec<_> = incodex_paths(&home)
        .into_iter()
        .filter(|path| path.contains("codex-home") || path.contains("/chromium"))
        .collect();
    assert!(leftover.is_empty(), "stdout={stdout} leftover={leftover:?}");
}
