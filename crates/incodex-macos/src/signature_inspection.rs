/*
 * [INPUT]: 依赖 codesign 的声明与调用方指定的浅层/深层验证。
 * [OUTPUT]: 提供组件身份快照，缺失 Team 声明不成为 generic 信任证据。
 * [POS]: 签名文本解析边界；注册 local 证书需独立 context 验证，不靠 Authority 名称判断。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::path::Path;
use std::process::Command;

use crate::signing::{SignatureKind, SignedComponent, VENDOR_TEAM_IDENTIFIER};

/// 读取并严格验证 outer 签名，不递归枚举或 deep 验证 nested components。
pub fn inspect_outer_signing(path: &Path) -> Result<SignedComponent, String> {
    inspect_codesign(path, verify_outer_strict)
}

pub(crate) fn has_identity_evidence(component: &SignedComponent) -> bool {
    component.identifier.is_some()
        && component
            .team_identifier
            .as_deref()
            .is_some_and(valid_team_identifier)
        && !component.authorities.is_empty()
}

pub(crate) fn inspect_codesign<F>(path: &Path, verify: F) -> Result<SignedComponent, String>
where
    F: FnOnce(&Path) -> bool,
{
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
        return Ok(unsigned_component(path));
    }

    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let identifier = signature_field(&text, "Identifier=");
    let team_identifier =
        signature_field(&text, "TeamIdentifier=").filter(|value| valid_team_identifier(value));
    let authorities = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("Authority=").map(str::to_string))
        .collect::<Vec<_>>();
    let is_adhoc = text.lines().any(|line| line.trim() == "Signature=adhoc");
    let kind = signature_kind(is_adhoc, team_identifier.as_deref(), &authorities);
    let verified = kind != SignatureKind::Unsigned && verify(path);

    Ok(SignedComponent {
        path: path.to_path_buf(),
        identifier,
        team_identifier,
        authorities,
        kind,
        verified,
    })
}

fn unsigned_component(path: &Path) -> SignedComponent {
    SignedComponent {
        path: path.to_path_buf(),
        identifier: None,
        team_identifier: None,
        authorities: Vec::new(),
        kind: SignatureKind::Unsigned,
        verified: false,
    }
}

fn signature_kind(
    is_adhoc: bool,
    team_identifier: Option<&str>,
    authorities: &[String],
) -> SignatureKind {
    if is_adhoc {
        SignatureKind::Adhoc
    } else if team_identifier == Some(VENDOR_TEAM_IDENTIFIER) {
        SignatureKind::Vendor
    } else if team_identifier.is_some() || !authorities.is_empty() {
        SignatureKind::Other
    } else {
        SignatureKind::Unknown
    }
}

fn signature_field(text: &str, prefix: &str) -> Option<String> {
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix(prefix)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn has_signature_marker(path: &Path) -> bool {
    path.join("Contents/_CodeSignature").exists()
        || path.join("_CodeSignature").exists()
        || path.join("Contents/CodeResources").exists()
}

fn verify_outer_strict(path: &Path) -> bool {
    Command::new("codesign")
        .args(["--verify", "--strict", "--verbose=4", "--"])
        .arg(path)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn valid_team_identifier(value: &str) -> bool {
    !value.is_empty() && !matches!(value, "not set" | "not present")
}
