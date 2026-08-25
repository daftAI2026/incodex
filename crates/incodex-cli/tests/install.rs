use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_asar::{pack_dir, Archive, LOADER_NAME, MARKER_KEY};
use incodex_macos::ditto;
use incodex_transaction::{acquire_target_lock, journal_v2, Engine};

#[path = "install/transaction_evidence.rs"]
mod transaction_evidence;

#[path = "install/fixtures.rs"]
mod fixtures;
use fixtures::{
    codesign_display, is_signed, marker_app, patchable_app, tree_digest, write_executable,
};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_incodex")
}

static SEQ: AtomicU64 = AtomicU64::new(0);

fn isolated_home() -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("incodex-install-{}-{n}", std::process::id()));
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

fn run_from(args: &[&str], home: &Path, current_dir: &Path) -> (i32, String, String) {
    let output = Command::new(bin())
        .args(args)
        .current_dir(current_dir)
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

fn run_with_path(args: &[&str], home: &Path, path: &Path) -> (i32, String, String) {
    let output = Command::new(bin())
        .args(args)
        .env("HOME", home)
        .env("PATH", path)
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

fn path_with_fake_bin(bin: &Path) -> PathBuf {
    let mut path = OsString::from(bin.as_os_str());
    path.push(":");
    if let Some(existing) = std::env::var_os("PATH") {
        path.push(existing);
    }
    PathBuf::from(path)
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

fn install_mutations(home: &Path) -> Vec<String> {
    incodex_paths(home)
        .into_iter()
        .filter(|path| !path.starts_with("cache"))
        .collect()
}

#[test]
fn install_dry_run_app_prints_plan_and_does_not_mutate() {
    let home = isolated_home();
    let app = marker_app(&home);
    let (status, stdout, stderr) = run(
        &["install", "--dry-run", "--app", app.to_str().unwrap()],
        &home,
    );
    assert_eq!(status, 0);
    assert_eq!(stderr, "");
    assert!(stdout.contains("➤ Install"));
    assert!(stdout.contains(&format!("  App          {}", app.display())));
    assert!(stdout.contains("  ! Dry run. No files changed."));
    assert_eq!(install_mutations(&home), Vec::<String>::new());
    assert_eq!(
        fs::read_to_string(app.join("marker")).unwrap(),
        "do-not-touch\n"
    );
}

#[test]
fn install_short_n_is_the_same_as_dry_run() {
    let home = isolated_home();
    let app = marker_app(&home);
    let dashed = run(
        &["install", "--dry-run", "--app", app.to_str().unwrap()],
        &home,
    );
    let short = run(&["install", "-n", "--app", app.to_str().unwrap()], &home);
    assert_eq!(dashed.0, 0);
    assert_eq!(short, dashed);
    assert!(dashed.1.contains("  ! Dry run. No files changed."));
}

#[test]
fn uninstall_dry_run_app_contract_is_stable() {
    let home = isolated_home();
    let app = marker_app(&home);
    let (status, stdout, stderr) = run(
        &["uninstall", "--dry-run", "--app", app.to_str().unwrap()],
        &home,
    );
    assert_eq!(status, 0);
    assert_eq!(stderr, "");
    assert_eq!(
        stdout,
        format!(
            "➤ Uninstall\n  App          {}\n  ! Dry run. No files changed.\n",
            app.display()
        )
    );
    assert_eq!(install_mutations(&home), Vec::<String>::new());
}

#[test]
fn non_tty_app_install_requires_yes_and_still_prints_the_plan() {
    let home = isolated_home();
    let app = marker_app(&home);
    let (status, stdout, stderr) = run(&["install", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 1);
    assert_eq!(
        stderr,
        "  ✗ non-interactive install requires --yes\n  incodex install --yes\n"
    );
    assert!(stdout.contains("➤ Install"));
    assert!(stdout.contains(&format!("  App          {}", app.display())));
    assert_eq!(install_mutations(&home), Vec::<String>::new());
    assert_eq!(
        fs::read_to_string(app.join("marker")).unwrap(),
        "do-not-touch\n"
    );
}

#[test]
fn clone_dry_run_does_not_create_scratch() {
    let home = isolated_home();
    let (status, stdout, stderr) = run(&["install", "--clone", "--dry-run"], &home);
    assert_eq!(status, 0);
    assert_eq!(stderr, "");
    assert!(stdout.contains("➤ Clone install"));
    assert!(stdout.contains("  ! Dry run. No files changed."));
    assert!(!home.join(".incodex").join("scratch").exists());
}

#[test]
fn recover_missing_transaction_is_explicit() {
    let home = isolated_home();
    let (status, stdout, stderr) = run(&["recover", "--transaction", "does-not-exist"], &home);
    assert_eq!(status, 1);
    assert_eq!(stdout, "");
    assert_eq!(stderr, "  ✗ no journal for does-not-exist\n");
}

#[test]
fn recover_dry_run_preserves_native_v2_transaction() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let root = home.join(".incodex");
    let staged_source = home.join("dry-run-staged.app");
    ditto(&app, &staged_source).unwrap();
    fs::write(staged_source.join("dry-run-marker"), "must remain staged\n").unwrap();

    let mut tx = Engine::begin(&root, &app, "recover-dry-run-test").unwrap();
    let id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&id)
        .join("original/ChatGPT.app");
    ditto(&app, &original).unwrap();
    tx.mark_backup_committed().unwrap();
    tx.place_staging(&staged_source).unwrap();
    assert_eq!(tx.journal().phase, "STAGED");
    drop(tx);

    let tx_dir = root.join("transactions").join(&id);
    let journal = tx_dir.join("journal.json");
    let staged = tx_dir.join("staging/ChatGPT.app");
    let root_before = tree_digest(&root);
    let app_before = tree_digest(&app);
    let journal_before = fs::read(&journal).unwrap();
    let staged_before = tree_digest(&staged);

    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    write_executable(&fake_bin.join("codesign"), "#!/bin/sh\nexit 0\n");
    let (status, stdout, stderr) = run_with_path(
        &["recover", "--dry-run", "--transaction", &id],
        &home,
        &path_with_fake_bin(&fake_bin),
    );

    assert_eq!(status, 1, "recover dry-run must fail closed");
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "  ✗ recover --dry-run is not supported; no files changed\n"
    );
    assert_eq!(
        tree_digest(&root),
        root_before,
        "dry-run changed INCODEX_HOME"
    );
    assert_eq!(
        tree_digest(&app),
        app_before,
        "dry-run changed the live app"
    );
    assert_eq!(fs::read(&journal).unwrap(), journal_before);
    assert!(
        staged.exists(),
        "dry-run removed the staged transaction tree"
    );
    assert_eq!(tree_digest(&staged), staged_before);
}

#[test]
fn recover_legacy_flat_journal_fails_explicitly_without_changes() {
    let home = isolated_home();
    let app = marker_app(&home);
    let root = home.join(".incodex");
    let transactions = root.join("transactions");
    fs::create_dir_all(&transactions).unwrap();
    let id = "00000000-0000-4000-8000-000000000042";
    let journal = transactions.join(format!("{id}.json"));
    fs::write(
        &journal,
        format!(
            "{}\n",
            serde_json::json!({
                "schemaVersion": 1,
                "installId": id,
                "targetRealPath": app,
                "stagedApp": root.join("scratch/legacy-staged.app"),
                "originalSnapshot": root.join("installations/legacy/original/ChatGPT.app"),
                "phase": "STAGED",
                "updatedAt": "2026-01-01T00:00:00.000Z"
            })
        ),
    )
    .unwrap();
    let root_before = tree_digest(&root);
    let app_before = tree_digest(&app);
    let journal_before = fs::read(&journal).unwrap();

    for args in [
        vec!["recover", "--transaction", id],
        vec!["recover", "--dry-run", "--transaction", id],
    ] {
        let (status, stdout, stderr) = run(&args, &home);
        assert_eq!(status, 1, "legacy recovery must fail closed");
        assert_eq!(stdout, "");
        assert!(stderr.to_lowercase().contains("legacy"), "{stderr}");
        assert!(stderr.to_lowercase().contains("not supported"), "{stderr}");
        assert!(
            stderr.to_lowercase().contains("no files changed"),
            "{stderr}"
        );
        assert!(!stderr.contains("No such file or directory"), "{stderr}");
    }
    assert_eq!(tree_digest(&root), root_before);
    assert_eq!(tree_digest(&app), app_before);
    assert_eq!(fs::read(&journal).unwrap(), journal_before);
}

#[test]
fn install_yes_app_patches_asar_writes_runtime_and_commits() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let official = PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/app.asar");
    let official_before = official
        .exists()
        .then(|| fs::read(&official).ok())
        .flatten();
    let cua_app = app.join("Contents/Frameworks/CUALockScreenGuardian.app");
    let cua = cua_app.join("Contents/MacOS/CUALockScreenGuardian");
    let cua_before = fs::read(&cua).unwrap();
    let cua_display_before = codesign_display(&cua_app);
    assert!(is_signed(&cua_app));

    let (status, stdout, stderr) =
        run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    assert_eq!(stderr, "");
    assert!(stdout.contains("➤ Install"));

    let asar = app.join("Contents/Resources/app.asar");
    let archive = Archive::open(&asar).unwrap();
    let pkg: serde_json::Value =
        serde_json::from_slice(&archive.extract("package.json").unwrap()).unwrap();
    assert_eq!(pkg["main"], LOADER_NAME);
    assert_eq!(pkg[MARKER_KEY]["originalMain"], "index.js");
    assert_eq!(
        String::from_utf8(archive.extract("index.js").unwrap()).unwrap(),
        "ok\n"
    );
    assert!(archive.extract(LOADER_NAME).is_ok());
    assert!(archive.has_only_loader());

    assert!(home.join(".incodex").join("runtime").exists());
    let journals: Vec<_> = install_mutations(&home)
        .into_iter()
        .filter(|path| path.ends_with("journal.json"))
        .collect();
    assert_eq!(journals.len(), 1);
    let journal: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(home.join(".incodex").join(&journals[0])).unwrap(),
    )
    .unwrap();
    assert_eq!(journal["phase"], "COMMITTED");
    assert_eq!(journal["schemaVersion"], 2);

    assert_eq!(fs::read(&cua).unwrap(), cua_before);
    assert!(
        is_signed(&cua_app),
        "vendor CUA sidecar signature must survive"
    );
    let cua_display = codesign_display(&cua_app);
    assert_eq!(
        cua_display, cua_display_before,
        "vendor CUA signature must be preserved"
    );
    assert!(
        !cua_display.to_lowercase().contains("2dc432gll2"),
        "{cua_display}"
    );

    if let Some(before) = official_before {
        assert_eq!(fs::read(&official).unwrap(), before);
    }
}

