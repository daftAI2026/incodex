use incodex_macos::ditto;
use incodex_transaction::Engine;

use super::*;

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
