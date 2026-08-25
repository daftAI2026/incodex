use std::path::Path;

use incodex_asar::Archive;
use incodex_core::paths::ASAR_REL;
use serde::ser::{Serialize, SerializeStruct, Serializer};

use crate::diagnose::Diagnosis;
use crate::diagnose_checks::{CheckResult, CheckStatus};

pub fn format_status(report: &Diagnosis) -> String {
    let mut lines = vec![incodex_core::format_step("Status", None)];
    let app_path = Path::new(&report.target);
    if !report.exists {
        lines.push(incodex_core::format_warn(
            &format!("Codex app not found: {}", app_path.display()),
            None,
        ));
        append_runtime_state(&mut lines, report, false);
        lines.push(String::new());
        return lines.join("\n");
    }
    lines.push(incodex_core::format_kv("App", &report.target, None));
    lines.push(incodex_core::format_kv(
        "Exists",
        if report.exists { "yes" } else { "no" },
        None,
    ));
    lines.push(incodex_core::format_kv(
        "Installed",
        if report.patched { "yes" } else { "no" },
        None,
    ));
    if report.patched {
        if let Some(loader_only) = report.asar_loader_only {
            lines.push(incodex_core::format_kv(
                "Loader",
                if loader_only {
                    "asar loader only"
                } else {
                    "mixed"
                },
                None,
            ));
        }
    }
    let runtime = if app_path.join(ASAR_REL).exists() {
        runtime_description(report)
    } else {
        "missing".to_string()
    };
    lines.push(incodex_core::format_kv("Runtime", &runtime, None));
    append_runtime_state(&mut lines, report, false);
    if report.patched {
        if let Some(version) = app_version_description(report) {
            lines.push(incodex_core::format_kv("Version", &version, None));
        }
        if let Some(package) = Archive::open(app_path.join(ASAR_REL))
            .ok()
            .and_then(|archive| archive.read_package_main().ok())
        {
            if let Some(install_id) = package.install_id {
                lines.push(incodex_core::format_kv("Install id", &install_id, None));
            }
        }
    }
    lines.push(incodex_core::format_kv("Target", &report.target_id, None));
    if !app_path.join(ASAR_REL).exists() {
        lines.push(incodex_core::format_warn("asar missing", None));
    } else if report.patched {
        if !report.original_main.is_empty() {
            lines.push(incodex_core::format_kv("Main", &report.original_main, None));
        }
        lines.push(incodex_core::format_ok(
            "Incodex is installed. Use doctor for hashes and signing.",
            None,
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

pub fn format_diagnosis(report: &Diagnosis) -> String {
    let runtime = runtime_description(report);
    let backup = match report.backup.as_ref() {
        Some(backup) if json_bool(backup, "originalExists") && json_bool(backup, "complete") => {
            "ok"
        }
        Some(_) => "incomplete",
        None => "none",
    };
    let loader = match report.asar_loader_only {
        None => "unknown",
        Some(true) => "asar only",
        Some(false) => "mixed",
    };
    let app_version = format!(
        "{} {}",
        report.app_version.as_deref().unwrap_or("unknown"),
        report.app_build.as_deref().unwrap_or("")
    );
    let mut lines = vec![
        incodex_core::format_step("App", None),
        incodex_core::format_kv("Path", &report.target, None),
        incodex_core::format_kv("Exists", if report.exists { "yes" } else { "no" }, None),
        incodex_core::format_kv("Installed", if report.patched { "yes" } else { "no" }, None),
        incodex_core::format_kv(
            "Bundle",
            report.bundle_id.as_deref().unwrap_or("unknown"),
            None,
        ),
        incodex_core::format_kv("Version", app_version.trim(), None),
        incodex_core::format_kv(
            "Arch",
            report.architecture.as_deref().unwrap_or("unknown"),
            None,
        ),
        String::new(),
        incodex_core::format_step("Runtime", None),
        incodex_core::format_kv(
            "Version",
            report.runtime_version.as_deref().unwrap_or("unknown"),
            None,
        ),
        incodex_core::format_kv("External", &runtime, None),
        incodex_core::format_kv("External check", check_status(&report.checks.runtime), None),
    ];
    append_runtime_state(&mut lines, report, true);
    if let Some(error) = &report.external_runtime.error {
        lines.push(incodex_core::format_warn(error, None));
    }
    let main = if report.original_main.is_empty() {
        "unknown"
    } else {
        report.original_main.as_str()
    };
    lines.extend([
        incodex_core::format_kv("Loader", loader, None),
        incodex_core::format_kv("Main", main, None),
        String::new(),
        incodex_core::format_step("Signing", None),
        incodex_core::format_kv(
            "Verify",
            match report.codesign_ok {
                Some(true) => "ok",
                Some(false) => "failed",
                None => "unknown",
            },
            None,
        ),
        incodex_core::format_kv("Nested", check_status(&report.checks.signing), None),
    ]);
    if let Some(signing) = &report.signing {
        let deep_signing_requested = signing
            .get("spctl")
            .and_then(|spctl| spctl.get("status"))
            .and_then(serde_json::Value::as_str)
            != Some("not-requested");
        if deep_signing_requested {
            lines.push(incodex_core::format_kv(
                "Hardened",
                if json_bool(signing, "hardenedRuntimeOk") {
                    "yes"
                } else {
                    "no"
                },
                None,
            ));
            let dropped = signing
                .get("unretainable")
                .and_then(serde_json::Value::as_array)
                .map(|items| {
                    let names = items
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ");
                    if names.is_empty() {
                        "none".to_string()
                    } else {
                        names
                    }
                })
                .unwrap_or_else(|| "unknown".to_string());
            lines.push(incodex_core::format_kv("Dropped", &dropped, None));
        }
    }
    if let Some(spctl) = &report.spctl {
        if spctl.get("status").and_then(serde_json::Value::as_str) != Some("not-requested") {
            lines.push(incodex_core::format_kv(
                "Gatekeeper",
                if json_bool(spctl, "accepted") {
                    "accepted"
                } else {
                    "not accepted (diagnostic)"
                },
                None,
            ));
        }
    }
    lines.extend([
        String::new(),
        incodex_core::format_step("Backup", None),
        incodex_core::format_kv("State", backup, None),
        incodex_core::format_kv("Proof", check_status(&report.checks.backup), None),
    ]);
    if let Some(backup) = &report.backup {
        lines.push(incodex_core::format_kv(
            "Matches",
            if json_bool(backup, "belongsToTarget") {
                "yes"
            } else {
                "no"
            },
            None,
        ));
    }
    lines.extend([
        String::new(),
        incodex_core::format_step("Sessions", None),
        incodex_core::format_kv(
            "Orphans",
            &format!(
                "{} ({})",
                report.orphan_sessions.len(),
                check_status(&report.checks.orphan_sessions)
            ),
            None,
        ),
        incodex_core::format_kv(
            "Chromium",
            &format!(
                "{} ({})",
                report.leftover_chromium.len(),
                check_status(&report.checks.chromium_residue)
            ),
            None,
        ),
        incodex_core::format_kv(
            "Stale pid",
            &format!(
                "{} ({})",
                if report.stale_pid { "yes" } else { "no" },
                check_status(&report.checks.process_identity)
            ),
            None,
        ),
        incodex_core::format_kv(
            "Journals",
            &format!(
                "{} ({})",
                report.interrupted_transactions.len(),
                check_status(&report.checks.journals)
            ),
            None,
        ),
    ]);
    for item in &report.interrupted_transactions {
        lines.push(incodex_core::format_kv(
            "Journal",
            &format!("{}  {} -> {}", item.install_id, item.phase, item.action),
            None,
        ));
    }
    for record in &report.journal_records {
        if let Some(original) = &record.retained_original {
            lines.push(incodex_core::format_kv("Retained original", original, None));
            if let Some(original_valid) = record.original_valid {
                lines.push(incodex_core::format_kv(
                    "Original proof",
                    if original_valid { "valid" } else { "invalid" },
                    None,
                ));
            }
        }
        if let Some(recovery) = &record.recovery {
            lines.push(incodex_core::format_kv("Recovery", recovery, None));
        }
        for artifact in &record.artifacts {
            lines.push(incodex_core::format_kv("Artifact", artifact, None));
        }
    }
    if !report.interrupted_transactions.is_empty() {
        lines.push(incodex_core::format_warn(
            "Old install journals are leftover. They do not mean the current app is broken.",
            None,
        ));
    }
    let warning_findings = report
        .findings
        .iter()
        .filter(|finding| finding.severity == "warning")
        .collect::<Vec<_>>();
    if !warning_findings.is_empty() {
        lines.push(String::new());
        lines.push(incodex_core::format_step("Findings", None));
        for finding in warning_findings {
            lines.push(incodex_core::format_warn(
                &format!("{}: {}", finding.code, finding.message),
                None,
            ));
        }
    }
    lines.push(String::new());
    lines.join("\n")
}

pub fn diagnosis_json(report: &Diagnosis) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(&DiagnosisJson(report)).expect("json")
    )
}

struct DiagnosisJson<'a>(&'a Diagnosis);

impl Serialize for DiagnosisJson<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let report = self.0;
        let mut json = serializer.serialize_struct("Diagnosis", 27)?;
        json.serialize_field("target", &report.target)?;
        json.serialize_field("targetId", &report.target_id)?;
        json.serialize_field("exists", &report.exists)?;
        json.serialize_field("patched", &report.patched)?;
        json.serialize_field("bundleId", &report.bundle_id)?;
        json.serialize_field("appVersion", &report.app_version)?;
        json.serialize_field("appBuild", &report.app_build)?;
        json.serialize_field("architecture", &report.architecture)?;
        json.serialize_field("asarFileHash", &report.asar_file_hash)?;
        json.serialize_field("asarHeaderHash", &report.asar_header_hash)?;
        json.serialize_field("plistFileHash", &report.plist_file_hash)?;
        json.serialize_field("plistIntegrityHash", &report.plist_integrity_hash)?;
        json.serialize_field("runtimeVersion", &report.runtime_version)?;
        json.serialize_field("originalMain", &report.original_main)?;
        json.serialize_field("codesignOk", &report.codesign_ok)?;
        json.serialize_field("backup", &report.backup)?;
        json.serialize_field("stalePid", &report.stale_pid)?;
        json.serialize_field("orphanSessions", &report.orphan_sessions)?;
        json.serialize_field("leftoverChromium", &report.leftover_chromium)?;
        json.serialize_field("asarLoaderOnly", &report.asar_loader_only)?;
        json.serialize_field("externalRuntime", &ExternalRuntimeJson(report))?;
        json.serialize_field("signing", &report.signing)?;
        json.serialize_field("spctl", &report.spctl)?;
        json.serialize_field("interruptedTransactions", &report.interrupted_transactions)?;
        json.serialize_field("journalRecords", &report.journal_records)?;
        json.serialize_field("checks", &report.checks)?;
        json.serialize_field("findings", &report.findings)?;
        json.end()
    }
}

