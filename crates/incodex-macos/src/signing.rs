//! 签名验收、vendor sidecar 策略与 entitlement 处理。
//!
//! 这里是 install、uninstall 和 Doctor 共用的唯一签名判断入口：
//! - mutation 路径 fail closed；
//! - Doctor 区分“检查出损坏”和“无法检查”；
//! - vendor sidecar 按签名身份识别，不按文件名猜测。

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::entitlements::add_entitlement_key;
use super::signature_inspection::{has_identity_evidence, inspect_codesign};
use super::{read_plist_info, PlistInfo};

pub const VENDOR_TEAM_IDENTIFIER: &str = "2DC432GLL2";
pub const OFFICIAL_BUNDLE_IDENTIFIER: &str = "com.openai.codex";

const ADHOC_UNRETAINABLE_ENTITLEMENTS: &[&str] = &[
    "com.apple.developer.team-identifier",
    "com.apple.application-identifier",
    "com.apple.developer.aps-environment",
    "com.apple.security.application-groups",
    "keychain-access-groups",
];

const DISABLE_LIBRARY_VALIDATION: &str = "com.apple.security.cs.disable-library-validation";
const SPARKLE_FRAMEWORK: &str = "Contents/Frameworks/Sparkle.framework";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntitlementSnapshot {
    pub xml: String,
    pub keys: BTreeSet<String>,
}

