// Runtime 发布必须交接已授权的 Store 更新，不改变保留 helper 的校验协议。
use super::*;
use crate::windows_update_prearm::{promote_prearmed_update_with, PrearmedLaunch};
use incodex_core::windows_session::apply_private_windows_acl;
use sha2::{Digest, Sha256};

#[path = "../tests/fixtures/windows-prearm-consumer-v1.4.rs"]
mod frozen_v1_4;

const OLD: &str = "OpenAI.Codex_1.2.3.4_x64__2p2nqsd0c76g0";
const NEW: &str = "OpenAI.Codex_1.2.3.5_x64__2p2nqsd0c76g0";

struct Fixture {
    root: PathBuf,
    source: WindowsInstallState,
    candidate: String,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "incodex-runtime-handoff-{}-{}",
            std::process::id(),
            random_registration_id().unwrap()
        ));
        let source = crate::windows_install::install_windows_runtime_with(
            &root,
            OLD,
            &std::env::current_exe().unwrap(),
            |_| Ok(vec![]),
            |_| Ok(false),
            |_| Ok(()),
            |_| Ok(()),
        )
        .unwrap();
        let candidate = runtime_variant(&root, &source.runtime_release);
        Self {
            root,
            source,
            candidate,
        }
    }

    fn prearm(&self) -> WindowsUpdateRepairIntent {
        stage_windows_update_prearm_intent(&self.root, &self.source, NEW).unwrap()
    }

    fn sync(&self) -> Result<WindowsInstallState, String> {
        synchronize_windows_install_runtime_release(&self.root, &self.candidate)
            .map(|state| state.unwrap())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn runtime_variant(root: &Path, original: &str) -> String {
    let releases = root.join("runtime/releases");
    let source = releases.join(original);
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(source.join("runtime-manifest.json")).unwrap()).unwrap();
    manifest["runtimeVersion"] = "9.9.9".into();
    let body = serde_json::to_vec_pretty(&manifest).unwrap();
    let digest: String = Sha256::digest(&body)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let name = format!("9.9.9-{digest}");
    let destination = ensure_private_windows_dir(&releases.join(&name)).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let path = destination.join(entry.file_name());
        fs::copy(entry.path(), &path).unwrap();
        apply_private_windows_acl(&path).unwrap();
    }
    fs::write(destination.join("runtime-manifest.json"), body).unwrap();
    name
}

