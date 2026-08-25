use std::path::Path;
use std::process::Command;

use crate::signing::{SignatureKind, SignedComponent, VENDOR_TEAM_IDENTIFIER};

pub(crate) fn has_identity_evidence(component: &SignedComponent) -> bool {
    component.identifier.is_some()
        && component.team_identifier.is_some()
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
    let team_identifier = signature_field(&text, "TeamIdentifier=");
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