impl EntitlementSnapshot {
    fn empty() -> Self {
        Self {
            xml: String::new(),
            keys: BTreeSet::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntitlementPlan {
    pub xml: String,
    pub source_keys: BTreeSet<String>,
    pub retained_keys: BTreeSet<String>,
    pub stripped_keys: BTreeSet<String>,
    pub added_keys: BTreeSet<String>,
    pub used_fallback: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureKind {
    Vendor,
    Adhoc,
    Other,
    Unsigned,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SigningPolicy {
    Official,
    Generic,
}

impl SignatureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vendor => "vendor",
            Self::Adhoc => "adhoc",
            Self::Other => "other",
            Self::Unsigned => "unsigned",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedComponent {
    pub path: PathBuf,
    pub identifier: Option<String>,
    pub team_identifier: Option<String>,
    pub authorities: Vec<String>,
    pub kind: SignatureKind,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningInventory {
    pub outer: SignedComponent,
    pub nested: Vec<SignedComponent>,
    pub entitlements: EntitlementSnapshot,
    pub deep_strict: bool,
}

/// 读取签名声明的 entitlement；命令失败与“没有 entitlement”必须区分。
pub fn read_entitlements(target: &Path) -> Result<EntitlementSnapshot, String> {
    let output = Command::new("codesign")
        .args(["--display", "--entitlements", ":-", "--"])
        .arg(target)
        .output()
        .map_err(|error| {
            format!(
                "cannot inspect entitlements for {}: {error}",
                target.display()
            )
        })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if detail.contains("code object is not signed at all") {
            // 未签名的自定义 target 没有可继承的 entitlement；这不是检查失败。
            // 其他错误仍然 fail closed，禁止回退到猜测的宽权限集合。
            return Ok(EntitlementSnapshot::empty());
        }
        return Err(format!(
            "entitlement inspection failed for {}{}",
            target.display(),
            if detail.is_empty() {
                String::new()
            } else {
                format!(": {detail}")
            }
        ));
    }
    let xml = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if xml.is_empty() {
        return Ok(EntitlementSnapshot::empty());
    }
    if !xml.contains("<plist") {
        return Err(format!(
            "entitlement inspection returned non-plist output for {}",
            target.display()
        ));
    }
    let keys = parse_entitlement_keys(&xml)?;
    Ok(EntitlementSnapshot { xml, keys })
}

/// 以最小必要策略生成 ad-hoc 外层 entitlement，不再猜测宽权限集合。
pub fn plan_adhoc_entitlements(source: &EntitlementSnapshot) -> Result<EntitlementPlan, String> {
    let source_keys = source.keys.clone();
    let stripped_keys: BTreeSet<_> = source
        .keys
        .iter()
        .filter(|key| ADHOC_UNRETAINABLE_ENTITLEMENTS.contains(&key.as_str()))
        .cloned()
        .collect();
    let retained_keys: BTreeSet<_> = source.keys.difference(&stripped_keys).cloned().collect();

    let mut xml = if source.xml.is_empty() {
        empty_entitlements_xml()
    } else {
        strip_unretainable_entitlements(&source.xml, &stripped_keys)?
    };
    let mut added_keys = BTreeSet::new();
    if !retained_keys.contains(DISABLE_LIBRARY_VALIDATION) {
        xml = add_entitlement_key(&xml, DISABLE_LIBRARY_VALIDATION)?;
        added_keys.insert(DISABLE_LIBRARY_VALIDATION.to_string());
    }
    Ok(EntitlementPlan {
        xml,
        source_keys,
        retained_keys,
        stripped_keys,
        added_keys,
        used_fallback: false,
    })
}

/// 读取当前 bundle 的完整签名清单。未签名 nested component 允许交给 deep sign，
/// 但无法解释的已签名 component 必须返回错误。
pub fn inspect_signing_inventory(app: &Path) -> Result<SigningInventory, String> {
    let outer = inspect_component(app)?;
    let nested = enumerate_nested_components(app)?;
    let entitlements = read_entitlements(app)?;
    let deep_strict = verify_deep_strict(app).is_ok();
    Ok(SigningInventory {
        outer,
        nested,
        entitlements,
        deep_strict,
    })
}

/// 当前安装完成后的 patched bundle 验收；不得接受普通浅层 codesign 成功。
pub fn verify_patched_adhoc_bundle_deep_strict(
    app: &Path,
    expected: Option<&PlistInfo>,
) -> Result<SigningInventory, String> {
    let inventory = inspect_signing_inventory(app)?;
    validate_signing_inventory(&inventory)?;
    if inventory.outer.kind != SignatureKind::Adhoc {
        return Err(format!(
            "patched bundle is not ad hoc signed: {}",
            inventory.outer.kind.as_str()
        ));
    }
    verify_plist_identity(app, expected, None)?;
    Ok(inventory)
}

/// 官方 original bundle 的 vendor-level 验收；不接受 ad-hoc 或未知 TeamIdentifier。
pub fn verify_original_vendor_bundle(
    app: &Path,
    expected_bundle_identifier: Option<&str>,
    expected_version: Option<&str>,
    expected_build: Option<&str>,
) -> Result<SigningInventory, String> {
    let inventory = inspect_signing_inventory(app)?;
    let expected_bundle_identifier =
        expected_bundle_identifier.unwrap_or(OFFICIAL_BUNDLE_IDENTIFIER);
    validate_official_signing_inventory(&inventory, Some(expected_bundle_identifier))?;
    verify_plist_identity(
        app,
        None,
        Some((expected_bundle_identifier, expected_version, expected_build)),
    )?;
    Ok(inventory)
}

/// 保持旧调用方的 bool seam，但底层已经升级为 deep/strict 验收。
pub fn verify_app(app: &Path) -> bool {
    let Ok(inventory) = inspect_signing_inventory(app) else {
        return false;
    };
    let patched = validate_signing_inventory(&inventory).is_ok()
        && inventory.outer.kind == SignatureKind::Adhoc
        && verify_plist_identity(app, None, None).is_ok();
    patched || validate_generic_signing_inventory(&inventory).is_ok()
}

/// 对 install、uninstall 与 Doctor 共享的签名清单做唯一 verdict 判断。
pub fn validate_signing_inventory(inventory: &SigningInventory) -> Result<(), String> {
    validate_inventory_with_policy(inventory, SigningPolicy::Official)
}

/// 对官方 original bundle 追加 vendor 身份与 outer bundle identifier 验收。
pub fn validate_official_signing_inventory(
    inventory: &SigningInventory,
    expected_bundle_identifier: Option<&str>,
) -> Result<(), String> {
    validate_signing_inventory(inventory)?;
    if inventory.outer.kind != SignatureKind::Vendor
        || inventory.outer.team_identifier.as_deref() != Some(VENDOR_TEAM_IDENTIFIER)
    {
        return Err("official original vendor signature is ad hoc or incomplete".into());
    }
    if inventory.outer.authorities.is_empty() {
        return Err("official original vendor signature has no authority chain".into());
    }
    let expected_bundle_identifier =
        expected_bundle_identifier.unwrap_or(OFFICIAL_BUNDLE_IDENTIFIER);
    if inventory.outer.identifier.as_deref() != Some(expected_bundle_identifier) {
        return Err(format!(
            "official original vendor signature identifier mismatch: expected {expected_bundle_identifier}"
        ));
    }
    Ok(())
}

/// 对自定义 `--app` 的 generic verifier 复用 deep/strict 与 identity evidence policy。
pub fn validate_generic_signing_inventory(inventory: &SigningInventory) -> Result<(), String> {
    validate_inventory_with_policy(inventory, SigningPolicy::Generic)
}

fn validate_inventory_with_policy(
    inventory: &SigningInventory,
    policy: SigningPolicy,
) -> Result<(), String> {
    if !inventory.deep_strict {
        return Err("bundle failed deep strict signature verification".into());
    }
    validate_outer_component(&inventory.outer, policy)?;
    validate_nested_components_with_policy(&inventory.nested, policy)
}

/// 供迁移 proof 等只需要 deep/strict 的调用方复用同一验收命令。
pub fn verify_bundle_deep_strict(app: &Path) -> Result<(), String> {
    verify_deep_strict(app)
}

pub fn has_hardened_runtime(app: &Path) -> bool {
    inspect_hardened_runtime(app).unwrap_or(false)
}

fn inspect_hardened_runtime(app: &Path) -> Result<bool, String> {
    let output = Command::new("codesign")
        .args(["--display", "--verbose=2", "--"])
        .arg(app)
        .output()
        .map_err(|error| format!("cannot inspect runtime flags {}: {error}", app.display()))?;
    if !output.status.success() {
        return Err(format!("cannot inspect runtime flags: {}", app.display()));
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(text
        .lines()
        .filter_map(|line| line.split_once("flags=").map(|(_, flags)| flags))
        .any(|flags| flags.contains("runtime")))
}

/// 使用共享 entitlement/component policy 完成 ad-hoc 签名。
pub fn sign_app_with_asar_integrity(app: &Path, hash: &str) -> Result<(), String> {
    sign_app_impl(app, Some(hash), None)
}

/// Sign a staged copy while resolving absolute Mach-O dependencies against the
/// verified final app root. Only dependencies that remain inside that final
/// bundle are translated to their staged counterparts.
pub fn sign_staged_app_with_asar_integrity(
    staged_app: &Path,
    final_app: &Path,
    hash: &str,
) -> Result<(), String> {
    let staged_info =
        super::read_plist_info(staged_app).ok_or("staged app identity unavailable")?;
    let staged_identifier = verified_host_identifier(staged_app, &staged_info.bundle_identifier)?;
    let final_info = super::read_plist_info(final_app).ok_or("final app identity unavailable")?;
    let final_identifier = verified_host_identifier(final_app, &final_info.bundle_identifier)?;
    if staged_identifier != final_identifier {
        return Err("staged and final app identities do not match".into());
    }
    sign_app_impl(staged_app, Some(hash), Some(final_app))
}

pub fn sign_app(app: &Path) -> Result<(), String> {
    sign_app_impl(app, None, None)
}

struct FrameworkDigestUpdate {
    bundle: PathBuf,
    bundle_identifier: String,
    binary: PathBuf,
    bytes: Vec<u8>,
    entitlements: String,
    hardened_runtime: bool,
}

struct DependentHelperUpdate {
    bundle: PathBuf,
    entitlements: String,
}

fn verified_bundle_identifier(bundle: &Path, plist_identifier: &str) -> Result<String, String> {
    verified_identifier(inspect_component(bundle)?, plist_identifier)
}

fn verified_host_identifier(app: &Path, plist_identifier: &str) -> Result<String, String> {
    // The installer validates the complete original before making its stage.
    // ASAR is intentionally edited in that stage before this signing API;
    // validate the still-sealed executable and Info.plist, not the old ASAR seal.
    // Nested helpers/Frameworks retain full validation before any writes.
    let component = inspect_codesign(app, |path| {
        Command::new("codesign")
            .args(["--verify", "--strict", "--ignore-resources", "--"])
            .arg(path)
            .output()
            .is_ok_and(|output| output.status.success())
    })?;
    verified_identifier(component, plist_identifier)
}

fn first_existing_load_candidate_matches(
    candidates: &[PathBuf],
    framework_binary: &Path,
) -> Result<bool, String> {
    for candidate in candidates {
        match fs::canonicalize(candidate) {
            Ok(resolved) => return Ok(resolved == framework_binary),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                continue;
            }
            Err(error) => {
                return Err(format!(
                    "cannot resolve Mach-O dylib dependency {}: {error}",
                    candidate.display()
                ));
            }
        }
    }
    Ok(false)
}

fn linked_dependencies_load_framework(
    dependencies: &[Vec<PathBuf>],
    framework_binary: &Path,
) -> Result<bool, String> {
    for candidates in dependencies {
        if first_existing_load_candidate_matches(candidates, framework_binary)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn remap_final_app_load_candidates_to_stage(
    dependencies: &mut [Vec<PathBuf>],
    staged_app: &Path,
    final_app: &Path,
) -> Result<(), String> {
    let staged_root = fs::canonicalize(staged_app).map_err(|error| {
        format!(
            "cannot resolve staged app {}: {error}",
            staged_app.display()
        )
    })?;
    let final_root = fs::canonicalize(final_app)
        .map_err(|error| format!("cannot resolve final app {}: {error}", final_app.display()))?;

    for candidate in dependencies.iter_mut().flatten() {
        if !candidate.is_absolute() {
            continue;
        }
        let resolved_final = match fs::canonicalize(&*candidate) {
            Ok(path) => path,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                continue;
            }
            Err(error) => {
                return Err(format!(
                    "cannot resolve final-app Mach-O dependency {}: {error}",
                    candidate.display()
                ));
            }
        };
        let Ok(relative) = resolved_final.strip_prefix(&final_root) else {
            // In-bundle symlinks that escape the verified final root are
            // external dependencies, not aliases for staged app contents.
            continue;
        };
        let staged_candidate = staged_root.join(relative);
        match fs::canonicalize(&staged_candidate) {
            Ok(resolved_staged) => {
                if !resolved_staged.starts_with(&staged_root) {
                    return Err(format!(
                        "staged Mach-O dependency escapes app root: {}",
                        staged_candidate.display()
                    ));
                }
                *candidate = resolved_staged;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                // Preserve the candidate's place in the dyld search order,
                // but model the path that will exist after stage placement.
                *candidate = staged_candidate;
            }
            Err(error) => {
                return Err(format!(
                    "cannot resolve staged Mach-O dependency {}: {error}",
                    staged_candidate.display()
                ));
            }
        }
    }
    Ok(())
}

fn verified_identifier(
    component: SignedComponent,
    plist_identifier: &str,
) -> Result<String, String> {
    let bundle = &component.path;
    if !component.verified {
        return Err(format!(
            "signature verification failed: {}",
            bundle.display()
        ));
    }
    let identifier = component
        .identifier
        .ok_or_else(|| format!("signature identifier unavailable: {}", bundle.display()))?;
    if identifier != plist_identifier {
        return Err(format!(
            "signature identifier mismatch: {}",
            bundle.display()
        ));
    }
    Ok(identifier)
}

fn entitlement_enabled(source: &EntitlementSnapshot, key: &str) -> Result<bool, String> {
    if !source.keys.contains(key) {
        return Ok(false);
    }
    let mut child = Command::new("plutil")
        .args(["-convert", "json", "-o", "-", "--", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    child
        .stdin
        .take()
        .ok_or("plutil stdin is unavailable")?
        .write_all(source.xml.as_bytes())
        .map_err(|error| error.to_string())?;
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("cannot parse library-validation entitlement".into());
    }
    let raw: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    raw.get(key)
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| "library-validation entitlement must be Boolean".into())
}

fn dependent_helper_updates(
    app: &Path,
    frameworks: &[FrameworkDigestUpdate],
    final_app: Option<&Path>,
) -> Result<Vec<DependentHelperUpdate>, String> {
    if frameworks.is_empty() {
        return Ok(Vec::new());
    }
    let app_identifier = read_plist_info(app)
        .ok_or("app identity unavailable")?
        .bundle_identifier;
    let app_identifier = verified_host_identifier(app, &app_identifier)?;
    let namespace = format!("{app_identifier}.helper");
    let mut updates = Vec::new();
    for bundle in enumerate_component_paths(app)?
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "app"))
    {
        let info = read_plist_info(&bundle).ok_or("nested helper plist unavailable")?;
        if info.executable.is_empty()
            || Path::new(&info.executable).components().count() != 1
            || info.executable == "."
            || info.executable == ".."
        {
            return Err("invalid helper executable name".into());
        }
        let binary = fs::canonicalize(bundle.join("Contents/MacOS").join(&info.executable))
            .map_err(|error| error.to_string())?;
        let canonical_bundle = fs::canonicalize(&bundle).map_err(|error| error.to_string())?;
        if !binary.starts_with(canonical_bundle) {
            return Err("helper executable escapes its bundle".into());
        }
        let bytes = fs::read(&binary).map_err(|error| error.to_string())?;
        let mut dependencies =
            super::asar_integrity_digest::resolved_linked_dylib_paths(&bytes, &binary)?;
        if let Some(final_app) = final_app {
            remap_final_app_load_candidates_to_stage(&mut dependencies, app, final_app)?;
        }
        let mut matching_framework = None;
        for framework in frameworks {
            if linked_dependencies_load_framework(&dependencies, &framework.binary)? {
                if matching_framework.is_some() {
                    return Err("helper loads multiple changed frameworks".into());
                }
                matching_framework = Some(framework);
            }
        }
        if matching_framework.is_none() {
            let dynamic_paths = super::asar_integrity_digest::dynamic_framework_load_paths(&bytes)?;
            for framework in frameworks {
                let dynamically_loads_framework = dynamic_paths.iter().any(|relative| {
                    binary
                        .parent()
                        .and_then(|parent| fs::canonicalize(parent.join(relative)).ok())
                        .is_some_and(|path| path == framework.binary)
                });
                if dynamically_loads_framework {
                    if matching_framework.is_some() {
                        return Err("helper loads multiple changed frameworks".into());
                    }
                    matching_framework = Some(framework);
                }
            }
        }
        let Some(framework) = matching_framework else {
            continue;
        };
        if !inspect_hardened_runtime(&bundle)? {
            continue;
        }
        let source = read_entitlements(&bundle)?;
        if entitlement_enabled(&source, DISABLE_LIBRARY_VALIDATION)? {
            continue;
        }
        let helper_identifier = verified_bundle_identifier(&bundle, &info.bundle_identifier)?;
        // Chromium's notification helper has a Framework-derived identity,
        // separate from Electron's .helper namespace. Admit only that exact
        // role under the host's own Framework, never arbitrary Framework children.
        let is_electron_helper = helper_identifier == namespace
            || helper_identifier.starts_with(&format!("{namespace}."));
        let is_notification_helper = framework.bundle_identifier
            == format!("{app_identifier}.framework")
            && helper_identifier
                == format!("{}.AlertNotificationService", framework.bundle_identifier);
        // CUA sidecars are neither of these identities. An unknown dependent
        // fails closed before any digest, metadata or signature changes.
        if !is_electron_helper && !is_notification_helper {
            return Err(format!(
                "protected or unknown helper requires changed framework: {}",
                bundle.display()
            ));
        }
        let mut source = source;
        if source.keys.remove(DISABLE_LIBRARY_VALIDATION) {
            source.xml = strip_unretainable_entitlements(
                &source.xml,
                &BTreeSet::from([DISABLE_LIBRARY_VALIDATION.to_string()]),
            )?;
        }
        let entitlements = plan_adhoc_entitlements(&source)?.xml;
        updates.push(DependentHelperUpdate {
            bundle,
            entitlements,
        });
    }
    Ok(updates)
}

fn framework_digest_updates(
    app: &Path,
    old: &serde_json::Value,
    new: &serde_json::Value,
) -> Result<Vec<FrameworkDigestUpdate>, String> {
    let mut updates = Vec::new();
    for bundle in enumerate_component_paths(app)?
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "framework"))
    {
        let plist = bundle.join("Resources/Info.plist");
        if !plist.exists() {
            continue;
        }
        let raw = super::read_plist_json_result(&plist)?;
        let name = raw
            .get("CFBundleExecutable")
            .and_then(serde_json::Value::as_str)
            .ok_or("framework has no CFBundleExecutable")?;
        if name.is_empty()
            || Path::new(name).components().count() != 1
            || name == "."
            || name == ".."
        {
            return Err("invalid framework executable name".into());
        }
        let binary = fs::canonicalize(bundle.join(name))
            .map_err(|error| format!("cannot resolve framework executable: {error}"))?;
        if !binary.starts_with(fs::canonicalize(&bundle).map_err(|error| error.to_string())?) {
            return Err("framework executable escapes its bundle".into());
        }
        let bytes = fs::read(&binary).map_err(|error| error.to_string())?;
        if let Some(bytes) =
            super::asar_integrity_digest::plan_integrity_digest_update(&bytes, old, new)?
        {
            let bundle_identifier = raw
                .get("CFBundleIdentifier")
                .and_then(serde_json::Value::as_str)
                .filter(|identifier| !identifier.is_empty())
                .ok_or("framework has no CFBundleIdentifier")?;
            let bundle_identifier = verified_bundle_identifier(&bundle, bundle_identifier)?;
            let source = read_entitlements(&bundle)?;
            let stripped = source
                .keys
                .iter()
                .filter(|key| ADHOC_UNRETAINABLE_ENTITLEMENTS.contains(&key.as_str()))
                .cloned()
                .collect();
            let entitlements = if source.xml.is_empty() {
                empty_entitlements_xml()
            } else {
                strip_unretainable_entitlements(&source.xml, &stripped)?
            };
            updates.push(FrameworkDigestUpdate {
                hardened_runtime: has_hardened_runtime(&bundle),
                bundle_identifier: bundle_identifier.to_string(),
                bundle,
                binary,
                bytes,
                entitlements,
            });
        }
    }
    if updates.len() > 1 {
        return Err(
            "multiple frameworks enforce the host ASAR dictionary; refusing ambiguous mutation"
                .into(),
        );
    }
    Ok(updates)
}

fn sign_app_impl(
    app: &Path,
    integrity_hash: Option<&str>,
    final_app: Option<&Path>,
) -> Result<(), String> {
    let before = read_entitlements(app)?;
    let plan = plan_adhoc_entitlements(&before)?;
    let outer = inspect_component(app)?;
    let integrity = integrity_hash
        .map(|hash| super::asar_integrity_payload(app, hash))
        .transpose()?;
    let updates = match &integrity {
        Some((old, new)) => framework_digest_updates(app, old, new)?,
        None => Vec::new(),
    };
    let helpers = dependent_helper_updates(app, &updates, final_app)?;
    // Validate every existing nested signature before altering any digest. Only the
    // proven digest-bearing framework and necessary loading Electron helpers
    // are excluded; all other vendor children stay stashed.
    let excluded: Vec<_> = updates
        .iter()
        .map(|update| update.bundle.clone())
        .chain(helpers.iter().map(|helper| helper.bundle.clone()))
        .collect();
    let preserve = collect_vendor_helper_roots_excluding(app, &outer, &excluded)?;
    let stash_root = if preserve.is_empty() {
        None
    } else {
        let dir = temporary_dir("incodex-vendor");
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        Some(dir)
    };
    let mut stashed = Vec::new();
    if let Some(root) = &stash_root {
        for (index, src) in preserve.iter().enumerate() {
            let dest = root.join(index.to_string()).join(
                src.file_name()
                    .ok_or_else(|| "vendor helper missing name".to_string())?,
            );
            super::ditto(src, &dest)?;
            fs::remove_dir_all(src).map_err(|error| error.to_string())?;
            stashed.push((src.clone(), dest));
        }
    }
    let deep = (|| {
        if let Some((_, new)) = &integrity {
            super::write_asar_integrity_payload(&app.join("Contents/Info.plist"), new)?;
        }
        for update in &updates {
            fs::write(&update.binary, &update.bytes).map_err(|error| error.to_string())?;
        }
        Command::new("codesign")
            .args(["--force", "--deep", "--sign", "-", "--"])
            .arg(app)
            .output()
            .map_err(|error| error.to_string())
            .and_then(command_success)
    })();
    let restore = restore_stashed_helpers(&stashed, stash_root.as_deref());
    if let Err(error) = restore {
        return Err(match deep {
            Ok(()) => error,
            Err(deep_error) => format!("{deep_error}; {error}"),
        });
    }
    deep?;
    for helper in &helpers {
        sign_component_with_entitlements(&helper.bundle, &helper.entitlements, true)?;
    }
    for update in &updates {
        sign_component_with_entitlements(
            &update.bundle,
            &update.entitlements,
            update.hardened_runtime,
        )?;
        if let Some((_, new)) = &integrity {
            let bytes = fs::read(&update.binary).map_err(|error| error.to_string())?;
            // An already matching digest is a no-op, not a validation failure.
            super::asar_integrity_digest::plan_integrity_digest_update(&bytes, new, new)?;
        }
    }
    sign_outer_with_entitlements(app, &plan.xml)?;
    verify_patched_adhoc_bundle_deep_strict(app, None)
        .map(|_| ())
        .map_err(|error| {
            format!("codesign --verify --deep --strict failed after adhoc resign: {error}")
        })
}

/// 返回当前 app 中需要保持 vendor identity 的顶层 sidecar。
pub fn collect_vendor_helper_roots(app: &Path) -> Result<Vec<PathBuf>, String> {
    let outer = inspect_component(app)?;
    collect_vendor_helper_roots_for_outer(app, &outer)
}

fn collect_vendor_helper_roots_for_outer(
    app: &Path,
    outer: &SignedComponent,
) -> Result<Vec<PathBuf>, String> {
    collect_vendor_helper_roots_excluding(app, outer, &[])
}

fn collect_vendor_helper_roots_excluding(
    app: &Path,
    outer: &SignedComponent,
    excluded: &[PathBuf],
) -> Result<Vec<PathBuf>, String> {
    let components = inspect_nested_components(app)?;
    let generic_outer =
        outer.kind == SignatureKind::Other && has_identity_evidence(outer) && outer.verified;
    if generic_outer {
        validate_generic_nested_components(&components)?;
    } else {
        validate_nested_components(&components)?;
    }
    let mut vendors = components
        .iter()
        .filter(|component| {
            component.kind == SignatureKind::Vendor
                && !component.path.starts_with(app.join(SPARKLE_FRAMEWORK))
                && !excluded.contains(&component.path)
        })
        .map(|component| component.path.clone())
        .collect::<Vec<_>>();
    vendors.sort_by_key(|path| path.components().count());
    let mut roots = Vec::new();
    for path in vendors {
        if !roots.iter().any(|root: &PathBuf| path.starts_with(root)) {
            roots.push(path);
        }
    }
    Ok(roots)
}

fn inspect_nested_components(app: &Path) -> Result<Vec<SignedComponent>, String> {
    let paths = enumerate_component_paths(app)?;
    let mut components = Vec::new();
    for path in paths {
        let component = inspect_component(&path)?;
        if component.kind != SignatureKind::Unsigned {
            components.push(component);
        }
    }
    Ok(components)
}

fn enumerate_nested_components(app: &Path) -> Result<Vec<SignedComponent>, String> {
    let mut components = inspect_nested_components(app)?;
    components.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(components)
}

fn enumerate_component_paths(app: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    walk_component_paths(app, &mut paths)?;
    Ok(paths)
}

fn walk_component_paths(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        fs::read_dir(dir).map_err(|error| format!("cannot scan {}: {error}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot scan {}: {error}", dir.display()))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if !metadata.is_dir() {
            continue;
        }
        if is_bundle_component(&path) {
            out.push(path.clone());
        }
        walk_component_paths(&path, out)?;
    }
    Ok(())
}

fn is_bundle_component(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "app" | "xpc" | "appex" | "framework"))
}