#[test]
fn install_accepts_an_existing_relative_app_path() {
    let home = isolated_home();
    let app = patchable_app(&home);

    let (status, stdout, stderr) = run_from(
        &["install", "--yes", "--app", "bundle/ChatGPT.app"],
        &home,
        &home,
    );

    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    assert!(
        app.join("Contents/Resources/app.asar").exists(),
        "relative target must resolve to the requested app"
    );
}

#[test]
fn install_refuses_stale_loader_without_a_committed_transaction() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let fake = home.join("fake-loader-only");
    fs::create_dir_all(&fake).unwrap();
    fs::write(
        fake.join("package.json"),
        format!(
            "{}\n",
            serde_json::json!({
                "main": LOADER_NAME,
                (MARKER_KEY): {
                    "originalMain": "index.js",
                    "installId": "00000000-0000-4000-8000-000000000000"
                }
            })
        ),
    )
    .unwrap();
    fs::write(fake.join(LOADER_NAME), "stale loader\n").unwrap();
    fs::write(fake.join("index.js"), "ok\n").unwrap();
    let asar = app.join("Contents/Resources/app.asar");
    pack_dir(&fake, &asar).unwrap();
    let before = fs::read(&asar).unwrap();

    let (status, _stdout, stderr) =
        run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 1, "stale patched app was accepted: {stderr}");
    assert!(stderr.contains("marker"), "{stderr}");
    assert_eq!(fs::read(&asar).unwrap(), before);
    let transactions = home.join(".incodex/transactions");
    assert!(!transactions.exists() || fs::read_dir(transactions).unwrap().next().is_none());
}

