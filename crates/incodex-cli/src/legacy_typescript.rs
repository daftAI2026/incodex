//! Reader for the frozen TypeScript CLI v1 on-disk contract.
//!
//! This module deliberately does not execute the retired TypeScript CLI. It
//! only describes the files that an already-installed v1 client left behind,
//! so a future migration can validate that state before touching the target.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use incodex_core::{canonical_path, is_official_app, target_id};
use incodex_transaction::validate_path_ancestors;

pub const SCHEMA_VERSION: u32 = 1;

const PHASES: &[&str] = &[
    "DISCOVERED",
    "BACKUP_COMMITTED",
    "STAGED",
    "PATCHED",
    "SIGNED",
    "VERIFIED",
    "TARGET_MOVED_OUT",
    "SWAPPED",
    "TARGET_VERIFIED",
    "COMMITTED",
    "ROLLED_BACK",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyStateKind {
    Committed,
    Interrupted,
    RolledBack,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallManifest {
    pub schema_version: u32,
    pub install_id: String,
    pub target_real_path: String,
    pub bundle_identifier: String,
    pub app_version: String,
    pub app_build: String,
    pub architecture: String,
    pub original_asar_header_hash: String,
    pub original_asar_file_hash: String,
    pub original_plist_file_hash: String,
    pub patched_asar_header_hash: String,
    pub patched_asar_file_hash: String,
    pub original_main: String,
    pub runtime_version: String,
    pub created_at: String,
    pub transaction_state: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeManifest {
    pub install_id: String,
    pub original_main: String,
    pub patched_asar_header_hash: String,
    pub patched_asar_file_hash: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CurrentPointer {
    pub install_id: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TransactionJournal {
    pub schema_version: u32,
    pub install_id: String,
    pub target_real_path: String,
    pub staged_app: String,
    pub original_snapshot: String,
    #[serde(default)]
    pub outgoing_app: Option<String>,
    pub phase: String,
    pub updated_at: String,
}

/// The only state in which post-commit metadata is available. Interrupted and
/// rolled-back journals deliberately cannot carry a stale committed snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyState {
    Committed {
        current: CurrentPointer,
        manifest: InstallManifest,
        runtime: RuntimeManifest,
        original_app: PathBuf,
    },
    Interrupted,
    RolledBack,
}

impl LegacyState {
    pub fn kind(&self) -> LegacyStateKind {
        match self {
            Self::Committed { .. } => LegacyStateKind::Committed,
            Self::Interrupted => LegacyStateKind::Interrupted,
            Self::RolledBack => LegacyStateKind::RolledBack,
        }
    }
}

/// A structurally consistent view of one retired TS v1 journal. This is not
/// proof of the live target, backup contents, signature, hashes, or inode
/// identity; migration must add those target-lock and backup-proof checks
/// before acting on this record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyTsV1State {
    pub target_id: String,
    pub target_real_path: PathBuf,
    pub install_id: String,
    pub target_store: PathBuf,
    pub install_dir: PathBuf,
    pub state: LegacyState,
    pub journal: TransactionJournal,
}

#[derive(Debug, Clone)]
struct LegacyJournalRecord {
    journal: TransactionJournal,
    kind: LegacyStateKind,
}

#[derive(Debug, Clone)]
struct LegacyMetadata {
    current: CurrentPointer,
    manifest: InstallManifest,
    runtime: RuntimeManifest,
    original_app: PathBuf,
}

/// Read one target's legacy v1 state without invoking Bun or the old router.
///
/// The flat transaction journals are enumerated before installation metadata.
/// This is required because the retired writer creates its journal at
/// `DISCOVERED`, before `current.json`, the manifest, the runtime manifest, or
/// the original backup exists. Ok(None) means there is no target-matching
/// journal and no legacy installation store. An existing or malformed record
/// is an error, not a clean result: migration must never silently ignore a
/// damaged legacy state.
pub fn load_legacy_ts_v1(root: &Path, target: &Path) -> Result<Option<LegacyTsV1State>, String> {
    let target_real_path = canonical_path(target);
    let target_key = target_id(target);
    let target_store = root.join("installations").join(&target_key);
    validate_storage_path(root, &target_store, "legacy installation target directory")?;
    let journals = enumerate_target_journals(root, &target_real_path, &target_key)?;
    if journals.is_empty() {
        if storage_path_exists(&target_store, "legacy installation target directory")? {
            return Err(
                "legacy installation records exist without a matching transaction journal".into(),
            );
        }
        return Ok(None);
    }

    let selected = select_journal(root, &target_key, &journals)?;
    let state = match selected.kind {
        LegacyStateKind::Committed => {
            let metadata = load_metadata(root, &target_key, &target_real_path, &selected.journal)?;
            LegacyState::Committed {
                current: metadata.current,
                manifest: metadata.manifest,
                runtime: metadata.runtime,
                original_app: metadata.original_app,
            }
        }
        LegacyStateKind::Interrupted => LegacyState::Interrupted,
        LegacyStateKind::RolledBack => LegacyState::RolledBack,
    };
    let install_dir = target_store.join(&selected.journal.install_id);

    Ok(Some(LegacyTsV1State {
        target_id: target_key,
        target_real_path,
        install_id: selected.journal.install_id.clone(),
        target_store,
        install_dir,
        state,
        journal: selected.journal.clone(),
    }))
}

fn enumerate_target_journals(
    root: &Path,
    target_real_path: &Path,
    target_key: &str,
) -> Result<Vec<LegacyJournalRecord>, String> {
    let transactions = root.join("transactions");
    validate_storage_path(root, &transactions, "legacy transactions directory")?;
    let transactions_metadata = match fs::symlink_metadata(&transactions) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "cannot inspect legacy transactions directory: {error}"
            ))
        }
    };
    if !transactions_metadata.file_type().is_dir() {
        return Err(format!(
            "legacy transactions path is not a directory: {}",
            transactions.display()
        ));
    }

    let mut records = Vec::new();
    for entry in fs::read_dir(&transactions)
        .map_err(|error| format!("cannot enumerate legacy transactions: {error}"))?
    {
        let entry =
            entry.map_err(|error| format!("cannot read legacy transaction entry: {error}"))?;
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(|| {
            format!(
                "legacy transaction filename is not valid UTF-8: {}",
                entry.path().display()
            )
        })?;
        if !name.ends_with(".json") || name.ends_with(".tmp") {
            continue;
        }
        let path = entry.path();
        validate_storage_path(root, &path, "legacy transaction journal")?;
        let journal: TransactionJournal = read_json(&path, "legacy transaction journal")?;
        if canonical_path(&journal.target_real_path) != target_real_path {
            continue;
        }
        let filename_install_id = Path::new(name)
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("legacy transaction filename is invalid: {name}"))?;
        if filename_install_id != journal.install_id {
            return Err(format!(
                "legacy transaction filename does not match installId: {}",
                path.display()
            ));
        }
        let kind = validate_journal(root, &journal, target_key, target_real_path)?;
        records.push(LegacyJournalRecord { journal, kind });
    }
    Ok(records)
}

