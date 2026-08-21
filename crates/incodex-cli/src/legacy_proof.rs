//! Read-only proof gate for a committed TypeScript CLI v1 installation.
//!
//! 结构读取器只回答“记录长什么样”；本模块回答“当前磁盘对象仍然是它
//! 所描述的对象吗”。证明期间持有 target lock，并在关键边界重新检查身份。

use std::fmt;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use incodex_asar::{Archive, PackageMain, MARKER_KEY};
use incodex_core::{inspect_target, recheck_target, CanonicalTarget};
use incodex_macos::{read_architecture, read_plist_info, verify_app, PlistInfo};
use incodex_transaction::{
    acquire_target_lock, adopt_legacy_committed_locked, JournalV2, LegacyMigrationInput, TargetLock,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::legacy_typescript::{
    LegacyFsIdentity, LegacyState, LegacyStructuralState, LegacyTargetIdentity,
};

const VENDOR_TEAM_IDENTIFIER: &str = "2DC432GLL2";
const OFFICIAL_BUNDLE_IDENTIFIER: &str = "com.openai.codex";

/// 证明后才可交给迁移器的状态。结构状态本身不实现任何 mutation 能力。
pub struct LegacyProvenState {
    structural: LegacyStructuralState,
    evidence: LegacyProofEvidence,
    /// Lock remains owned until the proven state is consumed by migration.
    #[allow(dead_code)]
    lock: TargetLock,
}

impl fmt::Debug for LegacyProvenState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LegacyProvenState")
            .field("structural", &self.structural)
            .field("evidence", &self.evidence)
            .finish_non_exhaustive()
    }
}

impl LegacyProvenState {
    /// Migration consumers run while this value still owns the target lock.
    pub fn with_locked<R>(
        &self,
        consumer: impl FnOnce(&LegacyStructuralState, &LegacyProofEvidence) -> R,
    ) -> R {
        consumer(&self.structural, &self.evidence)
    }

    /// Consume the proof while its target lock remains held and adopt its
    /// verified backup as a native committed transaction.
    pub fn migrate(self, root: &Path) -> Result<JournalV2, String> {
        let LegacyProvenState {
            structural,
            evidence,
            lock,
        } = self;
        let target = structural
            .target_identity
            .ok_or("legacy proof has no target identity")?;
        let original_source = match &structural.state {
            LegacyState::Committed { original_app, .. } => original_app.clone(),
            _ => return Err("legacy proof is not committed".into()),
        };
        let input = LegacyMigrationInput {
            install_id: structural.install_id,
            requested_path: PathBuf::from(&structural.journal.target_real_path),
            real_path: structural.target_real_path,
            target_device: target.target.device,
            target_inode: target.target.inode,
            parent_device: target.parent.device,
            parent_inode: target.parent.inode,
            original_source,
            live_asar_file_hash: evidence.live_asar_file_hash,
            original_asar_file_hash: evidence.original_asar_file_hash,
            original_plist_file_hash: evidence.original_plist_file_hash,
        };
        let journal = adopt_legacy_committed_locked(root, &lock, &input)?;
        let _ = evidence;
        Ok(journal)
    }
}

/// 迁移器可审计的 live、backup 与 vendor 证据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyProofEvidence {
    pub target: LegacyFsIdentity,
    pub parent: LegacyFsIdentity,
    pub original_backup: LegacyFsIdentity,
    pub live_install_id: String,
    pub live_asar_header_hash: String,
    pub live_asar_file_hash: String,
    pub original_asar_header_hash: String,
    pub original_asar_file_hash: String,
    pub original_plist_file_hash: String,
    pub vendor_signature: Option<LegacyVendorSignature>,
}

/// 官方 app 的签名身份；非官方 clone 不填充这项 vendor 证据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyVendorSignature {
    pub identifier: String,
    pub team_identifier: String,
    pub authorities: Vec<String>,
}

/// 在 target lock 内完成一次完整 proof。
pub fn prove_legacy_ts_v1(
    root: &Path,
    structural: LegacyStructuralState,
) -> Result<LegacyProvenState, String> {
    prove_legacy_ts_v1_with_boundaries(root, structural, || Ok(()), || Ok(()))
}