fn inspect_component(path: &Path) -> Result<SignedComponent, String> {
    inspect_codesign(path, |path| verify_deep_strict(path).is_ok())
}

/// 对 generic deep/strict fallback 复用 nested component policy。
pub fn validate_nested_components(components: &[SignedComponent]) -> Result<(), String> {
    validate_nested_components_with_policy(components, SigningPolicy::Official)
}

pub fn validate_generic_nested_components(components: &[SignedComponent]) -> Result<(), String> {
    validate_nested_components_with_policy(components, SigningPolicy::Generic)
}

fn validate_outer_component(
    component: &SignedComponent,
    policy: SigningPolicy,
) -> Result<(), String> {
    match component.kind {
        SignatureKind::Adhoc if component.verified => Ok(()),
        SignatureKind::Adhoc => Err("outer ad-hoc signature verification failed".into()),
        SignatureKind::Vendor => validate_vendor_component(component, "outer bundle"),
        SignatureKind::Other if policy == SigningPolicy::Generic => {
            if !has_identity_evidence(component) {
                return Err("third-party outer signature lacks identity evidence".into());
            }
            if !component.verified {
                return Err("third-party outer signature verification failed".into());
            }
            Ok(())
        }
        SignatureKind::Other | SignatureKind::Unknown | SignatureKind::Unsigned => Err(format!(
            "unsupported outer signature identity: {}",
            component.kind.as_str()
        )),
    }
}

