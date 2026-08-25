//! 签名验收、vendor sidecar 策略与 entitlement 处理。
//!
//! 这里是 install、uninstall 和 Doctor 共用的唯一签名判断入口：
//! - mutation 路径 fail closed；
//! - Doctor 区分“检查出损坏”和“无法检查”；
//! - vendor sidecar 按签名身份识别，不按文件名猜测。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::entitlements::add_entitlement_key;
use super::signing_outer::{has_signature_marker, signature_field};
use super::signing_policy::validate_generic_nested_components;
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
    if !inventory.deep_strict {
        return Err("bundle failed deep strict signature verification".into());
    }
    match inventory.outer.kind {
        SignatureKind::Adhoc => {
            if !inventory.outer.verified {
                return Err("outer ad-hoc signature verification failed".into());
            }
        }
        SignatureKind::Vendor => validate_vendor_component(&inventory.outer, "outer bundle")?,
        SignatureKind::Other | SignatureKind::Unknown | SignatureKind::Unsigned => {
            return Err(format!(
                "unsupported outer signature identity: {}",
                inventory.outer.kind.as_str()
            ));
        }
    }
    validate_nested_components(&inventory.nested)
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
    if !inventory.deep_strict {
        return Err("bundle failed deep strict signature verification".into());
    }
    match inventory.outer.kind {
        SignatureKind::Adhoc => {
            if !inventory.outer.verified {
                return Err("outer ad-hoc signature verification failed".into());
            }
        }
        SignatureKind::Vendor => validate_vendor_component(&inventory.outer, "outer bundle")?,
        SignatureKind::Other => {
            if inventory.outer.identifier.is_none()
                || inventory.outer.team_identifier.is_none()
                || inventory.outer.authorities.is_empty()
            {
                return Err("third-party outer signature lacks identity evidence".into());
            }
            if !inventory.outer.verified {
                return Err("third-party outer signature verification failed".into());
            }
        }
        SignatureKind::Unknown | SignatureKind::Unsigned => {
            return Err(format!(
                "unsupported outer signature identity: {}",
                inventory.outer.kind.as_str()
            ));
        }
    }
    validate_generic_nested_components(&inventory.nested)
}

/// 供迁移 proof 等只需要 deep/strict 的调用方复用同一验收命令。
pub fn verify_bundle_deep_strict(app: &Path) -> Result<(), String> {
    verify_deep_strict(app)
}

pub fn has_hardened_runtime(app: &Path) -> bool {
    let Ok(output) = Command::new("codesign")
        .args(["--display", "--verbose=2", "--"])
        .arg(app)
        .output()
    else {
        return false;
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    text.lines()
        .filter_map(|line| line.split_once("flags=").map(|(_, flags)| flags))
        .any(|flags| flags.contains("runtime"))
}

/// 使用共享 entitlement/component policy 完成 ad-hoc 签名。
pub fn sign_app(app: &Path) -> Result<(), String> {
    let before = read_entitlements(app)?;
    let plan = plan_adhoc_entitlements(&before)?;
    let outer = inspect_component(app)?;
    let preserve = collect_vendor_helper_roots_for_outer(app, &outer)?;
    let stash_root = if preserve.is_empty() {
        None
    } else {
        let dir = std::env::temp_dir().join(format!(
            "incodex-vendor-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
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
    let deep = Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-", "--"])
        .arg(app)
        .output()
        .map_err(|error| error.to_string())
        .and_then(|output| {
            if output.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
            }
        });
    let restore = restore_stashed_helpers(&stashed, stash_root.as_deref());
    if let Err(error) = restore {
        return Err(match deep {
            Ok(()) => error,
            Err(deep_error) => format!("{deep_error}; {error}"),
        });
    }
    deep?;
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
    let components = inspect_nested_components(app)?;
    let generic_outer = outer.kind == SignatureKind::Other
        && outer.identifier.is_some()
        && outer.team_identifier.is_some()
        && !outer.authorities.is_empty()
        && outer.verified;
    if generic_outer {
        validate_generic_nested_components(&components)?;
    } else {
        validate_nested_components(&components)?;
    }
    let mut vendors = components
        .iter()
        .filter(|component| component.kind == SignatureKind::Vendor)
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
    let output = Command::new("codesign")
        .args(["--display", "--verbose=4", "--"])
        .arg(path)
        .output()
        .map_err(|error| format!("cannot inspect signature {}: {error}", path.display()))?;
    if !output.status.success() {
        if has_signature_marker(path) {
            return Err(format!(
                "signed component could not be inspected: {}",
                path.display()
            ));
        }
        return Ok(SignedComponent {
            path: path.to_path_buf(),
            identifier: None,
            team_identifier: None,
            authorities: Vec::new(),
            kind: SignatureKind::Unsigned,
            verified: false,
        });
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
        .filter_map(|line| line.trim().strip_prefix("Authority=").map(str::to_string))
        .collect::<Vec<_>>();
    let adhoc = text.lines().any(|line| line.trim() == "Signature=adhoc");
    let kind = if adhoc {
        SignatureKind::Adhoc
    } else if team_identifier.as_deref() == Some(VENDOR_TEAM_IDENTIFIER) {
        SignatureKind::Vendor
    } else if team_identifier.is_some() || !authorities.is_empty() {
        SignatureKind::Other
    } else {
        SignatureKind::Unknown
    };
    let verified = if kind == SignatureKind::Unsigned {
        false
    } else {
        verify_deep_strict(path).is_ok()
    };
    Ok(SignedComponent {
        path: path.to_path_buf(),
        identifier,
        team_identifier,
        authorities,
        kind,
        verified,
    })
}

/// 对 generic deep/strict fallback 复用 nested component policy。
pub fn validate_nested_components(components: &[SignedComponent]) -> Result<(), String> {
    for component in components {
        match component.kind {
            SignatureKind::Vendor => validate_vendor_component(component, "nested component")?,
            SignatureKind::Adhoc => {
                if !component.verified {
                    return Err(format!(
                        "nested ad-hoc component signature verification failed: {}",
                        component.path.display()
                    ));
                }
            }
            SignatureKind::Other | SignatureKind::Unknown | SignatureKind::Unsigned => {
                return Err(format!(
                    "unsupported signed nested component identity: {}",
                    component.path.display()
                ));
            }
        }
    }
    Ok(())
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
    let root = std::env::temp_dir().join(format!(
        "incodex-ent-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let file = root.join("entitlements.plist");
    if let Err(error) = fs::write(&file, entitlements) {
        let _ = fs::remove_dir_all(&root);
        return Err(error.to_string());
    }
    let result = Command::new("codesign")
        .args([
            "--force",
            "--sign",
            "-",
            "--options",
            "runtime",
            "--entitlements",
        ])
        .arg(&file)
        .args(["--"])
        .arg(app)
        .output()
        .map_err(|error| error.to_string())
        .and_then(|output| {
            if output.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
            }
        });
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