fn select_journal<'a>(
    root: &Path,
    target_key: &str,
    records: &'a [LegacyJournalRecord],
) -> Result<&'a LegacyJournalRecord, String> {
    let current_id = read_current_install_id(root, target_key)?;
    let interrupted = records
        .iter()
        .filter(|record| record.kind == LegacyStateKind::Interrupted)
        .collect::<Vec<_>>();
    if !interrupted.is_empty() {
        let mut interrupted_ids = interrupted
            .iter()
            .map(|record| record.journal.install_id.clone())
            .collect::<Vec<_>>();
        interrupted_ids.sort_unstable();
        if let Some(current_id) = current_id.as_deref() {
            return Err(format!(
                "ambiguous legacy journals for target: interrupted [{}] conflicts with current.json installId {current_id}; refusing to choose one",
                interrupted_ids.join(", ")
            ));
        }
        if interrupted.len() > 1 {
            return Err(format!(
                "multiple actionable interrupted legacy journals for target; refusing to choose one: {}",
                interrupted_ids.join(", ")
            ));
        }
        return Ok(interrupted[0]);
    }

    if let Some(current_id) = current_id {
        if let Some(record) = records.iter().find(|record| {
            record.kind == LegacyStateKind::Committed && record.journal.install_id == current_id
        }) {
            return Ok(record);
        }
        return Err(format!(
            "legacy current.json points to installId {current_id}, but no matching committed journal exists; refusing to fall back"
        ));
    }
    if let Some(record) = newest_record(records, LegacyStateKind::Committed) {
        return Ok(record);
    }
    newest_record(records, LegacyStateKind::RolledBack)
        .ok_or_else(|| "legacy transaction journal set is empty".into())
}

