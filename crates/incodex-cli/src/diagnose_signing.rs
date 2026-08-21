use std::path::Path;

use incodex_macos::{
    has_hardened_runtime, inspect_outer_signing, plan_adhoc_entitlements,
    validate_generic_nested_components, validate_generic_signing_inventory,
    validate_nested_components, validate_official_signing_inventory, validate_signing_inventory,
    SignatureKind, SignedComponent, SigningInventory, OFFICIAL_BUNDLE_IDENTIFIER,
    VENDOR_TEAM_IDENTIFIER,
};

use crate::diagnose_checks::{CheckResult, DiagnosticFinding};

pub(crate) fn not_requested_spctl() -> serde_json::Value {
    serde_json::json!({
        "status": "not-requested",
        "output": serde_json::Value::Null,
        "accepted": serde_json::Value::Null,
        "usedAsSuccessGate": false,
    })
}

pub(crate) fn not_requested_signing(outer: Option<&SignedComponent>) -> serde_json::Value {
    let outer = outer.map(|component| {
        serde_json::json!({
            "path": component.path,
            "identifier": component.identifier,
            "teamIdentifier": component.team_identifier,
            "kind": component.kind.as_str(),
            "verified": component.verified,
        })
    });
    serde_json::json!({
        "status": "not-requested",
        "verified": serde_json::Value::Null,
        "componentCount": serde_json::Value::Null,
        "components": serde_json::Value::Null,
        "outer": outer,
        "hardenedRuntimeOk": serde_json::Value::Null,
        "unretainable": serde_json::Value::Null,
        "spctl": not_requested_spctl(),
    })
}

/// 默认 Doctor 只读取 outer identity，明确把 nested/entitlement 结论标成未请求。
pub(crate) fn inspect_outer(
    app_path: &Path,
    patched: bool,
    official_target: bool,
) -> (Option<serde_json::Value>, bool, CheckResult) {
    let outer = match inspect_outer_signing(app_path) {
        Ok(outer) => outer,
        Err(error) => {
            return (
                Some(serde_json::json!({
                    "status": "unknown",
                    "verified": false,
                    "componentCount": serde_json::Value::Null,
                    "components": serde_json::Value::Null,
                    "outer": serde_json::Value::Null,
                    "hardenedRuntimeOk": serde_json::Value::Null,
                    "unretainable": serde_json::Value::Null,
                    "error": error,
                    "spctl": not_requested_spctl(),
                })),
                false,
                CheckResult::unknown(
                    "signing.inspect-failed",
                    "outer signature identity could not be inspected",
                ),
            );
        }
    };
    let accepted = accepts_outer_identity(&outer, patched, official_target);
    let (code, message) = if accepted {
        (
            "signing.not-requested",
            "nested signing, entitlements, and Gatekeeper were not inspected; use doctor --deep"
                .to_string(),
        )
    } else {
        (
            "signing.outer-identity",
            "outer signature identity evidence does not match this target".to_string(),
        )
    };
    (
        Some(not_requested_signing(Some(&outer))),
        accepted,
        CheckResult::unknown(code, message),
    )
}

fn accepts_outer_identity(outer: &SignedComponent, patched: bool, official_target: bool) -> bool {
    if !outer.verified {
        return false;
    }
    if patched {
        return outer.kind == SignatureKind::Adhoc;
    }
    if official_target {
        return outer.kind == SignatureKind::Vendor
            && outer.team_identifier.as_deref() == Some(VENDOR_TEAM_IDENTIFIER)
            && outer.identifier.as_deref() == Some(OFFICIAL_BUNDLE_IDENTIFIER)
            && !outer.authorities.is_empty();
    }
    match outer.kind {
        SignatureKind::Adhoc => true,
        SignatureKind::Vendor | SignatureKind::Other => {
            outer.identifier.is_some()
                && outer.team_identifier.is_some()
                && !outer.authorities.is_empty()
        }
        SignatureKind::Unknown | SignatureKind::Unsigned => false,
    }
}

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
