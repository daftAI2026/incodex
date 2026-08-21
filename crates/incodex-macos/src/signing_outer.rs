use std::path::Path;
use std::process::Command;

use crate::signing::{SignatureKind, SignedComponent, VENDOR_TEAM_IDENTIFIER};

/// 读取并严格验证 outer 签名，不递归枚举或 deep 验证 nested components。
pub fn inspect_outer_signing(path: &Path) -> Result<SignedComponent, String> {
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
    let verified = kind != SignatureKind::Unsigned && verify_outer_strict(path);
    Ok(SignedComponent {
        path: path.to_path_buf(),
        identifier,
        team_identifier,
        authorities,
        kind,
        verified,
    })
}

fn verify_outer_strict(path: &Path) -> bool {
    Command::new("codesign")
        .args(["--verify", "--strict", "--verbose=4", "--"])
        .arg(path)
        .output()
        .is_ok_and(|output| output.status.success())
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