fn newest_record(
    records: &[LegacyJournalRecord],
    kind: LegacyStateKind,
) -> Option<&LegacyJournalRecord> {
    records
        .iter()
        .filter(|record| record.kind == kind)
        .max_by(|left, right| {
            left.journal
                .updated_at
                .cmp(&right.journal.updated_at)
                .then_with(|| left.journal.install_id.cmp(&right.journal.install_id))
        })
}

fn read_current_install_id(root: &Path, target_key: &str) -> Result<Option<String>, String> {
    let target_store = root.join("installations").join(target_key);
    validate_storage_path(root, &target_store, "legacy installation target directory")?;
    if !storage_path_exists(&target_store, "legacy installation target directory")? {
        return Ok(None);
    }
    let current_path = target_store.join("current.json");
    validate_storage_path(root, &current_path, "legacy current.json")?;
    if !storage_path_exists(&current_path, "legacy current.json")? {
        return Ok(None);
    }
    let current: CurrentPointer = read_json(&current_path, "legacy current.json")?;
    validate_install_id(&current.install_id, "current installId")?;
    Ok(Some(current.install_id))
}

fn load_metadata(
    root: &Path,
    target_key: &str,
    target_real_path: &Path,
    journal: &TransactionJournal,
) -> Result<LegacyMetadata, String> {
    let target_store = root.join("installations").join(target_key);
    let install_dir = target_store.join(&journal.install_id);
    let current_path = target_store.join("current.json");
    let manifest_path = install_dir.join("manifest.json");
    let runtime_path = install_dir.join("patched/runtime-manifest.json");
    let original_app = install_dir.join("original/ChatGPT.app");
    for (path, label) in [
        (&target_store, "legacy installation target directory"),
        (&install_dir, "legacy installation directory"),
        (&current_path, "legacy current.json"),
        (&manifest_path, "legacy manifest.json"),
        (&runtime_path, "legacy runtime-manifest.json"),
        (&original_app, "legacy original backup"),
    ] {
        validate_storage_path(root, path, label)?;
    }

    let paths = [
        (&current_path, "legacy current.json"),
        (&manifest_path, "legacy manifest.json"),
        (&runtime_path, "legacy runtime-manifest.json"),
        (&original_app, "legacy original backup"),
    ];
    let present = paths
        .iter()
        .map(|(path, label)| storage_path_exists(path, label))
        .collect::<Result<Vec<_>, _>>()?;
    if !present.iter().all(|value| *value) {
        if present.iter().all(|value| !*value) {
            return Err("committed legacy journal has no installation metadata".into());
        }
        return Err("committed legacy journal has incomplete installation metadata".into());
    }

    let current: CurrentPointer = read_json(&current_path, "legacy current.json")?;
    validate_install_id(&current.install_id, "current installId")?;
    if current.install_id != journal.install_id {
        return Err("legacy current installId does not match the selected journal".into());
    }
    let manifest: InstallManifest = read_json(&manifest_path, "legacy manifest.json")?;
    validate_manifest(&manifest, &journal.install_id, target_real_path)?;
    let runtime: RuntimeManifest = read_json(&runtime_path, "legacy runtime-manifest.json")?;
    validate_runtime(&runtime, &manifest)?;
    if !original_app.is_dir() {
        return Err(format!(
            "legacy original backup is missing: {}",
            original_app.display()
        ));
    }
    Ok(LegacyMetadata {
        current,
        manifest,
        runtime,
        original_app,
    })
}