/// 测试用的锁内 checkpoint；产品调用方应使用不注入 callback 的入口。
#[doc(hidden)]
pub fn prove_legacy_ts_v1_with_checkpoint<F>(
    root: &Path,
    structural: LegacyStructuralState,
    after_lock: F,
) -> Result<LegacyProvenState, String>
where
    F: FnOnce() -> Result<(), String>,
{
    prove_legacy_ts_v1_with_boundaries(root, structural, after_lock, || Ok(()))
}

/// 测试用的最终边界 checkpoint；产品调用方应使用不注入 callback 的入口。
#[doc(hidden)]
pub fn prove_legacy_ts_v1_with_boundaries<F, G>(
    root: &Path,
    structural: LegacyStructuralState,
    after_lock: F,
    after_initial_evidence: G,
) -> Result<LegacyProvenState, String>
where
    F: FnOnce() -> Result<(), String>,
    G: FnOnce() -> Result<(), String>,
{
    let (manifest, runtime, original_app) = match &structural.state {
        LegacyState::Committed {
            manifest,
            runtime,
            original_app,
            ..
        } => (manifest, runtime, original_app),
        LegacyState::Interrupted => {
            return Err(
                "legacy structural state is interrupted; proof requires committed metadata".into(),
            )
        }
        LegacyState::RolledBack => {
            return Err(
                "legacy structural state is rolled back; proof requires committed metadata".into(),
            )
        }
    };
    if structural.install_id != manifest.install_id || structural.install_id != runtime.install_id {
        return Err("legacy structural installId does not match committed metadata".into());
    }
    let expected_target = structural
        .target_identity
        .ok_or("legacy structural state has no target identity")?;
    let expected_original = structural
        .original_identity
        .ok_or("legacy structural state has no original backup identity")?;

    let target = &structural.target_real_path;
    ensure_no_symlink_path(target, "legacy live target")?;
    let before_lock = inspect_target(target, None)
        .map_err(|error| format!("cannot inspect legacy live target: {error}"))?;
    assert_target_identity(&before_lock, &expected_target, "before target lock")?;
    if before_lock.is_official && manifest.bundle_identifier != OFFICIAL_BUNDLE_IDENTIFIER {
        return Err("official target bundle identifier is not com.openai.codex".into());
    }
    let lock = acquire_target_lock(root, target, "legacy-proof", Some(&structural.install_id))?;
    recheck_target(&before_lock)
        .map_err(|error| format!("legacy live target changed at lock boundary: {error}"))?;
    after_lock()?;
    recheck_target(&before_lock)
        .map_err(|error| format!("legacy live target changed during proof: {error}"))?;
    ensure_no_symlink_path(target, "legacy live target")?;

    let live_info = require_plist(target, "live target", None)?;
    check_identity_fields(
        &live_info,
        &manifest.bundle_identifier,
        &manifest.app_version,
        &manifest.app_build,
        "live target",
    )?;
    let _live_executable = executable_path(target, &live_info, "live target", None)?;
    let live_architecture = read_architecture(target, &live_info.executable)
        .ok_or("legacy live target architecture is unreadable")?;
    if live_architecture != manifest.architecture {
        return Err(format!(
            "live target architecture mismatch: expected {}, got {}",
            manifest.architecture, live_architecture
        ));
    }
    let live_plist = plist_path(target, "live target", None)?;
    let live_plist_hash = sha256_file(&live_plist)?;
    let live_asar_path = asar_path(target, "live target", None)?;
    let live_asar_identity = object_identity(&live_asar_path, "live target ASAR")?;
    let live_archive = Archive::open(&live_asar_path)
        .map_err(|error| format!("cannot read live target ASAR: {error}"))?;
    let (live_package, live_package_json) = package_metadata(&live_archive, "live target")?;
    require_live_marker(
        &live_package_json,
        &structural.install_id,
        &manifest.original_main,
    )?;
    if !live_package.already_patched
        || live_package.install_id.as_deref() != Some(&structural.install_id)
    {
        return Err("live target marker is not bound to the legacy installId".into());
    }
    if live_package.main != manifest.original_main {
        return Err("live target original main does not match the legacy manifest".into());
    }
    if live_archive.header_hash() != manifest.patched_asar_header_hash {
        return Err("live target patched ASAR header hash mismatch".into());
    }
    if live_archive.file_hash() != manifest.patched_asar_file_hash {
        return Err("live target patched ASAR file hash mismatch".into());
    }
    if before_lock.is_official {
        verify_bundle_deep_strict(target, "official live target")?;
    } else if !verify_app(target) {
        return Err("live target signature verification failed".into());
    }
    if object_identity(&live_asar_path, "live target ASAR")? != live_asar_identity {
        return Err("live target ASAR identity changed during proof".into());
    }

    ensure_no_symlink_under(root, original_app, "legacy original backup")?;
    assert_directory(original_app, "legacy original backup")?;
    let original_identity = object_identity(original_app, "legacy original backup")?;
    if original_identity != expected_original {
        return Err("legacy original backup identity changed since structural read".into());
    }
    let original_info = require_plist(original_app, "legacy original backup", Some(root))?;
    check_identity_fields(
        &original_info,
        &manifest.bundle_identifier,
        &manifest.app_version,
        &manifest.app_build,
        "legacy original backup",
    )?;
    let _original_executable = executable_path(
        original_app,
        &original_info,
        "legacy original backup",
        Some(root),
    )?;
    let original_architecture = read_architecture(original_app, &original_info.executable)
        .ok_or("legacy original backup architecture is unreadable")?;
    if original_architecture != manifest.architecture {
        return Err(format!(
            "legacy original backup architecture mismatch: expected {}, got {}",
            manifest.architecture, original_architecture
        ));
    }
    let original_plist = original_app.join("Contents/Info.plist");
    ensure_no_symlink_under(root, &original_plist, "legacy original backup Info.plist")?;
    let original_plist_hash = sha256_file(&original_plist)?;
    if original_plist_hash != manifest.original_plist_file_hash {
        return Err("legacy original backup plist hash mismatch".into());
    }
    let original_asar_path = asar_path(original_app, "legacy original backup", Some(root))?;
    let original_asar_identity = object_identity(&original_asar_path, "legacy original ASAR")?;
    let original_archive = Archive::open(&original_asar_path)
        .map_err(|error| format!("cannot read legacy original backup ASAR: {error}"))?;
    let (original_package, original_package_json) =
        package_metadata(&original_archive, "legacy original backup")?;
    if original_package_json.get(MARKER_KEY).is_some() || original_package.already_patched {
        return Err("legacy original backup contains an Incodex marker".into());
    }
    if original_package.install_id.is_some() || original_package.main != manifest.original_main {
        return Err(
            "legacy original backup package metadata does not match the clean manifest".into(),
        );
    }
    if original_archive.header_hash() != manifest.original_asar_header_hash {
        return Err("legacy original ASAR header hash mismatch".into());
    }
    if original_archive.file_hash() != manifest.original_asar_file_hash {
        return Err("legacy original ASAR file hash mismatch".into());
    }
    if !verify_app(original_app) {
        return Err("legacy original backup signature verification failed".into());
    }
    let vendor_signature = if before_lock.is_official {
        Some(verify_official_vendor_bundle(
            original_app,
            &manifest.bundle_identifier,
        )?)
    } else {
        None
    };

    after_initial_evidence()?;

    ensure_no_symlink_under(root, original_app, "legacy original backup")?;
    ensure_no_symlink_under(root, &original_asar_path, "legacy original ASAR")?;
    if object_identity(original_app, "legacy original backup")? != expected_original {
        return Err("legacy original backup identity changed during proof".into());
    }
    if object_identity(&original_asar_path, "legacy original ASAR")? != original_asar_identity {
        return Err("legacy original ASAR identity changed during proof".into());
    }
    let final_live_info = require_plist(target, "final live target", None)
        .map_err(|error| format!("final live identity proof failed: {error}"))?;
    check_identity_fields(
        &final_live_info,
        &manifest.bundle_identifier,
        &manifest.app_version,
        &manifest.app_build,
        "final live target",
    )?;
    let _ = executable_path(target, &final_live_info, "final live target", None)?;
    let final_live_architecture = read_architecture(target, &final_live_info.executable)
        .ok_or("final live target architecture is unreadable")?;
    if final_live_architecture != manifest.architecture {
        return Err("final live target architecture mismatch".into());
    }
    if sha256_file(&plist_path(target, "final live target", None)?)? != live_plist_hash {
        return Err("final live target Info.plist hash mismatch".into());
    }
    let final_live_asar_path = asar_path(target, "final live target", None)?;
    let final_live_archive = Archive::open(&final_live_asar_path)
        .map_err(|error| format!("final live ASAR proof failed: {error}"))?;
    let (final_live_package, final_live_package_json) =
        package_metadata(&final_live_archive, "final live target")?;
    require_live_marker(
        &final_live_package_json,
        &structural.install_id,
        &manifest.original_main,
    )?;
    if !final_live_package.already_patched
        || final_live_package.install_id.as_deref() != Some(&structural.install_id)
        || final_live_package.main != manifest.original_main
    {
        return Err("final live target marker identity mismatch".into());
    }
    if final_live_archive.header_hash() != manifest.patched_asar_header_hash
        || final_live_archive.file_hash() != manifest.patched_asar_file_hash
    {
        return Err("final live target patched ASAR hash mismatch".into());
    }
    let final_original_info = require_plist(original_app, "final original backup", Some(root))
        .map_err(|error| format!("final original identity proof failed: {error}"))?;
    check_identity_fields(
        &final_original_info,
        &manifest.bundle_identifier,
        &manifest.app_version,
        &manifest.app_build,
        "final original backup",
    )?;
    let _ = executable_path(
        original_app,
        &final_original_info,
        "final original backup",
        Some(root),
    )?;
    let final_original_architecture =
        read_architecture(original_app, &final_original_info.executable)
            .ok_or("final original backup architecture is unreadable")?;
    if final_original_architecture != manifest.architecture {
        return Err("final original backup architecture mismatch".into());
    }
    if sha256_file(&plist_path(
        original_app,
        "final original backup",
        Some(root),
    )?)? != original_plist_hash
    {
        return Err("final original backup Info.plist hash mismatch".into());
    }
    let final_original_asar_path = asar_path(original_app, "final original backup", Some(root))?;
    let final_original_archive = Archive::open(&final_original_asar_path)
        .map_err(|error| format!("final original ASAR proof failed: {error}"))?;
    let (final_original_package, final_original_package_json) =
        package_metadata(&final_original_archive, "final original backup")?;
    if final_original_package_json.get(MARKER_KEY).is_some()
        || final_original_package.already_patched
        || final_original_package.install_id.is_some()
        || final_original_package.main != manifest.original_main
    {
        return Err("final original backup clean marker identity mismatch".into());
    }
    if final_original_archive.header_hash() != manifest.original_asar_header_hash
        || final_original_archive.file_hash() != manifest.original_asar_file_hash
    {
        return Err("final original backup ASAR hash mismatch".into());
    }
    if before_lock.is_official {
        verify_bundle_deep_strict(target, "final official live target")?;
        let _ = verify_official_vendor_bundle(original_app, &manifest.bundle_identifier)?;
    } else {
        if !verify_app(target) {
            return Err("final live target signature verification failed".into());
        }
        if !verify_app(original_app) {
            return Err("final original backup signature verification failed".into());
        }
    }
    ensure_no_symlink_path(target, "legacy live target")?;
    ensure_no_symlink_path(&live_asar_path, "live target ASAR")?;
    if object_identity(&live_asar_path, "live target ASAR")? != live_asar_identity {
        return Err("live target ASAR identity changed after proof".into());
    }
    recheck_target(&before_lock)
        .map_err(|error| format!("legacy live target changed after proof: {error}"))?;
    let live_install_id = structural.install_id.clone();
    let live_asar_header_hash = manifest.patched_asar_header_hash.clone();
    let live_asar_file_hash = manifest.patched_asar_file_hash.clone();
    let original_asar_header_hash = manifest.original_asar_header_hash.clone();
    let original_asar_file_hash = manifest.original_asar_file_hash.clone();
    let original_plist_file_hash = manifest.original_plist_file_hash.clone();

    Ok(LegacyProvenState {
        structural,
        evidence: LegacyProofEvidence {
            target: expected_target.target,
            parent: expected_target.parent,
            original_backup: expected_original,
            live_install_id,
            live_asar_header_hash,
            live_asar_file_hash,
            original_asar_header_hash,
            original_asar_file_hash,
            original_plist_file_hash,
            vendor_signature,
        },
        lock,
    })
}

