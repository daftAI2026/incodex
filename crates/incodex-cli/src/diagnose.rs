use std::path::Path;

use serde::Serialize;

use incodex_core::paths::{user_root, ASAR_REL, RUNTIME_CURRENT_NAME, RUNTIME_DIR_NAME};
use incodex_core::target_id;
use incodex_transaction::{list_journals, recover_action, Recovery};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalRuntimeReport {
    pub present: bool,
    pub ok: bool,
    pub version: Option<String>,
    pub release: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterruptedTransaction {
    pub install_id: String,
    pub phase: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    pub target: String,
    pub target_id: String,
    pub exists: bool,
    pub patched: bool,
    pub bundle_id: Option<String>,
    pub app_version: Option<String>,
    pub app_build: Option<String>,
    pub architecture: Option<String>,
    pub asar_file_hash: Option<String>,
    pub asar_header_hash: Option<String>,
    pub plist_file_hash: Option<String>,
    pub plist_integrity_hash: Option<String>,
    pub runtime_version: Option<String>,
    pub original_main: String,
    pub codesign_ok: bool,
    pub backup: Option<serde_json::Value>,
    pub stale_pid: bool,
    pub orphan_sessions: Vec<String>,
    pub leftover_chromium: Vec<String>,
    pub asar_loader_only: Option<bool>,
    pub external_runtime: ExternalRuntimeReport,
    pub signing: Option<serde_json::Value>,
    pub spctl: Option<serde_json::Value>,
    pub interrupted_transactions: Vec<InterruptedTransaction>,
}

pub fn diagnose(app_path: &Path) -> Diagnosis {
    diagnose_with_root(app_path, &user_root())
}

pub fn diagnose_with_root(app_path: &Path, root: &Path) -> Diagnosis {
    let exists = app_path.exists();
    let asar_path = app_path.join(ASAR_REL);
    let asar_exists = asar_path.exists();
    Diagnosis {
        target: app_path.display().to_string(),
        target_id: target_id(app_path),
        exists,
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
        codesign_ok: false,
        backup: None,
        stale_pid: false,
        orphan_sessions: Vec::new(),
        leftover_chromium: Vec::new(),
        asar_loader_only: if asar_exists { Some(false) } else { None },
        external_runtime: inspect_external_runtime(root),
        signing: None,
        spctl: None,
        interrupted_transactions: list_journals(root)
            .into_iter()
            .filter(|journal| recover_action(journal) != Recovery::Done)
            .map(|journal| {
                let action = recover_action(&journal).as_str().to_string();
                InterruptedTransaction {
                    install_id: journal.install_id,
                    phase: journal.phase,
                    action,
                }
            })
            .collect(),
    }
}

fn inspect_external_runtime(root: &Path) -> ExternalRuntimeReport {
    let current = root.join(RUNTIME_DIR_NAME).join(RUNTIME_CURRENT_NAME);
    if !current.exists() {
        return ExternalRuntimeReport {
            present: false,
            ok: false,
            version: None,
            release: None,
            error: Some("missing current.json".to_string()),
        };
    }
    ExternalRuntimeReport {
        present: true,
        ok: false,
        version: None,
        release: None,
        error: Some("invalid current.json".to_string()),
    }
}

pub fn format_status(app_path: &Path) -> String {
    let mut lines = vec![incodex_core::format_step("Status", None)];
    if !app_path.exists() {
        lines.push(incodex_core::format_warn(
            &format!("Codex app not found: {}", app_path.display()),
            None,
        ));
        lines.push(String::new());
        return lines.join("\n");
    }
    let report = diagnose(app_path);
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
    lines.push(incodex_core::format_kv("Runtime", runtime_label(&report), None));
    lines.push(incodex_core::format_kv("Target", &report.target_id, None));
    if !app_path.join(ASAR_REL).exists() {
        lines.push(incodex_core::format_warn("asar missing", None));
    }
    lines.push(String::new());
    lines.join("\n")
}

pub fn format_diagnosis(report: &Diagnosis) -> String {
    let runtime = if report.external_runtime.ok {
        format!(
            "{} {}",
            report.external_runtime.version.as_deref().unwrap_or("unknown"),
            report.external_runtime.release.as_deref().unwrap_or("")
        )
        .trim()
        .to_string()
    } else if report.external_runtime.present {
        "invalid".to_string()
    } else {
        "missing".to_string()
    };
    let backup = if report.backup.is_some() {
        "incomplete"
    } else {
        "none"
    };
    let loader = match report.asar_loader_only {
        None => "unknown",
        Some(true) => "asar only",
        Some(false) => "mixed",
    };
    let mut lines = vec![
        incodex_core::format_step("App", None),
        incodex_core::format_kv("Path", &report.target, None),
        incodex_core::format_kv("Exists", if report.exists { "yes" } else { "no" }, None),
        incodex_core::format_kv(
            "Installed",
            if report.patched { "yes" } else { "no" },
            None,
        ),
        incodex_core::format_kv("Bundle", report.bundle_id.as_deref().unwrap_or("unknown"), None),
        incodex_core::format_kv(
            "Version",
            &format!(
                "{} {}",
                report.app_version.as_deref().unwrap_or("unknown"),
                report.app_build.as_deref().unwrap_or("")
            )
            .trim()
            .to_string(),
            None,
        ),
        incodex_core::format_kv("Arch", report.architecture.as_deref().unwrap_or("unknown"), None),
        String::new(),
        incodex_core::format_step("Runtime", None),
        incodex_core::format_kv(
            "Version",
            report.runtime_version.as_deref().unwrap_or("unknown"),
            None,
        ),
        incodex_core::format_kv("External", &runtime, None),
    ];
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
        incodex_core::format_kv("Verify", if report.codesign_ok { "ok" } else { "failed" }, None),
        String::new(),
        incodex_core::format_step("Backup", None),
        incodex_core::format_kv("State", backup, None),
        String::new(),
        incodex_core::format_step("Sessions", None),
        incodex_core::format_kv("Orphans", &report.orphan_sessions.len().to_string(), None),
        incodex_core::format_kv("Chromium", &report.leftover_chromium.len().to_string(), None),
        incodex_core::format_kv("Stale pid", if report.stale_pid { "yes" } else { "no" }, None),
        incodex_core::format_kv(
            "Journals",
            &report.interrupted_transactions.len().to_string(),
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
    if !report.interrupted_transactions.is_empty() {
        lines.push(incodex_core::format_warn(
            "Old install journals are leftover. They do not mean the current app is broken.",
            None,
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

pub fn diagnosis_json(report: &Diagnosis) -> String {
    format!("{}\n", serde_json::to_string_pretty(report).expect("json"))
}

fn runtime_label(report: &Diagnosis) -> &str {
    if report.external_runtime.ok {
        "ok"
    } else if report.external_runtime.present {
        "invalid"
    } else {
        "missing"
    }
}