fn validate_nested_components_with_policy(
    components: &[SignedComponent],
    policy: SigningPolicy,
) -> Result<(), String> {
    for component in components {
        if policy == SigningPolicy::Generic && component.kind == SignatureKind::Other {
            if !has_identity_evidence(component) || !component.verified {
                return Err(format!(
                    "third-party nested component lacks identity evidence: {}",
                    component.path.display()
                ));
            }
        } else {
            validate_nested_component(component)?;
        }
    }
    Ok(())
}

fn validate_nested_component(component: &SignedComponent) -> Result<(), String> {
    match component.kind {
        SignatureKind::Vendor => validate_vendor_component(component, "nested component"),
        SignatureKind::Adhoc if component.verified => Ok(()),
        SignatureKind::Adhoc => Err(format!(
            "nested ad-hoc component signature verification failed: {}",
            component.path.display()
        )),
        SignatureKind::Other | SignatureKind::Unknown | SignatureKind::Unsigned => Err(format!(
            "unsupported signed nested component identity: {}",
            component.path.display()
        )),
    }
}

fn validate_vendor_component(component: &SignedComponent, label: &str) -> Result<(), String> {
    if component.team_identifier.as_deref() != Some(VENDOR_TEAM_IDENTIFIER) {
        return Err(format!("{label} has an unexpected vendor TeamIdentifier"));
    }
    if component.authorities.is_empty() {
        return Err(format!("{label} has no vendor authority evidence"));
    }
    if !component.verified {
        return Err(format!("{label} signature verification failed"));
    }
    let identifier = component
        .identifier
        .as_deref()
        .ok_or_else(|| format!("{label} has no vendor identifier evidence"))?;
    verify_apple_vendor_requirement(component.path.as_path(), identifier, label)?;
    Ok(())
}