#[test]
fn uninstall_refuses_while_another_command_holds_the_target_lock() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let (status, _, stderr) = run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0, "{stderr}");
    let asar = app.join("Contents/Resources/app.asar");
    let patched = fs::read(&asar).unwrap();
    let root = home.join(".incodex");
    let _lock = acquire_target_lock(&root, &app, "test-holder", None).unwrap();

    let (status, _stdout, stderr) = run(
        &["uninstall", "--yes", "--app", app.to_str().unwrap()],
        &home,
    );
    assert_eq!(status, 1, "{stderr}");
    assert!(
        stderr.contains("another incodex command is modifying this app"),
        "{stderr}"
    );
    assert_eq!(fs::read(asar).unwrap(), patched);
}

#[test]
fn post_swap_verification_failure_rolls_back_the_original_app() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let asar = app.join("Contents/Resources/app.asar");
    let before = fs::read(&asar).unwrap();
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    write_executable(
        &fake_bin.join("codesign"),
        &format!(
            "#!/bin/sh\nlast=''\nfor arg in \"$@\"; do last=\"$arg\"; done\nif [ \"$1\" = \"--display\" ] && [ \"$2\" = \"--entitlements\" ]; then\n  printf '%s\\n' '<plist><dict></dict></plist>'\n  exit 0\nfi\nif [ \"$1\" = \"--display\" ] && [ \"$2\" = \"--verbose=4\" ]; then\n  case \"$last\" in *CUALockScreenGuardian.app) printf '%s\\n' 'Identifier=com.example.cua-guardian' 'TeamIdentifier=2DC432GLL2' 'Authority=Developer ID Application: fixture' ;; *) printf '%s\\n' 'Identifier=com.example.incodex-fixture' 'Signature=adhoc' ;; esac\n  exit 0\nfi\nif [ \"$1\" = \"--verify\" ] && [ \"$last\" = \"{}\" ]; then\n  printf '%s\\n' 'forced post-swap verification failure' >&2\n  exit 1\nfi\nexit 0\n",
            app.display()
        ),
    );

    let (status, _stdout, stderr) = run_with_path(
        &["install", "--yes", "--app", app.to_str().unwrap()],
        &home,
        &path_with_fake_bin(&fake_bin),
    );
    assert_eq!(status, 1, "{stderr}");
    assert!(
        stderr.contains("post-swap") || stderr.contains("verification"),
        "{stderr}"
    );
    assert_eq!(fs::read(&asar).unwrap(), before);
    let journal_path = install_mutations(&home)
        .into_iter()
        .find(|path| path.ends_with("journal.json"))
        .unwrap();
    let journal: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(home.join(".incodex").join(journal_path)).unwrap(),
    )
    .unwrap();
    assert_eq!(journal["phase"], "ROLLED_BACK");
    let id = journal["installId"].as_str().unwrap();
    assert!(home
        .join(".incodex/transactions")
        .join(id)
        .join("original/ChatGPT.app")
        .exists());
    assert!(!home
        .join(".incodex/transactions")
        .join(id)
        .join("staging/ChatGPT.app")
        .exists());
    assert!(!home
        .join(".incodex/transactions")
        .join(id)
        .join("outgoing/ChatGPT.app")
        .exists());
}

