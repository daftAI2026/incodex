use std::path::Path;

use serde::Serialize;

use incodex_core::paths::{RUNTIME_CURRENT_NAME, RUNTIME_DIR_NAME};
use incodex_runtime_bundle::{DeployedRuntime, RuntimeIdentity};

use crate::diagnose_checks::{CheckResult, CheckStatus, DiagnosticFinding};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalRuntimeReport {
    pub status: CheckStatus,
    pub present: bool,
    pub ok: bool,
    pub version: Option<String>,
    pub release: Option<String>,
    pub error: Option<String>,
}

pub(crate) fn inspect_external_runtime(root: &Path) -> (ExternalRuntimeReport, CheckResult) {
    let current = root.join(RUNTIME_DIR_NAME).join(RUNTIME_CURRENT_NAME);
    match incodex_runtime_bundle::inspect_deployed(root) {
        Ok(None) => (
            ExternalRuntimeReport {
                status: CheckStatus::Checked,
                present: false,
                ok: false,
                version: None,
                release: None,
                error: Some("missing current.json".to_string()),
            },
            CheckResult::checked(vec![DiagnosticFinding::info(
                "runtime.missing",
                "external Runtime has not been published",
                Some(&current),
            )]),
        ),
        Ok(Some(deployed)) => match incodex_runtime_bundle::runtime_identity() {
            Ok(identity) => report_valid_runtime(deployed, &identity, &current),
            Err(error) => (
                ExternalRuntimeReport {
                    status: CheckStatus::Unknown,
                    present: true,
                    ok: true,
                    version: Some(deployed.version),
                    release: Some(deployed.release),
                    error: None,
                },
                CheckResult {
                    status: CheckStatus::Unknown,
                    findings: vec![DiagnosticFinding::warning(
                        "runtime.identity-unknown",
                        error,
                        Some(&current),
                    )],
                },
            ),
        },
        Err(error) => {
            let code = if error.contains("symlink") {
                "runtime.symlink"
            } else {
                "runtime.invalid"
            };
            (
                ExternalRuntimeReport {
                    status: CheckStatus::Checked,
                    present: true,
                    ok: false,
                    version: None,
                    release: None,
                    error: Some(error.clone()),
                },
                CheckResult::checked(vec![DiagnosticFinding::warning(
                    code,
                    error,
                    Some(&current),
                )]),
            )
        }
    }
}

fn report_valid_runtime(
    deployed: DeployedRuntime,
    bundled: &RuntimeIdentity,
    current: &Path,
) -> (ExternalRuntimeReport, CheckResult) {
    let matches_bundled = bundled.matches(&deployed);
    let check = if matches_bundled {
        CheckResult::checked(Vec::new())
    } else {
        CheckResult::checked(vec![DiagnosticFinding::warning(
            "runtime.stale",
            "deployed Runtime does not match this CLI's bundled Runtime; run `incodex runtime`",
            Some(current),
        )])
    };
    (
        ExternalRuntimeReport {
            status: check.status,
            present: true,
            ok: true,
            version: Some(deployed.version),
            release: Some(deployed.release),
            error: None,
        },
        check,
    )
}
