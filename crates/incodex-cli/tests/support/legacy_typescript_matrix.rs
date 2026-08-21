// Shared matrix cases for the retired TypeScript v1 reader.
// This is included by legacy_typescript.rs so Cargo keeps one fixture crate.
use super::*;

#[derive(Debug, Clone, Copy)]
enum MatrixCurrent {
    Absent,
    MatchingCommitted,
    MissingJournal,
    RolledBack,
}

impl MatrixCurrent {
    fn install_id(self) -> Option<&'static str> {
        match self {
            Self::Absent => None,
            Self::MatchingCommitted => Some(INSTALL_ID),
            Self::MissingJournal => Some(MISSING_CURRENT_INSTALL_ID),
            Self::RolledBack => Some(ROLLED_BACK_INSTALL_ID),
        }
    }
}

fn empty_legacy_state_root() -> (PathBuf, PathBuf) {
    let root = temp_root();
    let app = root.join("ChatGPT.app");
    fs::create_dir_all(&app).unwrap();
    (root.join(".incodex"), app)
}

fn write_current_pointer(root: &std::path::Path, app: &std::path::Path, install_id: &str) {
    let target_store = root.join("installations").join(target_id(app));
    fs::create_dir_all(&target_store).unwrap();
    fs::write(
        target_store.join("current.json"),
        format!("{}\n", json!({"installId": install_id})),
    )
    .unwrap();
}

fn build_matrix_state(interrupted_count: usize, current: MatrixCurrent) -> (PathBuf, PathBuf) {
    let (root, app) = if matches!(current, MatrixCurrent::MatchingCommitted) {
        let fixture = LegacyTsV1Fixture::create();
        (fixture.root, fixture.app)
    } else {
        empty_legacy_state_root()
    };
    let interrupted_ids = if matches!(current, MatrixCurrent::MatchingCommitted) {
        [ORPHAN_INSTALL_ID, SECOND_ORPHAN_INSTALL_ID]
    } else {
        [INSTALL_ID, ORPHAN_INSTALL_ID]
    };
    for (index, install_id) in interrupted_ids.iter().take(interrupted_count).enumerate() {
        write_flat_journal(
            &root,
            &app,
            install_id,
            "DISCOVERED",
            &format!("2026-08-21T00:1{index}:00.000Z"),
        );
    }
    if matches!(current, MatrixCurrent::RolledBack) {
        write_flat_journal(
            &root,
            &app,
            ROLLED_BACK_INSTALL_ID,
            "ROLLED_BACK",
            "2026-08-21T00:20:00.000Z",
        );
    }
    if let Some(install_id) = current.install_id() {
        write_current_pointer(&root, &app, install_id);
    }
    (root, app)
}

#[test]
fn legacy_typescript_fixture_covers_interrupted_and_current_state_matrix() {
    for interrupted_count in 0..=2 {
        for current in [
            MatrixCurrent::Absent,
            MatrixCurrent::MatchingCommitted,
            MatrixCurrent::MissingJournal,
            MatrixCurrent::RolledBack,
        ] {
            let (root, app) = build_matrix_state(interrupted_count, current);
            let label = format!("interrupted={interrupted_count}, current={current:?}");
            let result = incodex_cli::legacy_typescript::load_legacy_ts_v1(&root, &app);
            match (interrupted_count, current) {
                (0, MatrixCurrent::Absent) => {
                    assert!(matches!(result, Ok(None)), "{label}: {result:?}");
                }
                (0, MatrixCurrent::MatchingCommitted) => {
                    let state = result
                        .expect("matching committed state should be readable")
                        .expect("matching committed state should be detected");
                    assert!(matches!(
                        state.state,
                        incodex_cli::legacy_typescript::LegacyState::Committed { .. }
                    ));
                }
                (1, MatrixCurrent::Absent) => {
                    let state = result
                        .expect("a lone interrupted state should be readable")
                        .expect("a lone interrupted journal should be detected");
                    assert!(matches!(
                        state.state,
                        incodex_cli::legacy_typescript::LegacyState::Interrupted
                    ));
                }
                _ => {
                    let error = result.expect_err(&format!(
                        "{label} must fail closed instead of selecting a journal"
                    ));
                    if interrupted_count > 0 {
                        if let Some(current_id) = current.install_id() {
                            assert!(error.contains(current_id), "{label}: {error}");
                        }
                    }
                    let interrupted_ids = if matches!(current, MatrixCurrent::MatchingCommitted) {
                        [ORPHAN_INSTALL_ID, SECOND_ORPHAN_INSTALL_ID]
                    } else {
                        [INSTALL_ID, ORPHAN_INSTALL_ID]
                    };
                    for install_id in interrupted_ids.iter().take(interrupted_count) {
                        assert!(error.contains(install_id), "{label}: {error}");
                    }
                }
            }
        }
    }
}