fn verify_apple_vendor_requirement(
    path: &Path,
    identifier: &str,
    label: &str,
) -> Result<(), String> {
    let escaped = identifier.replace('\\', "\\\\").replace('"', "\\\"");
    let requirement = format!("=anchor apple generic and identifier \"{escaped}\"");
    let output = Command::new("codesign")
        .args(["--verify", "--test-requirement", &requirement, "--"])
        .arg(path)
        .output()
        .map_err(|error| format!("cannot verify Apple vendor trust for {label}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if detail.is_empty() {
        format!("{label} is not anchored to an Apple vendor signature")
    } else {
        format!("{label} failed Apple vendor trust requirement: {detail}")
    })
}

fn verify_deep_strict(app: &Path) -> Result<(), String> {
    let output = Command::new("codesign")
        .args(["--verify", "--deep", "--strict", "--verbose=4", "--"])
        .arg(app)
        .output()
        .map_err(|error| format!("cannot verify signature {}: {error}", app.display()))?;
    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if detail.is_empty() {
            format!("signature verification failed: {}", app.display())
        } else {
            format!("signature verification failed: {detail}")
        })
    }
}

fn verify_plist_identity(
    app: &Path,
    expected: Option<&PlistInfo>,
    vendor: Option<(&str, Option<&str>, Option<&str>)>,
) -> Result<(), String> {
    let info =
        read_plist_info(app).ok_or_else(|| format!("Info.plist unreadable: {}", app.display()))?;
    if info.executable.trim().is_empty() {
        return Err("CFBundleExecutable is missing".into());
    }
    if let Some(expected) = expected {
        if info.bundle_identifier != expected.bundle_identifier
            || info.app_version != expected.app_version
            || info.app_build != expected.app_build
            || info.executable != expected.executable
        {
            return Err("patched bundle identity changed during signing".into());
        }
    }
    if let Some((bundle_identifier, version, build)) = vendor {
        if info.bundle_identifier != bundle_identifier {
            return Err(format!(
                "vendor bundle identifier mismatch: expected {bundle_identifier}, got {}",
                info.bundle_identifier
            ));
        }
        if version.is_some_and(|expected| expected != info.app_version)
            || build.is_some_and(|expected| expected != info.app_build)
        {
            return Err("vendor bundle version/build mismatch".into());
        }
    }
    Ok(())
}