#[test]
fn runtime_handoff_keeps_prearmed_first_launch_authorized() {
    let fixture = Fixture::new();
    let old = fixture.prearm();
    let helper_before = fs::read(&fixture.source.helper_path).unwrap();
    let registration_before = fs::read(fixture.root.join("windows-registration.json")).unwrap();
    let selected = fixture.sync().unwrap();
    let intent = read_windows_update_prearm_intent(&fixture.root)
        .unwrap()
        .unwrap();
    assert!(
        intent.authorizes_source(&selected, NEW),
        "published Runtime stranded the prepared launch"
    );
    assert_eq!(intent.operation_id, old.operation_id);
    let source_bytes = fs::read(&selected.state_path).unwrap();
    assert!(!frozen_v1_4::accepts(
        &source_bytes,
        &serde_json::to_vec(&old).unwrap(),
        NEW
    ));
    assert!(frozen_v1_4::accepts(
        &source_bytes,
        &fs::read(&intent.intent_path).unwrap(),
        NEW
    ));
    assert_eq!(
        intent.target_package_full_name,
        old.target_package_full_name
    );
    assert_eq!(fs::read(&selected.helper_path).unwrap(), helper_before);
    assert_eq!(
        fs::read(fixture.root.join("windows-registration.json")).unwrap(),
        registration_before
    );
    let installed = promote_prearmed_update_with(
        PrearmedLaunch {
            root: &fixture.root,
            helper: &selected.helper_path,
            target: NEW,
            held_pid: 42,
        },
        || Ok(()),
        |package| Ok(if package == NEW { vec![42] } else { vec![] }),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .unwrap()
    .unwrap();
    assert_eq!(installed.registration_id, old.operation_id);
    assert_eq!(installed.runtime_release, fixture.candidate);
}

#[test]
fn runtime_handoff_same_version_repairs_legacy_pending_without_rotating_owner() {
    let fixture = Fixture::new();
    let old = fixture.prearm();
    let mut already_updated = fixture.source.clone();
    already_updated.epoch += 3;
    already_updated.runtime_release = fixture.candidate.clone();
    write_state(&fixture.root, &already_updated).unwrap();
    let selected = fixture.sync().unwrap();
    let repaired = read_windows_update_prearm_intent(&fixture.root)
        .unwrap()
        .unwrap();
    assert!(
        repaired.authorizes_source(&selected, NEW),
        "current-version repair left the old prearm stranded"
    );
    assert_eq!(selected, already_updated);
    assert_eq!(repaired.operation_id, old.operation_id);
}

#[test]
fn runtime_handoff_keeps_the_running_app_coordinator_authorized() {
    let fixture = Fixture::new();
    let selected = fixture.sync().unwrap();
    assert_eq!(
        selected.epoch, fixture.source.epoch,
        "Runtime content must not revoke the installed App coordinator"
    );
    let restored = crate::windows_update_repair::repair_windows_runtime_after_update_with(
        &fixture.root,
        crate::windows_update_repair::WindowsUpdateRepairAuthorization {
            package_full_name: OLD,
            epoch: fixture.source.epoch,
            registration_id: &fixture.source.registration_id,
            helper_source: &fixture.source.helper_path,
        },
        NEW,
        |_| Ok(vec![]),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(restored.runtime_release, fixture.candidate);
}

#[test]
fn runtime_handoff_rebinds_retained_repair_but_rejects_its_old_snapshot() {
    let fixture = Fixture::new();
    let prearm = fixture.prearm();
    let old = stage_windows_update_repair_intent(&fixture.root, &fixture.source, NEW).unwrap();
    let selected = fixture.sync().unwrap();
    let repaired = read_windows_update_repair_intent(&fixture.root)
        .unwrap()
        .unwrap();
    assert!(repaired.authorizes_source(&selected, NEW));
    assert_eq!(repaired.operation_id, prearm.operation_id);
    assert!(
        crate::windows_update_repair::resume_windows_update_repair_with(
            &fixture.root,
            &old,
            &selected.helper_path,
            |_| panic!("stale caller must not inspect processes"),
            |_| panic!("stale caller must not inspect packages"),
            |_| panic!("stale caller must not disable"),
            |_| panic!("stale caller must not enable"),
        )
        .is_err()
    );
    let installed = crate::windows_update_repair::resume_windows_update_repair_with(
        &fixture.root,
        &repaired,
        &selected.helper_path,
        |_| Ok(vec![]),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(installed.runtime_release, fixture.candidate);
}

#[test]
fn runtime_handoff_refuses_foreign_pending_before_changing_any_state() {
    for field in [
        "registration",
        "helper",
        "source",
        "future-epoch",
        "future-runtime",
        "target",
    ] {
        let fixture = Fixture::new();
        let mut intent = fixture.prearm();
        match field {
            "registration" => intent.source_registration_id = "a".repeat(32),
            "helper" => intent.helper_sha256 = "a".repeat(64),
            "source" => {
                intent.source_package_full_name = "OpenAI.Codex_1.2.3.3_x64__2p2nqsd0c76g0".into()
            }
            "future-epoch" => intent.source_epoch += 2,
            "future-runtime" => intent.runtime_release = format!("10.0.0-{}", "a".repeat(64)),
            "target" => intent.target_package_full_name = "Other.App_1.2.3.5_x64__publisher".into(),
            _ => unreachable!(),
        }
        write_update_repair_intent(&fixture.root, &intent).unwrap();
        let state_before = fs::read(&fixture.source.state_path).unwrap();
        let intent_before = fs::read(&intent.intent_path).unwrap();
        assert!(fixture.sync().is_err(), "must not adopt {field} mismatch");
        assert_eq!(fs::read(&fixture.source.state_path).unwrap(), state_before);
        assert_eq!(fs::read(&intent.intent_path).unwrap(), intent_before);
    }
}

#[test]
fn runtime_handoff_completes_a_prepared_write_after_interruption() {
    let fixture = Fixture::new();
    let mut prepared = fixture.prearm();
    prepared.runtime_release = fixture.candidate.clone();
    write_update_repair_intent(&fixture.root, &prepared).unwrap();
    let selected = fixture.sync().unwrap();
    let intent = read_windows_update_prearm_intent(&fixture.root)
        .unwrap()
        .unwrap();
    assert!(intent.authorizes_source(&selected, NEW));
    assert_eq!(intent, prepared);
}

#[test]
fn runtime_handoff_rejects_corrupt_assets_without_losing_pending() {
    let fixture = Fixture::new();
    let pending = fixture.prearm();
    fs::write(
        fixture
            .root
            .join("runtime/releases")
            .join(&fixture.candidate)
            .join("incodex-inject.js"),
        b"corrupt",
    )
    .unwrap();
    assert!(fixture.sync().is_err());
    assert_eq!(
        read_windows_install_state(&fixture.root).unwrap(),
        Some(fixture.source.clone())
    );
    assert_eq!(
        read_windows_update_prearm_intent(&fixture.root).unwrap(),
        Some(pending)
    );
}

#[test]
fn runtime_handoff_interrupted_upgrade_preserves_a_usable_old_launch_route() {
    let fixture = Fixture::new();
    let old = fixture.prearm();
    stage_windows_update_repair_intent(&fixture.root, &fixture.source, NEW).unwrap();
    let mut writes = 0;
    let failure =
        runtime_handoff::synchronize_with(&fixture.root, &fixture.candidate, |root, intent| {
            writes += 1;
            if writes == 2 {
                return Err("fixture interrupted between pending writes".into());
            }
            write_update_repair_intent(root, intent)
        });
    assert!(failure.is_err());
    let state = read_windows_install_state(&fixture.root).unwrap().unwrap();
    assert_eq!(state, fixture.source);
    let prearm = read_windows_update_prearm_intent(&fixture.root)
        .unwrap()
        .unwrap();
    assert!(
        prearm.authorizes_source(&state, NEW),
        "old helper lost its one usable route before state commit"
    );
    let adopted = promote_prearmed_update_with(
        PrearmedLaunch {
            root: &fixture.root,
            helper: &state.helper_path,
            target: NEW,
            held_pid: 42,
        },
        || Ok(()),
        |package| Ok(if package == NEW { vec![42] } else { vec![] }),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .unwrap()
    .unwrap();
    assert_eq!(adopted.registration_id, old.operation_id);
    let resumed = fixture.sync().unwrap();
    assert_eq!(resumed.package_full_name, NEW);
    assert_eq!(resumed.runtime_release, fixture.candidate);
}

#[test]
fn runtime_handoff_rejects_changed_registration_disabled_source_and_corrupt_old_assets() {
    for fault in ["registration", "disabled", "old-assets"] {
        let fixture = Fixture::new();
        let pending = fixture.prearm();
        match fault {
            "registration" => {
                let path = fixture.root.join("windows-registration.json");
                let mut value: serde_json::Value =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                value["registrationId"] = "a".repeat(32).into();
                replace_private_file(&fixture.root, &path, &serde_json::to_vec(&value).unwrap())
                    .unwrap();
            }
            "disabled" => {
                transition_windows_install_state(
                    &fixture.root,
                    fixture.source.epoch,
                    WindowsInstallPhase::DisableRequested,
                )
                .unwrap();
            }
            "old-assets" => {
                fs::write(
                    fixture
                        .root
                        .join("runtime/releases")
                        .join(&fixture.source.runtime_release)
                        .join("incodex-inject.js"),
                    b"corrupt old assets",
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let before = fs::read(&fixture.source.state_path).unwrap();
        let pending_before = fs::read(&pending.intent_path).unwrap();
        assert!(fixture.sync().is_err(), "must reject {fault}");
        assert_eq!(fs::read(&fixture.source.state_path).unwrap(), before);
        assert_eq!(fs::read(&pending.intent_path).unwrap(), pending_before);
    }
}

#[test]
fn runtime_handoff_retries_after_state_write_failure_and_old_coordinator_rebind() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
    for with_repair in [false, true] {
        let fixture = Fixture::new();
        let prearm = fixture.prearm();
        if with_repair {
            stage_windows_update_repair_intent(&fixture.root, &fixture.source, NEW).unwrap();
        }
        let mut state_lock = None;
        let failure =
            runtime_handoff::synchronize_with(&fixture.root, &fixture.candidate, |root, intent| {
                write_update_repair_intent(root, intent)?;
                if intent.intent_path == prearm.intent_path {
                    state_lock = Some(
                        fs::OpenOptions::new()
                            .read(true)
                            .share_mode(FILE_SHARE_READ)
                            .open(&fixture.source.state_path)
                            .unwrap(),
                    );
                }
                Ok(())
            });
        assert!(
            failure.is_err(),
            "deny-delete handle must prevent the state commit"
        );
        drop(state_lock);
        assert_eq!(
            read_windows_install_state(&fixture.root).unwrap(),
            Some(fixture.source.clone())
        );
        let restored = crate::windows_update_repair::repair_windows_runtime_after_update_with(
            &fixture.root,
            crate::windows_update_repair::WindowsUpdateRepairAuthorization {
                package_full_name: OLD,
                epoch: fixture.source.epoch,
                registration_id: &fixture.source.registration_id,
                helper_source: &fixture.source.helper_path,
            },
            NEW,
            |_| Ok(vec![]),
            |_| Ok(false),
            |_| Ok(()),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(restored.package_full_name, NEW);
        assert_eq!(restored.runtime_release, fixture.source.runtime_release);
        assert!(read_windows_update_prearm_intent(&fixture.root)
            .unwrap()
            .is_some());
        let selected = fixture
            .sync()
            .expect("explicit retry must finish the content handoff after fallback rebind");
        assert_eq!(selected.package_full_name, NEW);
        assert_eq!(selected.registration_id, restored.registration_id);
        assert_eq!(selected.epoch, restored.epoch);
        assert_eq!(selected.runtime_release, fixture.candidate);
        assert!(read_windows_update_prearm_intent(&fixture.root)
            .unwrap()
            .is_none());
    }
}

#[test]
fn runtime_handoff_retains_completed_target_when_registration_is_unproven() {
    let fixture = Fixture::new();
    let prearm = fixture.prearm();
    crate::windows_update_repair::repair_windows_runtime_after_update_with(
        &fixture.root,
        crate::windows_update_repair::WindowsUpdateRepairAuthorization {
            package_full_name: OLD,
            epoch: fixture.source.epoch,
            registration_id: &fixture.source.registration_id,
            helper_source: &fixture.source.helper_path,
        },
        NEW,
        |_| Ok(vec![]),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .unwrap();
    fs::remove_file(fixture.root.join("windows-registration.json")).unwrap();
    let source_before = fs::read(&fixture.source.state_path).unwrap();
    let prearm_before = fs::read(&prearm.intent_path).unwrap();
    assert!(fixture.sync().is_err());
    assert_eq!(fs::read(&fixture.source.state_path).unwrap(), source_before);
    assert_eq!(fs::read(&prearm.intent_path).unwrap(), prearm_before);
}