fn assert_target_identity(
    actual: &CanonicalTarget,
    expected: &LegacyTargetIdentity,
    boundary: &str,
) -> Result<(), String> {
    if actual.target_device != expected.target.device
        || actual.target_inode != expected.target.inode
    {
        return Err(format!("legacy target inode/device mismatch {boundary}"));
    }
    if actual.parent_device != expected.parent.device
        || actual.parent_inode != expected.parent.inode
    {
        return Err(format!(
            "legacy target parent inode/device mismatch {boundary}"
        ));
    }
    Ok(())
}

fn object_identity(path: &Path, label: &str) -> Result<LegacyFsIdentity, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("cannot inspect {label}: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{label} is a symlink: {}", path.display()));
    }
    Ok(LegacyFsIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

fn require_plist(app: &Path, label: &str, root: Option<&Path>) -> Result<PlistInfo, String> {
    let _ = plist_path(app, label, root)?;
    read_plist_info(app).ok_or_else(|| format!("{label} Info.plist is unreadable"))
}

fn plist_path(app: &Path, label: &str, root: Option<&Path>) -> Result<PathBuf, String> {
    let path = app.join("Contents/Info.plist");
    if let Some(root) = root {
        ensure_no_symlink_under(root, &path, &format!("{label} Info.plist"))?;
    } else {
        ensure_no_symlink_path(&path, &format!("{label} Info.plist"))?;
    }
    Ok(path)
}

fn executable_path(
    app: &Path,
    info: &PlistInfo,
    label: &str,
    root: Option<&Path>,
) -> Result<PathBuf, String> {
    if info.executable.is_empty() {
        return Err(format!("{label} executable identity is empty"));
    }
    let path = app.join("Contents/MacOS").join(&info.executable);
    if let Some(root) = root {
        ensure_no_symlink_under(root, &path, &format!("{label} executable"))?;
    } else {
        ensure_no_symlink_path(&path, &format!("{label} executable"))?;
    }
    Ok(path)
}

fn check_identity_fields(
    info: &PlistInfo,
    bundle_identifier: &str,
    app_version: &str,
    app_build: &str,
    label: &str,
) -> Result<(), String> {
    if info.bundle_identifier != bundle_identifier {
        return Err(format!("{label} bundleIdentifier mismatch"));
    }
    if info.app_version != app_version {
        return Err(format!("{label} appVersion mismatch"));
    }
    if info.app_build != app_build {
        return Err(format!("{label} appBuild mismatch"));
    }
    Ok(())
}

fn asar_path(app: &Path, label: &str, root: Option<&Path>) -> Result<PathBuf, String> {
    let path = app.join("Contents/Resources/app.asar");
    if let Some(root) = root {
        ensure_no_symlink_under(root, &path, &format!("{label} ASAR"))?;
    } else {
        ensure_no_symlink_path(&path, &format!("{label} ASAR"))?;
    }
    Ok(path)
}

fn package_metadata(archive: &Archive, label: &str) -> Result<(PackageMain, Value), String> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let package = archive
            .read_package_main()
            .map_err(|error| format!("{label} package metadata is unreadable: {error}"))?;
        let raw = serde_json::from_slice(
            &archive
                .extract("package.json")
                .map_err(|error| format!("{label} package.json is unreadable: {error}"))?,
        )
        .map_err(|error| format!("{label} package.json is invalid: {error}"))?;
        Ok::<_, String>((package, raw))
    }));
    match result {
        Ok(value) => value,
        Err(_) => Err(format!("{label} package metadata is malformed")),
    }
}