fn parse_entitlement_keys(xml: &str) -> Result<BTreeSet<String>, String> {
    if !xml.contains("<plist") || !xml.contains("<dict") || !xml.contains("</dict>") {
        return Err("entitlement plist is malformed".into());
    }
    let mut keys = BTreeSet::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<key>") {
        let after = &rest[start + "<key>".len()..];
        let end = after
            .find("</key>")
            .ok_or("entitlement plist has an unterminated key")?;
        let key = after[..end].trim();
        if key.is_empty() {
            return Err("entitlement plist contains an empty key".into());
        }
        keys.insert(key.to_string());
        rest = &after[end + "</key>".len()..];
    }
    Ok(keys)
}

fn strip_unretainable_entitlements(
    xml: &str,
    stripped: &BTreeSet<String>,
) -> Result<String, String> {
    let mut next = xml.to_string();
    for key in stripped {
        let marker = format!("<key>{key}</key>");
        while let Some(start) = next.find(&marker) {
            let value_start = start + marker.len();
            let value_start = value_start
                + next[value_start..]
                    .chars()
                    .take_while(|character| character.is_whitespace())
                    .map(char::len_utf8)
                    .sum::<usize>();
            let value_end = xml_value_end(&next, value_start)
                .ok_or_else(|| format!("cannot parse entitlement value for {key}"))?;
            next.replace_range(start..value_end, "");
        }
    }
    if !next.contains("<dict") || !next.contains("</dict>") {
        return Err("entitlement plist became malformed after filtering".into());
    }
    Ok(next)
}