#[test]
fn interrupted_recover_refuses_uninstall_then_reinstall_round_trips_original() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let root = home.join(".incodex");
    let original_digest = tree_digest(&app);

    let (status, stdout, stderr) =
        run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    let (status, stdout, stderr) = run(
        &["uninstall", "--yes", "--app", app.to_str().unwrap()],
        &home,
    );
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    assert_eq!(tree_digest(&app), original_digest);

    let staged_source = home.join("interrupted-staged.app");
    ditto(&app, &staged_source).unwrap();
    fs::write(
        staged_source.join("interrupted-marker"),
        "must not survive\n",
    )
    .unwrap();
    let mut tx = Engine::begin(&root, &app, "test-interrupted-install").unwrap();
    let id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&id)
        .join("original/ChatGPT.app");
    ditto(&app, &original).unwrap();
    tx.mark_backup_committed().unwrap();
    tx.place_staging(&staged_source).unwrap();
    tx.swap().unwrap();
    assert_eq!(tx.journal().phase, "SWAPPED");
    assert_ne!(tree_digest(&app), original_digest);
    assert!(tx.outgoing_app().exists());
    drop(tx);

    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    write_executable(
        &fake_bin.join("codesign"),
        "#!/bin/sh\nlast=''\nfor arg in \"$@\"; do last=\"$arg\"; done\nif [ \"$1\" = \"--display\" ] && [ \"$2\" = \"--entitlements\" ]; then printf '%s\\n' '<plist><dict></dict></plist>'; exit 0; fi\nif [ \"$1\" = \"--display\" ] && [ \"$2\" = \"--verbose=4\" ]; then case \"$last\" in *CUALockScreenGuardian.app) printf '%s\\n' 'Identifier=com.example.cua-guardian' 'TeamIdentifier=2DC432GLL2' 'Authority=Developer ID Application: fixture' ;; *) printf '%s\\n' 'Identifier=com.example.incodex-fixture' 'Signature=adhoc' ;; esac; exit 0; fi\nexit 0\n",
    );
    let (status, stdout, stderr) = run_with_path(
        &["recover", "--transaction", &id],
        &home,
        &path_with_fake_bin(&fake_bin),
    );
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    assert!(stdout.contains("phase: ROLLED_BACK"), "{stdout}");
    assert_eq!(tree_digest(&app), original_digest);
    let tx_dir = root.join("transactions").join(&id);
    let journal: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(tx_dir.join("journal.json")).unwrap()).unwrap();
    assert_eq!(journal["phase"], "ROLLED_BACK");
    assert!(tx_dir.join("original/ChatGPT.app").exists());
    assert!(!tx_dir.join("staging/ChatGPT.app").exists());
    assert!(!tx_dir.join("outgoing/ChatGPT.app").exists());

    let (status, _stdout, stderr) = run(
        &["uninstall", "--yes", "--app", app.to_str().unwrap()],
        &home,
    );
    assert_eq!(
        status, 1,
        "uninstall must refuse without a committed install"
    );
    assert!(
        stderr.contains("no installation record for this target"),
        "{stderr}"
    );
    assert_eq!(tree_digest(&app), original_digest);

    let (status, stdout, stderr) =
        run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    assert_ne!(tree_digest(&app), original_digest);

    let (status, stdout, stderr) = run(
        &["uninstall", "--yes", "--app", app.to_str().unwrap()],
        &home,
    );
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    assert_eq!(tree_digest(&app), original_digest);

    for entry in fs::read_dir(root.join("transactions")).unwrap().flatten() {
        let tx_dir = entry.path();
        assert!(!tx_dir.join("staging/ChatGPT.app").exists());
        assert!(!tx_dir.join("outgoing/ChatGPT.app").exists());
    }
}