fn storage_path_exists(path: &Path, label: &str) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("cannot inspect {label}: {error}")),
    }
}

fn validate_manifest(
    manifest: &InstallManifest,
    expected_id: &str,
    target_real_path: &Path,
) -> Result<(), String> {
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported legacy manifest schema: {}",
            manifest.schema_version
        ));
    }
    validate_install_id(&manifest.install_id, "manifest installId")?;
    if manifest.install_id != expected_id {
        return Err("legacy manifest installId does not match current.json".into());
    }
    if manifest.transaction_state != "committed" {
        return Err(format!(
            "legacy manifest transactionState is not committed: {}",
            manifest.transaction_state
        ));
    }
    if canonical_path(&manifest.target_real_path) != target_real_path {
        return Err("legacy manifest targetRealPath does not match the target".into());
    }
    for (name, value) in [
        ("bundleIdentifier", &manifest.bundle_identifier),
        ("appVersion", &manifest.app_version),
        ("appBuild", &manifest.app_build),
        ("architecture", &manifest.architecture),
        (
            "originalAsarHeaderHash",
            &manifest.original_asar_header_hash,
        ),
        ("originalAsarFileHash", &manifest.original_asar_file_hash),
        ("originalPlistFileHash", &manifest.original_plist_file_hash),
        ("patchedAsarHeaderHash", &manifest.patched_asar_header_hash),
        ("patchedAsarFileHash", &manifest.patched_asar_file_hash),
        ("originalMain", &manifest.original_main),
        ("runtimeVersion", &manifest.runtime_version),
        ("createdAt", &manifest.created_at),
    ] {
        if value.is_empty() {
            return Err(format!("legacy manifest field is empty: {name}"));
        }
    }
    Ok(())
}

fn validate_runtime(runtime: &RuntimeManifest, manifest: &InstallManifest) -> Result<(), String> {
    validate_install_id(&runtime.install_id, "runtime manifest installId")?;
    if runtime.install_id != manifest.install_id {
        return Err("legacy runtime manifest installId does not match manifest".into());
    }
    if runtime.original_main != manifest.original_main {
        return Err("legacy runtime manifest originalMain does not match manifest".into());
    }
    if runtime.patched_asar_header_hash != manifest.patched_asar_header_hash {
        return Err("legacy runtime manifest ASAR header hash does not match manifest".into());
    }
    if runtime.patched_asar_file_hash != manifest.patched_asar_file_hash {
        return Err("legacy runtime manifest ASAR file hash does not match manifest".into());
    }
    Ok(())
}

