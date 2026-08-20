use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_transaction::{journal_v2, new_install_id, recover, Recovery};
use serde::Serialize;
use sha2::{Digest, Sha256};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("incodex-legacy-journal-{now}-{seq}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn app_bundle(root: &Path, marker: &str) -> PathBuf {
    let app = root.join("ChatGPT.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), marker).unwrap();
    app
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyTarget<'a> {
    requested_path: &'a str,
    real_path: &'a str,
    device: &'a str,
    inode: &'a str,
}

#[derive(Serialize)]
struct LegacyPaths<'a> {
    staged: &'a str,
    outgoing: &'a str,
    original: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyJournal<'a> {
    schema_version: u32,
    install_id: &'a str,
    target: LegacyTarget<'a>,
    paths: LegacyPaths<'a>,
    phase: &'a str,
    sequence: u64,
    checksum: &'a str,
}

fn legacy_json(id: &str, target: &Path, phase: &str) -> String {
    let target = target.display().to_string();
    let unsigned = LegacyJournal {
        schema_version: 2,
        install_id: id,
        target: LegacyTarget {
            requested_path: &target,
            real_path: &target,
            device: "legacy-device",
            inode: "legacy-inode",
        },
        paths: LegacyPaths {
            staged: "staging/ChatGPT.app",
            outgoing: "outgoing/ChatGPT.app",
            original: "original/ChatGPT.app",
        },
        phase,
        sequence: 7,
        checksum: "",
    };
    let canonical = serde_json::to_vec(&unsigned).unwrap();
    let checksum = Sha256::digest(canonical);
    let checksum = checksum
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let sealed = LegacyJournal {
        checksum: &checksum,
        ..unsigned
    };
    format!("{}\n", serde_json::to_string_pretty(&sealed).unwrap())
}

fn write_legacy(root: &Path, id: &str, target: &Path, phase: &str) {
    let dir = root.join("transactions").join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("journal.json"), legacy_json(id, target, phase)).unwrap();
}

#[test]
fn legacy_committed_journal_loads_and_cleanup_can_finish() {
    let root = scratch();
    let app = app_bundle(&root, "patched");
    let id = new_install_id();
    let outgoing = root
        .join("transactions")
        .join(&id)
        .join("outgoing/ChatGPT.app");
    fs::create_dir_all(&outgoing).unwrap();
    fs::write(outgoing.join("partial"), "leftover").unwrap();
    write_legacy(&root, &id, &app, "COMMITTED");

    let loaded = journal_v2(&root, &id).expect("legacy COMMITTED journal remains readable");
    assert_eq!(loaded.phase, "COMMITTED");
    assert!(loaded.target.parent_device.is_empty());
    assert!(loaded.pre_swap_digest.is_empty());

    let result = recover(&root, &id).expect("COMMITTED cleanup uses the legacy journal");
    assert_eq!(result.action, Recovery::Done);
    assert!(!outgoing.exists(), "committed leftover was not cleaned");
    assert_eq!(fs::read_to_string(app.join("marker")).unwrap(), "patched");
}

#[test]
fn legacy_unfinished_journal_without_proofs_fails_closed_before_live_change() {
    let root = scratch();
    let app = app_bundle(&root, "patched");
    let id = new_install_id();
    let original = root
        .join("transactions")
        .join(&id)
        .join("original/ChatGPT.app");
    fs::create_dir_all(&original).unwrap();
    fs::write(original.join("marker"), "original").unwrap();
    write_legacy(&root, &id, &app, "SWAPPED");

    let error = recover(&root, &id).expect_err("legacy incomplete recovery must refuse");
    assert!(error.to_string().contains("proof"), "{error}");
    assert_eq!(fs::read_to_string(app.join("marker")).unwrap(), "patched");
    assert!(original.exists(), "fail-closed recovery touched the backup");
}