#[test]
fn recover_does_not_finish_until_the_restored_app_verifies() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let root = home.join(".incodex");
    let (status, _, stderr) = run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0, "{stderr}");

    let staged = home.join("broken-staged.app");
    let copied = Command::new("ditto")
        .args([&app, &staged])
        .status()
        .unwrap();
    assert!(copied.success());
    fs::write(
        staged.join("Contents/MacOS/ChatGPT"),
        "broken after signing\n",
    )
    .unwrap();
    let mut tx = Engine::begin(&root, &app, "test-crash").unwrap();
    let original = root
        .join("transactions")
        .join(tx.install_id())
        .join("original/ChatGPT.app");
    ditto(&app, &original).unwrap();
    tx.mark_backup_committed().unwrap();
    tx.place_staging(&staged).unwrap();
    tx.swap().unwrap();
    let id = tx.install_id().to_string();
    drop(tx);

    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    write_executable(&fake_bin.join("codesign"), "#!/bin/sh\nexit 1\n");
    let (status, _stdout, stderr) = run_with_path(
        &["recover", "--transaction", &id],
        &home,
        &path_with_fake_bin(&fake_bin),
    );
    assert_eq!(status, 1, "{stderr}");
    assert!(
        stderr.contains("restored target failed codesign verification"),
        "{stderr}"
    );
    let journal: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("transactions").join(&id).join("journal.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(journal["phase"], "SWAPPED");

    let (status, stdout, stderr) = run(&["recover", "--transaction", &id], &home);
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    assert!(stdout.contains("phase: ROLLED_BACK"), "{stdout}");
    let journal: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("transactions").join(&id).join("journal.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(journal["phase"], "ROLLED_BACK");
    assert!(root
        .join("transactions")
        .join(&id)
        .join("original/ChatGPT.app")
        .exists());
}

#[test]
fn uninstall_yes_app_restores_original_asar() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let (status, stdout, stderr) =
        run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    let install_id = Archive::open(app.join("Contents/Resources/app.asar"))
        .unwrap()
        .read_package_main()
        .unwrap()
        .install_id
        .unwrap();

    let (status, stdout, stderr) = run(
        &["uninstall", "--yes", "--app", app.to_str().unwrap()],
        &home,
    );
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    assert_eq!(stderr, "");
    assert!(stdout.contains("➤ Uninstall"));

    let archive = Archive::open(app.join("Contents/Resources/app.asar")).unwrap();
    let pkg: serde_json::Value =
        serde_json::from_slice(&archive.extract("package.json").unwrap()).unwrap();
    assert_eq!(pkg["main"], "index.js");
    assert!(archive.extract(LOADER_NAME).is_err());
    assert_eq!(
        String::from_utf8(archive.extract("index.js").unwrap()).unwrap(),
        "ok\n"
    );
    assert_eq!(
        journal_v2(&home.join(".incodex"), &install_id)
            .unwrap()
            .phase,
        "ROLLED_BACK"
    );
}

#[test]
fn recover_committed_transaction_is_done() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let (status, _, stderr) = run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0, "{stderr}");
    let journal_path = install_mutations(&home)
        .into_iter()
        .find(|path| path.ends_with("journal.json"))
        .expect("journal");
    let journal: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(home.join(".incodex").join(&journal_path)).unwrap(),
    )
    .unwrap();
    let id = journal["installId"].as_str().expect("installId");
    let asar_before = fs::read(app.join("Contents/Resources/app.asar")).unwrap();

    let (status, stdout, stderr) = run(&["recover", "--transaction", id], &home);
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    assert_eq!(stderr, "");
    assert_eq!(
        fs::read(app.join("Contents/Resources/app.asar")).unwrap(),
        asar_before
    );
}
