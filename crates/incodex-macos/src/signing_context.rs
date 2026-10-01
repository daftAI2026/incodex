/*
 * [INPUT]: 依赖调用方经私有注册核验的非秘密证书句柄、codesign 与共享组件/深度验证政策。
 * [OUTPUT]: 提供明确签名上下文、逐组件证书绑定 DR 与 local bundle 验收；无身份创建或授权动作。
 * [POS]: 稳定签名身份的政策边界，保留旧 ad-hoc 合同且不放宽 generic/vendor 信任。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::path::{Path, PathBuf};
use std::process::Command;

use super::signing::{inspect_component, validate_vendor_component, verify_plist_identity};
use super::{
    inspect_signing_inventory, read_plist_info, verify_patched_adhoc_bundle_deep_strict, PlistInfo,
    SignatureKind, SignedComponent, SigningInventory,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSigningIdentity {
    pub keychain_path: PathBuf,
    pub certificate_sha1: String,
    pub certificate_sha256: String,
}

impl LocalSigningIdentity {
    pub fn new(
        keychain_path: PathBuf,
        certificate_sha1: String,
        certificate_sha256: String,
    ) -> Result<Self, String> {
        if !keychain_path.is_absolute()
            || !hex_digest(&certificate_sha1, 40)
            || !hex_digest(&certificate_sha256, 64)
        {
            return Err(
                "local signing identity has invalid path or certificate fingerprints".into(),
            );
        }
        Ok(Self {
            keychain_path,
            certificate_sha1,
            certificate_sha256,
        })
    }

    pub fn requirement(&self, identifier: &str) -> Result<String, String> {
        if identifier.is_empty()
            || identifier.len() > 255
            || !identifier
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        {
            return Err("local signing component identifier is invalid".into());
        }
        if !hex_digest(&self.certificate_sha1, 40) {
            return Err("invalid local certificate SHA1".into());
        }
        Ok(format!(
            "designated => identifier \"{identifier}\" and certificate leaf = H\"{}\"",
            self.certificate_sha1
        ))
    }
}

fn hex_digest(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SigningContext {
    Adhoc,
    Local(LocalSigningIdentity),
}

impl SigningContext {
    pub(super) fn configure_sign_command(
        &self,
        command: &mut Command,
        path: &Path,
        own_requirement: bool,
    ) -> Result<(), String> {
        match self {
            Self::Adhoc => {
                command.args(["--sign", "-"]);
            }
            Self::Local(identity) => {
                LocalSigningIdentity::new(
                    identity.keychain_path.clone(),
                    identity.certificate_sha1.clone(),
                    identity.certificate_sha256.clone(),
                )?;
                command
                    .args(["--sign", &identity.certificate_sha1, "--keychain"])
                    .arg(&identity.keychain_path)
                    .arg("--timestamp=none");
                if own_requirement {
                    let identifier = inspect_component(path)?
                        .identifier
                        .or_else(|| read_plist_info(path).map(|info| info.bundle_identifier))
                        .ok_or("local signing requires an existing component identifier")?;
                    command.args([
                        "--identifier",
                        &identifier,
                        "--requirements",
                        &format!("={}", identity.requirement(&identifier)?),
                    ]);
                }
            }
        }
        Ok(())
    }
}

/// 默认 Doctor 可验证 outer，不为浅层诊断遍历 children。
pub fn verify_local_outer(app: &Path, identity: &LocalSigningIdentity) -> Result<(), String> {
    let outer = super::inspect_outer_signing(app)?;
    verify_local_component(&outer, identity)
}

pub(super) fn verify_local_component(
    component: &SignedComponent,
    identity: &LocalSigningIdentity,
) -> Result<(), String> {
    if !component.verified
        || matches!(
            component.kind,
            SignatureKind::Adhoc
                | SignatureKind::Unsigned
                | SignatureKind::Unknown
                | SignatureKind::Vendor
        )
    {
        return Err(format!(
            "component is not a verified registered local signature: {}",
            component.path.display()
        ));
    }
    let identifier = component
        .identifier
        .as_deref()
        .ok_or("local signature has no identifier")?;
    let designated = identity.requirement(identifier)?;
    let requirement = designated.strip_prefix("designated => ").unwrap();
    let output = Command::new("codesign")
        .args([
            "--verify",
            "--strict",
            "--test-requirement",
            &format!("={requirement}"),
            "--",
        ])
        .arg(&component.path)
        .output()
        .map_err(|error| format!("cannot verify local certificate: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "local certificate requirement failed: {}",
            component.path.display()
        ));
    }
    let output = Command::new("codesign")
        .args(["--display", "--requirements", "-", "--"])
        .arg(&component.path)
        .output()
        .map_err(|error| format!("cannot inspect local DR: {error}"))?;
    if !output.status.success() {
        return Err("cannot inspect embedded local DR".into());
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = text
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("designated => "))
        .ok_or("local signature has no designated requirement")?;
    // codesign 可能把证书 hex 打成大写；只正规化指纹，identifier 保持大小写精确。
    let actual = actual.replace(
        &identity.certificate_sha1.to_ascii_uppercase(),
        &identity.certificate_sha1,
    );
    if actual != designated {
        return Err(format!(
            "local designated requirement is not certificate-bound: {}",
            component.path.display()
        ));
    }
    Ok(())
}

pub fn validate_local_signing_inventory(
    inventory: &SigningInventory,
    identity: &LocalSigningIdentity,
) -> Result<(), String> {
    if !inventory.deep_strict {
        return Err("local bundle failed deep strict verification".into());
    }
    verify_local_component(&inventory.outer, identity)?;
    for component in &inventory.nested {
        if component.kind == SignatureKind::Vendor {
            validate_vendor_component(component, "preserved vendor")?;
        } else {
            verify_local_component(component, identity)?;
        }
    }
    Ok(())
}

pub fn verify_patched_bundle_with_context(
    app: &Path,
    expected: Option<&PlistInfo>,
    context: &SigningContext,
) -> Result<SigningInventory, String> {
    match context {
        SigningContext::Adhoc => verify_patched_adhoc_bundle_deep_strict(app, expected),
        SigningContext::Local(identity) => {
            let inventory = inspect_signing_inventory(app)?;
            validate_local_signing_inventory(&inventory, identity)?;
            verify_plist_identity(app, expected, None)?;
            if read_plist_info(app)
                .map(|info| info.bundle_identifier)
                .as_deref()
                != inventory.outer.identifier.as_deref()
            {
                return Err(
                    "local outer signing identifier does not match bundle identifier".into(),
                );
            }
            Ok(inventory)
        }
    }
}
