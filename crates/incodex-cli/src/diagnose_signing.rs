use std::path::Path;

use incodex_macos::{
    has_hardened_runtime, plan_adhoc_entitlements, validate_generic_nested_components,
    validate_generic_signing_inventory, validate_nested_components,
    validate_official_signing_inventory, validate_signing_inventory, SignatureKind,
    SignedComponent, SigningInventory, VENDOR_TEAM_IDENTIFIER,
};

use crate::diagnose_checks::{CheckResult, DiagnosticFinding};

pub(crate) fn validate_doctor_signing_inventory(
    inventory: &SigningInventory,
    patched: bool,
    official_target: bool,
) -> Result<(), String> {
    if patched {
        validate_signing_inventory(inventory)
    } else if official_target {
        validate_official_signing_inventory(inventory, None)
    } else {
        validate_generic_signing_inventory(inventory)
    }
}

fn validate_doctor_nested_component(
    component: &SignedComponent,
    patched: bool,
    official_target: bool,
) -> Result<(), String> {
    let component = std::slice::from_ref(component);
    if patched || official_target {
        validate_nested_components(component)
    } else {
        validate_generic_nested_components(component)
    }
}

pub(crate) fn inspect_signing(
    spctl: Option<&serde_json::Value>,
    codesign_ok: bool,
    app_path: &Path,
    signing_inventory: Option<&Result<SigningInventory, String>>,
    patched: bool,
    official_target: bool,
) -> (Option<serde_json::Value>, CheckResult) {
    let Some(spctl) = spctl else {
        return (
            None,
            CheckResult::unknown(
                "signing.not-checked",
                "the application does not exist, so nested signing was not inspected",
            ),
        );
    };
    let inventory = match signing_inventory {
        Some(Ok(inventory)) => inventory,
        Some(Err(error)) => {
            let report = serde_json::json!({
                "status": "unknown",
                "verified": codesign_ok,
                "componentCount": serde_json::Value::Null,
                "hardenedRuntimeOk": has_hardened_runtime(app_path),
                "unretainable": serde_json::Value::Null,
                "error": error,
                "spctl": spctl,
            });
            return (
                Some(report),
                CheckResult::unknown(
                    "signing.inspect-failed",
                    "signature inventory could not be inspected",
                ),
            );
        }
        None => {
            return (
                None,
                CheckResult::unknown(
                    "signing.not-checked",
                    "signature inventory was not requested",
                ),
            );
        }
    };

    let mut findings = Vec::new();
    let plan = plan_adhoc_entitlements(&inventory.entitlements);
    let unretainable = match &plan {
        Ok(plan) => plan
            .stripped_keys
            .iter()
            .cloned()
            .map(serde_json::Value::String)
            .collect::<Vec<_>>(),
        Err(error) => {
            findings.push(DiagnosticFinding::warning(
                "signing.entitlements-invalid",
                format!("entitlement policy could not be evaluated: {error}"),
                Some(app_path),
            ));
            Vec::new()
        }
    };
    if !unretainable.is_empty() {
        findings.push(DiagnosticFinding::info(
            "signing.entitlements-unretainable",
            "some vendor entitlements cannot be retained by an ad-hoc outer signature",
            Some(app_path),
        ));
    }
    if !inventory.deep_strict {
        findings.push(DiagnosticFinding::warning(
            "signing.deep-strict-failed",
            "bundle did not pass deep strict signature verification",
            Some(app_path),
        ));
    }
    let acceptance = validate_doctor_signing_inventory(&inventory, patched, official_target);
    if let Err(error) = &acceptance {
        let mut emitted_component_finding = false;
        for component in &inventory.nested {
            if validate_doctor_nested_component(component, patched, official_target).is_ok() {
                continue;
            }
            let finding = match component.kind {
                SignatureKind::Other | SignatureKind::Unknown | SignatureKind::Unsigned => Some((
                    "signing.component-identity-unsupported",
                    format!(
                        "nested {} component has an unsupported signing identity",
                        component.kind.as_str()
                    ),
                )),
                SignatureKind::Vendor
                    if component.team_identifier.as_deref() != Some(VENDOR_TEAM_IDENTIFIER)
                        || component.authorities.is_empty() =>
                {
                    Some((
                        "signing.component-identity-unsupported",
                        "nested vendor component lacks the required identity evidence".to_string(),
                    ))
                }
                _ if !component.verified => Some((
                    "signing.component-invalid",
                    format!(
                        "nested {} component failed deep strict verification",
                        component.kind.as_str()
                    ),
                )),
                _ => None,
            };
            if let Some((code, message)) = finding {
                emitted_component_finding = true;
                findings.push(DiagnosticFinding::warning(
                    code,
                    message,
                    Some(&component.path),
                ));
            }
        }
        if !emitted_component_finding {
            findings.push(DiagnosticFinding::warning(
                "signing.acceptance-failed",
                error.clone(),
                Some(app_path),
            ));
        }
    }
    let expected_kind = if patched {
        Some(SignatureKind::Adhoc)
    } else if official_target {
        Some(SignatureKind::Vendor)
    } else {
        None
    };
    if let Some(expected_kind) = expected_kind {
        if inventory.outer.kind != expected_kind {
            findings.push(DiagnosticFinding::warning(
                "signing.outer-identity",
                format!(
                    "outer bundle is {}; expected {} for this target state",
                    inventory.outer.kind.as_str(),
                    expected_kind.as_str()
                ),
                Some(app_path),
            ));
        }
    }
    let verified = codesign_ok
        && acceptance.is_ok()
        && expected_kind
            .map(|expected| inventory.outer.kind == expected)
            .unwrap_or(true);
    let components = inventory
        .nested
        .iter()
        .map(|component| {
            serde_json::json!({
                "path": component.path,
                "identifier": component.identifier,
                "teamIdentifier": component.team_identifier,
                "kind": component.kind.as_str(),
                "verified": component.verified,
            })
        })
        .collect::<Vec<_>>();
    let report = serde_json::json!({
        "status": "checked",
        "verified": verified,
        "componentCount": inventory.nested.len(),
        "components": components,
        "outer": {
            "identifier": inventory.outer.identifier,
            "teamIdentifier": inventory.outer.team_identifier,
            "kind": inventory.outer.kind.as_str(),
            "verified": inventory.outer.verified,
        },
        "hardenedRuntimeOk": has_hardened_runtime(app_path),
        "unretainable": unretainable,
        "spctl": spctl,
    });
    (Some(report), CheckResult::checked(findings))
}

