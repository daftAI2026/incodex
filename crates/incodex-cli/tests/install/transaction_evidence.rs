use super::*;

fn transaction_journal(home: &Path) -> (String, serde_json::Value) {
    let transactions = home.join(".incodex/transactions");
    let mut entries = fs::read_dir(&transactions)
        .unwrap()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1, "expected one transaction: {entries:?}");
    let entry = entries.pop().unwrap();
    let id = entry.file_name().to_string_lossy().into_owned();
    let journal: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(entry.path().join("journal.json")).unwrap(),
    )
    .unwrap();
    (id, journal)
}

#[test]
fn install_codesign_failure_aborts_custom_target_before_swap() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let asar = app.join("Contents/Resources/app.asar");
    let before = fs::read(&asar).unwrap();
    let cua = app
        .join("Contents/Frameworks/CUALockScreenGuardian.app/Contents/MacOS/CUALockScreenGuardian");
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    write_executable(
        &fake_bin.join("codesign"),
        "#!/bin/sh\nprintf '%s\\n' 'forced codesign failure' >&2\nexit 1\n",
    );

    let (status, _stdout, stderr) = run_with_path(
        &["install", "--yes", "--app", app.to_str().unwrap()],
        &home,
        &path_with_fake_bin(&fake_bin),
    );
    assert_eq!(status, 1, "stderr={stderr}");
    assert!(stderr.contains("forced codesign failure"), "{stderr}");
    assert_eq!(fs::read(&asar).unwrap(), before);
    assert!(
        cua.exists(),
        "vendor helper must be restored after sign failure"
    );
    let (id, journal) = transaction_journal(&home);
    assert_eq!(journal["phase"], "ROLLED_BACK");
    assert!(home
        .join(".incodex/transactions")
        .join(&id)
        .join("original/ChatGPT.app")
        .exists());
    assert!(!home
        .join(".incodex/scratch")
        .join(format!("ChatGPT.app.staged-{id}"))
        .exists());
}

#[test]
fn install_aborts_when_asar_integrity_cannot_be_written() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let asar = app.join("Contents/Resources/app.asar");
    let before = fs::read(&asar).unwrap();
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    write_executable(
        &fake_bin.join("plutil"),
        "#!/bin/sh\nif [ \"$1\" = \"-convert\" ]; then exec /usr/bin/plutil \"$@\"; fi\nexit 1\n",
    );
    write_executable(&fake_bin.join("codesign"), "#!/bin/sh\nexit 0\n");

    let (status, _stdout, stderr) = run_with_path(
        &["install", "--yes", "--app", app.to_str().unwrap()],
        &home,
        &path_with_fake_bin(&fake_bin),
    );
    assert_eq!(status, 1, "stderr={stderr}");
    assert!(
        stderr.contains("ElectronAsarIntegrity") || stderr.contains("plutil"),
        "{stderr}"
    );
    assert_eq!(fs::read(&asar).unwrap(), before);
    let (id, journal) = transaction_journal(&home);
    assert_eq!(journal["phase"], "ROLLED_BACK");
    assert!(home
        .join(".incodex/transactions")
        .join(&id)
        .join("original/ChatGPT.app")
        .exists());
    assert!(!home
        .join(".incodex/scratch")
        .join(format!("ChatGPT.app.staged-{id}"))
        .exists());
}