struct ExternalRuntimeJson<'a>(&'a Diagnosis);

impl Serialize for ExternalRuntimeJson<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let report = self.0;
        let runtime = &report.external_runtime;
        let bundled = incodex_runtime_bundle::runtime_identity();
        let bundled_version = bundled.as_ref().map_or_else(
            |_| incodex_runtime_bundle::runtime_version(),
            |identity| identity.version.clone(),
        );
        let bundled_manifest = bundled
            .as_ref()
            .ok()
            .map(|identity| identity.manifest_sha256.as_str());
        let state = runtime_state(report);
        let matches_bundled = match state {
            RuntimeState::Current => Some(true),
            RuntimeState::Stale => Some(false),
            RuntimeState::Missing | RuntimeState::Invalid | RuntimeState::Unknown => None,
        };
        let mut json = serializer.serialize_struct("ExternalRuntime", 11)?;
        json.serialize_field("status", &runtime.status)?;
        json.serialize_field("present", &runtime.present)?;
        json.serialize_field("ok", &runtime.ok)?;
        json.serialize_field("version", &runtime.version)?;
        json.serialize_field("release", &runtime.release)?;
        json.serialize_field("error", &runtime.error)?;
        json.serialize_field("bundledVersion", &bundled_version)?;
        json.serialize_field("bundledManifestSha256", &bundled_manifest)?;
        json.serialize_field("manifestSha256", &deployed_manifest_hash(report))?;
        json.serialize_field("matchesBundled", &matches_bundled)?;
        json.serialize_field("state", state.as_str())?;
        json.end()
    }
}

