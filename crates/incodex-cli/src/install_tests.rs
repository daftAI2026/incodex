use super::*;
use incodex_macos::{AppQuiescence, ProcessProbe, QuiescenceClock, QuitRequester};
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct FixtureProbe {
    paths: Vec<(i32, PathBuf)>,
}

impl ProcessProbe for FixtureProbe {
    fn process_paths(&self) -> Result<Vec<(i32, PathBuf)>, String> {
        Ok(self.paths.clone())
    }
}

struct FailingQuit;

impl QuitRequester for FailingQuit {
    fn request_quit(&mut self, _executable: &Path, _pids: &[i32]) -> Result<(), String> {
        Err("fixture osascript failure".into())
    }
}

struct SuccessfulQuit;

impl QuitRequester for SuccessfulQuit {
    fn request_quit(&mut self, _executable: &Path, _pids: &[i32]) -> Result<(), String> {
        Ok(())
    }
}

struct FixtureClock(Instant);

impl QuiescenceClock for FixtureClock {
    fn now(&self) -> Instant {
        self.0
    }

    fn sleep(&mut self, duration: Duration) {
        self.0 += duration;
    }
}

#[test]
fn cli_official_quit_propagates_request_errors() {
    let quiescence = AppQuiescence::from_executable(PathBuf::from(
        "/tmp/incodex/ChatGPT.app/Contents/MacOS/ChatGPT",
    ))
    .unwrap();
    let probe = FixtureProbe {
        paths: vec![(42, quiescence.executable().to_path_buf())],
    };
    let mut requester = FailingQuit;
    let mut clock = FixtureClock(Instant::now());

    let error =
        close_official_app_with(&quiescence, &probe, &mut requester, &mut clock).unwrap_err();

    assert!(error.contains("fixture osascript failure"), "{error}");
}

#[test]
fn cli_official_quit_propagates_timeout() {
    let quiescence = AppQuiescence::from_executable(PathBuf::from(
        "/tmp/incodex/ChatGPT.app/Contents/MacOS/ChatGPT",
    ))
    .unwrap();
    let probe = FixtureProbe {
        paths: vec![(42, quiescence.executable().to_path_buf())],
    };
    let mut requester = SuccessfulQuit;
    let mut clock = FixtureClock(Instant::now());

    let error =
        close_official_app_with(&quiescence, &probe, &mut requester, &mut clock).unwrap_err();

    assert!(error.contains("timed out"), "{error}");
}

#[test]
fn default_path_foreign_bundle_is_rejected_before_snapshot() {
    let info = incodex_macos::PlistInfo {
        bundle_identifier: "com.example.foreign".into(),
        ..Default::default()
    };
    let error = ensure_official_bundle_identifier(&info).unwrap_err();
    assert!(error.contains("foreign bundle"), "{error}");
}

#[test]
fn locked_target_validation_failure_rolls_back_before_original_snapshot() {
    let sandbox = std::env::temp_dir().join(format!(
        "incodex-locked-install-validation-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let root = sandbox.join("state");
    let app = sandbox.join("ChatGPT.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), "replacement\n").unwrap();
    let canonical = fs::canonicalize(&app).unwrap();

    let result = begin_verified_transaction(&root, &app, |locked_target| {
        assert_eq!(locked_target, canonical);
        Err("locked target validation failed".to_string())
    });

    assert!(result.is_err());
    let transaction = fs::read_dir(root.join("transactions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .file_name()
        .to_string_lossy()
        .into_owned();
    let journal = journal_v2(&root, &transaction).unwrap();
    assert_eq!(journal.phase, "ROLLED_BACK");
    assert!(!root
        .join("transactions")
        .join(transaction)
        .join("original/ChatGPT.app")
        .exists());
    fs::remove_dir_all(sandbox).unwrap();
}

#[test]
fn target_replacement_after_locked_validation_does_not_leave_a_snapshot() {
    let sandbox = std::env::temp_dir().join(format!(
        "incodex-target-replacement-before-snapshot-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let root = sandbox.join("state");
    let app = sandbox.join("ChatGPT.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), "original\n").unwrap();

    let mut tx = begin_verified_transaction(&root, &app, |_locked_target| {
        let moved = sandbox.join("ChatGPT-updater-old.app");
        fs::rename(&app, &moved).unwrap();
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("marker"), "updater replacement\n").unwrap();
        Ok(())
    })
    .unwrap();
    let install_id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&install_id)
        .join("original/ChatGPT.app");

    let error = snapshot_original(&mut tx, &app, &original).unwrap_err();

    assert!(error.contains("changed"), "{error}");
    assert_eq!(journal_v2(&root, &install_id).unwrap().phase, "ROLLED_BACK");
    assert!(!original.exists());
    fs::remove_dir_all(sandbox).unwrap();
}

#[test]
fn snapshot_failure_after_backup_commit_rolls_back_the_transaction() {
    let sandbox = std::env::temp_dir().join(format!(
        "incodex-snapshot-failure-after-backup-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let root = sandbox.join("state");
    let app = sandbox.join("ChatGPT.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), "original\n").unwrap();

    let mut tx = Engine::begin(&root, &app, "snapshot-failure-test").unwrap();
    let install_id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&install_id)
        .join("original/ChatGPT.app");
    fs::create_dir_all(&original).unwrap();
    fs::copy(app.join("marker"), original.join("marker")).unwrap();
    tx.mark_backup_committed().unwrap();

    // mark_backup_committed() may return after BACKUP_COMMITTED is durable.
    let error = rollback_snapshot_failure(&mut tx, "injected snapshot failure".into());

    assert!(error.contains("injected snapshot failure"), "{error}");
    assert_eq!(journal_v2(&root, &install_id).unwrap().phase, "ROLLED_BACK");
    assert!(original.exists());
    fs::remove_dir_all(sandbox).unwrap();
}

#[test]
fn durable_rollback_error_cleans_scratch_without_claiming_recover_completed() {
    let sandbox = std::env::temp_dir().join(format!(
        "incodex-durable-rollback-error-{}",
        std::process::id()
    ));
    let root = sandbox.join("state");
    let app = sandbox.join("ChatGPT.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), "original\n").unwrap();

    let mut tx = Engine::begin(&root, &app, "durable-rollback-error-test").unwrap();
    tx.rollback("test rollback").unwrap();
    assert_eq!(tx.journal().phase, "ROLLED_BACK");
    let scratch = root
        .join("scratch")
        .join(format!("ChatGPT.app.staged-{}", tx.install_id()));
    fs::create_dir_all(&scratch).unwrap();

    let output = finish_rollback(
        &tx,
        Some(&scratch),
        "install failed".into(),
        Some("journal readback failed".into()),
    );

    assert!(
        !scratch.exists(),
        "durable rollback must still clean scratch"
    );
    assert!(output.contains("rollback reached ROLLED_BACK"), "{output}");
    assert!(!output.contains("recover"), "{output}");
    fs::remove_dir_all(sandbox).unwrap();
}