fn validate_journal(
    root: &Path,
    journal: &TransactionJournal,
    target_key: &str,
    target_real_path: &Path,
) -> Result<LegacyStateKind, String> {
    if journal.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported legacy transaction schema: {}",
            journal.schema_version
        ));
    }
    validate_install_id(&journal.install_id, "journal installId")?;
    if canonical_path(&journal.target_real_path) != target_real_path {
        return Err("legacy journal targetRealPath does not match the target".into());
    }
    if journal.staged_app.is_empty() || journal.original_snapshot.is_empty() {
        return Err("legacy journal path is empty".into());
    }
    let staged_app = Path::new(&journal.staged_app);
    validate_storage_path(root, staged_app, "legacy stagedApp")?;
    validate_emitted_staged_path(root, staged_app, &journal.install_id, target_real_path)?;
    validate_storage_path(
        root,
        Path::new(&journal.original_snapshot),
        "legacy originalSnapshot",
    )?;
    let expected_original = root
        .join("installations")
        .join(target_key)
        .join(&journal.install_id)
        .join("original/ChatGPT.app");
    if Path::new(&journal.original_snapshot) != expected_original {
        return Err("legacy journal originalSnapshot is not the emitted backup path".into());
    }
    if let Some(outgoing) = &journal.outgoing_app {
        if outgoing.is_empty() {
            return Err("legacy journal outgoingApp is empty".into());
        }
        let outgoing_path = Path::new(outgoing);
        validate_storage_path(root, outgoing_path, "legacy outgoingApp")?;
        validate_emitted_outgoing_path(root, outgoing_path, &journal.install_id)?;
    }
    if !PHASES.contains(&journal.phase.as_str()) {
        return Err(format!(
            "unsupported legacy journal phase: {}",
            journal.phase
        ));
    }
    if journal.updated_at.is_empty() {
        return Err("legacy journal updatedAt is empty".into());
    }
    Ok(match journal.phase.as_str() {
        "COMMITTED" => LegacyStateKind::Committed,
        "ROLLED_BACK" => LegacyStateKind::RolledBack,
        _ => LegacyStateKind::Interrupted,
    })
}

fn validate_emitted_staged_path(
    root: &Path,
    path: &Path,
    install_id: &str,
    target_real_path: &Path,
) -> Result<(), String> {
    let live_path = root.join("ChatGPT.app.live");
    let clone_path = root
        .join("scratch")
        .join(format!("ChatGPT.app.staged-{install_id}"));
    let expected = if is_official_app(target_real_path, None) {
        live_path
    } else {
        clone_path
    };
    if path == expected {
        return Ok(());
    }
    Err(format!(
        "legacy stagedApp is not the emitted TypeScript v1 staging path for this target: {}",
        path.display()
    ))
}

fn validate_emitted_outgoing_path(
    root: &Path,
    path: &Path,
    install_id: &str,
) -> Result<(), String> {
    let expected = root
        .join("transactions")
        .join(install_id)
        .join("outgoing")
        .join("ChatGPT.app");
    if path == expected {
        return Ok(());
    }
    Err(format!(
        "legacy outgoingApp is not an emitted TypeScript v1 transaction path: {}",
        path.display()
    ))
}

fn validate_install_id(value: &str, field: &str) -> Result<(), String> {
    if !is_uuid(value) {
        return Err(format!("{field} is not an RFC 4122 UUID"));
    }
    Ok(())
}

fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    let is_hex = |byte: u8| byte.is_ascii_hexdigit();
    [8, 13, 18, 23].iter().all(|index| bytes[*index] == b'-')
        && (0..36).all(|index| [8, 13, 18, 23].contains(&index) || is_hex(bytes[index]))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, label: &str) -> Result<T, String> {
    let body = fs::read_to_string(path).map_err(|error| format!("{label} unreadable: {error}"))?;
    serde_json::from_str(&body).map_err(|error| format!("{label} invalid: {error}"))
}

fn reject_symlink(path: &Path, label: &str) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("{label} is a symlink: {}", path.display()))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot inspect {label}: {error}")),
    }
}

fn validate_storage_path(root: &Path, path: &Path, label: &str) -> Result<(), String> {
    reject_symlink(root, "legacy state root")?;
    let relative = path
        .strip_prefix(root)
        .map_err(|_| format!("{label} escaped the legacy state root: {}", path.display()))?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "{label} is not a safe relative path under the legacy state root: {}",
            path.display()
        ));
    }
    let relative = relative.to_str().ok_or_else(|| {
        format!(
            "{label} is not valid UTF-8 under the legacy state root: {}",
            path.display()
        )
    })?;
    validate_path_ancestors(root, relative)
        .map_err(|error| format!("{label} ancestor validation failed: {error}"))?;
    reject_symlink(path, label)
}
