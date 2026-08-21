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