#[test]
fn legacy_typescript_fixture_rejects_empty_target_before_canonicalizing() {
    let root = temp_root().join(".incodex");
    let target = std::env::current_dir().expect("test working directory");
    let journal_path = write_flat_journal(
        &root,
        &target,
        INSTALL_ID,
        "DISCOVERED",
        "2026-08-21T00:30:00.000Z",
    );
    let mut raw: serde_json::Value =
        serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
    raw["targetRealPath"] = json!("");
    fs::write(
        &journal_path,
        format!("{}\n", serde_json::to_string_pretty(&raw).unwrap()),
    )
    .unwrap();

    let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&root, &target)
        .expect_err("an empty targetRealPath must be rejected before canonicalization");
    assert!(error.contains("targetRealPath"), "{error}");
}

#[test]
fn legacy_typescript_fixture_does_not_skip_empty_target_for_a_non_cwd_target() {
    let fixture = LegacyTsV1JournalFixture::create();
    assert_ne!(
        canonical_path(&fixture.app),
        std::env::current_dir().expect("test working directory"),
        "the regression requires a target different from the process cwd"
    );
    let mut raw: serde_json::Value =
        serde_json::from_slice(&fs::read(&fixture.journal_path).unwrap()).unwrap();
    raw["targetRealPath"] = json!("");
    fs::write(
        &fixture.journal_path,
        format!("{}\n", serde_json::to_string_pretty(&raw).unwrap()),
    )
    .unwrap();

    let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect_err("an empty targetRealPath must be an error, not a skipped journal");
    assert!(error.contains("targetRealPath"), "{error}");
}

#[test]
fn legacy_typescript_fixture_matches_parse_journal_field_guards() {
    // These are the retired src/transaction.ts parseJournal non-empty/type guards.
    let invalid_fields = [
        ("schemaVersion", json!(0)),
        ("schemaVersion", json!("1")),
        ("installId", json!("")),
        ("installId", json!(123)),
        ("targetRealPath", json!(123)),
        ("stagedApp", json!("")),
        ("stagedApp", json!(false)),
        ("originalSnapshot", json!("")),
        ("originalSnapshot", json!([])),
        ("outgoingApp", json!("")),
        ("outgoingApp", json!(false)),
        ("outgoingApp", serde_json::Value::Null),
        ("phase", json!("UNKNOWN")),
        ("phase", json!(123)),
        // Rust deliberately keeps the stronger existing non-empty timestamp guard.
        ("updatedAt", json!("")),
    ];

    for (field, replacement) in invalid_fields {
        let fixture = LegacyTsV1JournalFixture::create();
        let mut raw: serde_json::Value =
            serde_json::from_slice(&fs::read(&fixture.journal_path).unwrap()).unwrap();
        raw[field] = replacement;
        fs::write(
            &fixture.journal_path,
            format!("{}\n", serde_json::to_string_pretty(&raw).unwrap()),
        )
        .unwrap();
        let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
            .expect_err(&format!("legacy field {field} accepted invalid value"));
        assert!(
            !error.is_empty(),
            "legacy field {field} returned an empty error"
        );
    }

    let fixture = LegacyTsV1JournalFixture::create();
    let mut raw: serde_json::Value =
        serde_json::from_slice(&fs::read(&fixture.journal_path).unwrap()).unwrap();
    raw.as_object_mut().unwrap().remove("outgoingApp");
    fs::write(
        &fixture.journal_path,
        format!("{}\n", serde_json::to_string_pretty(&raw).unwrap()),
    )
    .unwrap();
    incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect("omitting optional outgoingApp should remain compatible")
        .expect("journal without optional outgoingApp should be detected");
}