fn empty_entitlements_xml() -> String {
    "<?xml version=\"1.0\"?><plist><dict></dict></plist>\n".to_string()
}

fn xml_value_end(xml: &str, start: usize) -> Option<usize> {
    let rest = xml.get(start..)?;
    if rest.starts_with("<true/>") || rest.starts_with("<false/>") {
        return Some(start + rest.find('>')? + 1);
    }
    let open_end = rest.find('>')?;
    let open = &rest[1..open_end];
    if open.trim_end().ends_with('/') {
        return Some(start + open_end + 1);
    }
    let name = open.split_whitespace().next()?.trim_end_matches('/');
    let close = format!("</{name}>");
    let close_start = rest[open_end + 1..].find(&close)? + open_end + 1;
    Some(start + close_start + close.len())
}

fn sign_outer_with_entitlements(app: &Path, entitlements: &str) -> Result<(), String> {
    sign_component_with_entitlements(app, entitlements, true)
}

fn sign_component_with_entitlements(
    app: &Path,
    entitlements: &str,
    hardened_runtime: bool,
) -> Result<(), String> {
    let root = temporary_dir("incodex-ent");
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let file = root.join("entitlements.plist");
    if let Err(error) = fs::write(&file, entitlements) {
        let _ = fs::remove_dir_all(&root);
        return Err(error.to_string());
    }
    let mut command = Command::new("codesign");
    command.args(["--force", "--sign", "-"]);
    if hardened_runtime {
        command.args(["--options", "runtime"]);
    }
    let result = command
        .args(["--entitlements"])
        .arg(&file)
        .args(["--"])
        .arg(app)
        .output()
        .map_err(|error| error.to_string())
        .and_then(command_success);
    let cleanup = fs::remove_dir_all(root).map_err(|error| error.to_string());
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Err(sign), Err(cleanup)) => {
            Err(format!("{sign}; failed to clean entitlements: {cleanup}"))
        }
    }
}

fn command_success(output: std::process::Output) -> Result<(), String> {
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn temporary_dir(prefix: &str) -> PathBuf {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("{prefix}-{}-{suffix}", std::process::id()))
}

fn restore_stashed_helpers(
    stashed: &[(PathBuf, PathBuf)],
    stash_root: Option<&Path>,
) -> Result<(), String> {
    let mut failures = Vec::new();
    for (src, dest) in stashed {
        if let Err(error) = super::ditto(dest, src) {
            failures.push(format!("failed to restore {}: {error}", src.display()));
        }
    }
    if failures.is_empty() {
        if let Some(root) = stash_root {
            if let Err(error) = fs::remove_dir_all(root) {
                failures.push(format!(
                    "failed to remove vendor stash {}: {error}",
                    root.display()
                ));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}