fn runtime_description(report: &Diagnosis) -> String {
    if report.external_runtime.ok {
        format!(
            "{} {}",
            report
                .external_runtime
                .version
                .as_deref()
                .unwrap_or("unknown"),
            report.external_runtime.release.as_deref().unwrap_or("")
        )
        .trim()
        .to_string()
    } else if report.external_runtime.present {
        "invalid".to_string()
    } else {
        "missing".to_string()
    }
}

fn app_version_description(report: &Diagnosis) -> Option<String> {
    report.app_version.as_ref().map(|version| {
        format!("{} {}", version, report.app_build.as_deref().unwrap_or(""))
            .trim()
            .to_string()
    })
}

fn json_bool(value: &serde_json::Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn check_status(check: &CheckResult) -> &'static str {
    match check.status {
        CheckStatus::Checked => "checked",
        CheckStatus::Unknown => "unknown",
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RuntimeState {
    Current,
    Stale,
    Missing,
    Invalid,
    Unknown,
}

impl RuntimeState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Stale => "stale",
            Self::Missing => "missing",
            Self::Invalid => "invalid",
            Self::Unknown => "unknown",
        }
    }
}

fn runtime_state(report: &Diagnosis) -> RuntimeState {
    if !report.external_runtime.present {
        return RuntimeState::Missing;
    }
    if !report.external_runtime.ok {
        return RuntimeState::Invalid;
    }
    if report
        .checks
        .runtime
        .findings
        .iter()
        .any(|finding| finding.code == "runtime.stale")
    {
        return RuntimeState::Stale;
    }
    if report.checks.runtime.status == CheckStatus::Unknown {
        RuntimeState::Unknown
    } else {
        RuntimeState::Current
    }
}