fn require_live_marker(
    package: &Value,
    expected_install_id: &str,
    expected_main: &str,
) -> Result<(), String> {
    let marker = package
        .get(MARKER_KEY)
        .and_then(Value::as_object)
        .ok_or("live target is missing the Incodex marker")?;
    if marker.get("installId").and_then(Value::as_str) != Some(expected_install_id) {
        return Err("live target marker installId mismatch".into());
    }
    if marker.get("originalMain").and_then(Value::as_str) != Some(expected_main) {
        return Err("live target marker originalMain mismatch".into());
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot hash {}: {error}", path.display()))?;
    Ok(hex_digest(&bytes))
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn verify_bundle_deep_strict(app: &Path, label: &str) -> Result<(), String> {
    let output = Command::new("codesign")
        .args(["--verify", "--deep", "--strict", "--verbose=4", "--"])
        .arg(app)
        .output()
        .map_err(|error| format!("{label} verification failed to start: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("{label} deep strict signature verification failed"))
    }
}

/// Verify official vendor identity; an ad-hoc fixture is never accepted.
#[doc(hidden)]
pub fn verify_official_vendor_bundle(
    app: &Path,
    expected_identifier: &str,
) -> Result<LegacyVendorSignature, String> {
    verify_bundle_deep_strict(app, "official vendor bundle")?;
    let output = Command::new("codesign")
        .args(["--display", "--verbose=4", "--"])
        .arg(app)
        .output()
        .map_err(|error| format!("cannot inspect vendor signature: {error}"))?;
    if !output.status.success() {
        return Err("official original vendor signature inspection failed".into());
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let identifier = signature_field(&text, "Identifier=");
    let team_identifier = signature_field(&text, "TeamIdentifier=");
    let authorities = text
        .lines()
        .filter_map(|line| line.strip_prefix("Authority=").map(str::to_string))
        .collect::<Vec<_>>();
    if identifier != expected_identifier {
        return Err("official original vendor bundle identifier mismatch".into());
    }
    if team_identifier != VENDOR_TEAM_IDENTIFIER {
        return Err("official original vendor team identifier mismatch".into());
    }
    if authorities.is_empty() || text.lines().any(|line| line.trim() == "Signature=adhoc") {
        return Err("official original vendor signature is ad hoc or incomplete".into());
    }
    Ok(LegacyVendorSignature {
        identifier,
        team_identifier,
        authorities,
    })
}

fn signature_field(text: &str, prefix: &str) -> String {
    text.lines()
        .find_map(|line| line.strip_prefix(prefix).map(str::trim).map(str::to_string))
        .unwrap_or_default()
}

fn assert_directory(path: &Path, label: &str) -> Result<(), String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("cannot inspect {label}: {error}"))?;
    if !metadata.is_dir() {
        return Err(format!("{label} is not a directory: {}", path.display()));
    }
    Ok(())
}

/// 不依赖 canonicalize 作为安全证明；逐级检查每个已经存在的组件。
fn ensure_no_symlink_path(path: &Path, label: &str) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!("{label} is not absolute: {}", path.display()));
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(format!(
                    "{label} contains a parent traversal: {}",
                    path.display()
                ))
            }
            Component::Normal(name) => current.push(name),
        }
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            format!(
                "cannot inspect {label} component {}: {error}",
                current.display()
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(format!("{label} contains a symlink: {}", current.display()));
        }
    }
    Ok(())
}

/// 对状态根内部的路径从 canonical root 开始检查，避免把 macOS 的
/// `/var -> /private/var` 系统别名误判为应用状态被替换。
fn ensure_no_symlink_under(root: &Path, path: &Path, label: &str) -> Result<(), String> {
    let root_metadata = fs::symlink_metadata(root)
        .map_err(|error| format!("cannot inspect legacy state root: {error}"))?;
    if root_metadata.file_type().is_symlink() {
        return Err(format!(
            "legacy state root is a symlink: {}",
            root.display()
        ));
    }
    if !root_metadata.is_dir() {
        return Err(format!(
            "legacy state root is not a directory: {}",
            root.display()
        ));
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| format!("{label} escaped the legacy state root: {}", path.display()))?;
    let mut current = fs::canonicalize(root)
        .map_err(|error| format!("cannot resolve legacy state root: {error}"))?;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(format!("{label} has an unsafe relative component"));
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            format!(
                "cannot inspect {label} component {}: {error}",
                current.display()
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(format!("{label} contains a symlink: {}", current.display()));
        }
    }
    Ok(())
}
