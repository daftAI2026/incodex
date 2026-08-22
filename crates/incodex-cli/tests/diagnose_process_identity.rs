use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_cli::diagnose::{diagnose_with_root_mode, DiagnosisMode};
use serde_json::Value;

static ENV_LOCK: Mutex<()> = Mutex::new(());
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

const CANONICAL_START: &str = "Sat Aug 22 10:37:03 2025";
const LEGACY_START: &str = "六  8月/22 10:37:03 2025";

struct EnvironmentGuard {
    path: Option<OsString>,
    locale: Option<OsString>,
}

impl EnvironmentGuard {
    fn install(fake_bin: &Path) -> Self {
        let guard = Self {
            path: std::env::var_os("PATH"),
            locale: std::env::var_os("LC_ALL"),
        };
        let mut path = OsString::from(fake_bin.as_os_str());
        path.push(":");
        if let Some(existing) = std::env::var_os("PATH") {
            path.push(existing);
        }
        std::env::set_var("PATH", path);
        std::env::set_var("LC_ALL", "zh_CN.UTF-8");
        guard
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        match &self.path {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
        match &self.locale {
            Some(locale) => std::env::set_var("LC_ALL", locale),
            None => std::env::remove_var("LC_ALL"),
        }
    }
}

fn fixture_root(label: &str) -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "incodex-diagnose-process-identity-{label}-{}-{timestamp}-{sequence}",
        std::process::id()
    ))
}

fn fake_ps(root: &Path) -> PathBuf {
    let bin = root.join("fake-bin");
    fs::create_dir_all(&bin).unwrap();
    let ps = bin.join("ps");
    fs::write(
        &ps,
        format!(
            "#!/bin/sh\nif [ \"$LC_ALL\" = \"C\" ]; then\n  printf '%s\\n' '{} /usr/bin/fake-child'\nelse\n  printf '%s\\n' '{} /usr/bin/fake-child'\nfi\n",
            CANONICAL_START, LEGACY_START
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&ps).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&ps, permissions).unwrap();
    bin
}

fn write_owner_records(root: &Path, process_start_identity: &str) {
    let pid = std::process::id();
    let session = root.join("sessions/s-locale");
    fs::create_dir_all(&session).unwrap();
    fs::write(
        session.join("owner.json"),
        serde_json::to_vec(&serde_json::json!({
            "sessionId": "s-locale",
            "pid": pid,
            "processStartIdentity": process_start_identity,
        }))
        .unwrap(),
    )
    .unwrap();

    let target = root.join("targets/fixture");
    fs::create_dir_all(&target).unwrap();
    fs::write(
        target.join("incognito.lock"),
        serde_json::to_vec(&serde_json::json!({
            "pid": pid,
            "processStartIdentity": process_start_identity,
            "execIdentity": "/usr/bin/fake-child",
        }))
        .unwrap(),
    )
    .unwrap();
}

fn diagnose(root: &Path, mode: DiagnosisMode) -> Value {
    let app = root.join("Missing.app");
    serde_json::to_value(diagnose_with_root_mode(&app, root, mode)).unwrap()
}

#[test]
fn status_and_doctor_pin_ps_to_c_locale_for_live_session_and_owner() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let root = fixture_root("canonical");
    let fake_bin = fake_ps(&root);
    write_owner_records(&root, CANONICAL_START);
    let _environment = EnvironmentGuard::install(&fake_bin);

    for mode in [DiagnosisMode::Status, DiagnosisMode::Doctor] {
        let report = diagnose(&root, mode);
        assert_eq!(report["stalePid"], false, "mode={mode:?}");
        assert!(report["orphanSessions"].as_array().unwrap().is_empty());
        assert_eq!(report["checks"]["processIdentity"]["status"], "checked");
        assert_eq!(report["checks"]["orphanSessions"]["status"], "checked");
        assert!(!report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| {
                finding["code"] == "owner.identity-mismatch" || finding["code"] == "session.orphan"
            }));
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn status_and_doctor_retain_unparseable_legacy_session_and_owner_identity() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let root = fixture_root("legacy");
    let fake_bin = fake_ps(&root);
    write_owner_records(&root, LEGACY_START);
    let _environment = EnvironmentGuard::install(&fake_bin);

    for mode in [DiagnosisMode::Status, DiagnosisMode::Doctor] {
        let report = diagnose(&root, mode);
        assert_eq!(report["stalePid"], false, "mode={mode:?}");
        assert!(report["orphanSessions"].as_array().unwrap().is_empty());
        assert_eq!(report["checks"]["processIdentity"]["status"], "unknown");
        assert_eq!(report["checks"]["orphanSessions"]["status"], "unknown");
        let findings = report["findings"].as_array().unwrap();
        assert!(findings
            .iter()
            .any(|finding| { finding["code"] == "owner.identity-unknown" }));
        assert!(findings
            .iter()
            .any(|finding| { finding["code"] == "session.identity-unknown" }));
        assert!(!findings.iter().any(|finding| {
            finding["code"] == "owner.identity-mismatch" || finding["code"] == "session.orphan"
        }));
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn status_and_doctor_retain_pending_session_with_dead_launcher() {
    let root = fixture_root("pending");
    let session = root.join("sessions/s-pending");
    fs::create_dir_all(session.join("chromium")).unwrap();
    fs::write(
        session.join("owner.json"),
        serde_json::to_vec(&serde_json::json!({
            "sessionId": "s-pending",
            "pid": 999_999_999_i64,
            "processStartIdentity": CANONICAL_START,
            "handoffPending": true,
        }))
        .unwrap(),
    )
    .unwrap();

    for mode in [DiagnosisMode::Status, DiagnosisMode::Doctor] {
        let report = diagnose(&root, mode);
        assert_eq!(report["stalePid"], false, "mode={mode:?}");
        assert!(report["orphanSessions"].as_array().unwrap().is_empty());
        assert!(report["leftoverChromium"].as_array().unwrap().is_empty());
        assert_eq!(report["checks"]["orphanSessions"]["status"], "unknown");
        assert_eq!(report["checks"]["chromiumResidue"]["status"], "unknown");
        assert!(!report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| {
                finding["code"] == "session.orphan" || finding["code"] == "chromium.residue"
            }));
    }

    let _ = fs::remove_dir_all(root);
}