fn runtime_state_description(report: &Diagnosis) -> &'static str {
    match runtime_state(report) {
        RuntimeState::Stale => "stale; run `incodex runtime`",
        state => state.as_str(),
    }
}

fn append_runtime_state(lines: &mut Vec<String>, report: &Diagnosis, include_manifest: bool) {
    let bundled = incodex_runtime_bundle::runtime_identity();
    lines.push(incodex_core::format_kv(
        "CLI Runtime",
        &bundled.as_ref().map_or_else(
            |_| incodex_runtime_bundle::runtime_version(),
            |identity| identity.version.clone(),
        ),
        None,
    ));
    if include_manifest {
        lines.push(incodex_core::format_kv(
            "CLI manifest",
            bundled
                .as_ref()
                .ok()
                .map(|identity| identity.manifest_sha256.as_str())
                .unwrap_or("unknown"),
            None,
        ));
        lines.push(incodex_core::format_kv(
            "Deployed manifest",
            deployed_manifest_description(report),
            None,
        ));
    }
    lines.push(incodex_core::format_kv(
        "Runtime state",
        runtime_state_description(report),
        None,
    ));
}

fn deployed_manifest_description(report: &Diagnosis) -> &str {
    if let Some(hash) = deployed_manifest_hash(report) {
        return hash;
    }
    match runtime_state(report) {
        RuntimeState::Missing => "not published",
        RuntimeState::Invalid => "invalid",
        RuntimeState::Unknown => "unknown",
        RuntimeState::Current | RuntimeState::Stale => "legacy content hashes",
    }
}

fn deployed_manifest_hash(report: &Diagnosis) -> Option<&str> {
    if !report.external_runtime.ok {
        return None;
    }
    let version = report.external_runtime.version.as_deref()?;
    let release = report.external_runtime.release.as_deref()?;
    let hash = release.strip_prefix(&format!("releases/{version}-"))?;
    let hash = (hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then_some(hash)?;
    // A verified modern pointer carrying the bundled manifest hash cannot be
    // stale: that manifest fixes both the version and every file hash. The
    // only valid stale case with this release suffix is a legacy pointer that
    // omitted provenance, so do not manufacture a manifest hash for it.
    if runtime_state(report) == RuntimeState::Stale
        && incodex_runtime_bundle::runtime_identity()
            .ok()
            .is_some_and(|identity| identity.manifest_sha256 == hash)
    {
        None
    } else {
        Some(hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnose::{ExternalRuntimeReport, InterruptedTransaction};
    use crate::diagnose_checks::{empty_checks, JournalRecord};

    #[test]
    fn diagnosis_omits_proof_line_when_proof_was_not_requested() {
        let report = Diagnosis {
            target: "/tmp/ChatGPT.app".into(),
            target_id: "app-test".into(),
            exists: true,
            patched: false,
            bundle_id: None,
            app_version: None,
            app_build: None,
            architecture: None,
            asar_file_hash: None,
            asar_header_hash: None,
            plist_file_hash: None,
            plist_integrity_hash: None,
            runtime_version: None,
            original_main: String::new(),
            codesign_ok: None,
            backup: None,
            stale_pid: false,
            orphan_sessions: Vec::new(),
            leftover_chromium: Vec::new(),
            asar_loader_only: None,
            external_runtime: ExternalRuntimeReport {
                status: CheckStatus::Unknown,
                present: false,
                ok: false,
                version: None,
                release: None,
                error: None,
            },
            signing: None,
            spctl: None,
            interrupted_transactions: Vec::<InterruptedTransaction>::new(),
            journal_records: vec![JournalRecord {
                kind: "completed".into(),
                path: "/tmp/transaction".into(),
                install_id: Some("test".into()),
                phase: Some("ROLLED_BACK".into()),
                action: Some("done".into()),
                error: None,
                retained_original: Some("/tmp/original/ChatGPT.app".into()),
                original_valid: None,
                artifacts: Vec::new(),
                recovery: Some("retained".into()),
            }],
            checks: empty_checks(),
            findings: Vec::new(),
        };

        let output = format_diagnosis(&report);

        assert!(output.contains("Retained original"), "{output}");
        assert!(!output.contains("Original proof"), "{output}");
    }
}
