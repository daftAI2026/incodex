#![cfg(target_os = "windows")]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_cli::windows_install::{
    install_windows_runtime_with, uninstall_windows_runtime_with, WindowsUninstallOutcome,
};
use incodex_cli::windows_install_state::{
    read_windows_install_state, read_windows_update_repair_intent,
};
use incodex_cli::windows_update_repair::{
    await_package_quiescence_with, classify_package_update,
    repair_windows_runtime_after_update_with, resume_windows_update_repair_with,
    PackageUpdateObservation, PackageUpdateOutcome, WindowsUpdateRepairAuthorization,
};

const FAMILY: &str = "OpenAI.Codex_2p2nqsd0c76g0";
const OLD_PACKAGE: &str = "OpenAI.Codex_26.820.9563.0_x64__2p2nqsd0c76g0";
const NEW_PACKAGE: &str = "OpenAI.Codex_26.825.3734.0_x64__2p2nqsd0c76g0";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn scratch_root() -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "incodex-windows-update-repair-{}-{sequence}",
        std::process::id()
    ))
}

fn observation(
    family: &str,
    target: &str,
    complete: bool,
    error_code: i32,
) -> PackageUpdateObservation {
    PackageUpdateObservation {
        source_package_full_name: OLD_PACKAGE.to_string(),
        target_package_full_name: target.to_string(),
        target_package_family_name: family.to_string(),
        complete,
        error_code,
    }
}

#[test]
fn retry_rejects_changed_helper_when_only_the_intent_remains() {
    let root = scratch_root();
    let installed = install_windows_runtime_with(
        &root,
        OLD_PACKAGE,
        &std::env::current_exe().unwrap(),
        |_| Ok(vec![]),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .unwrap();
    repair_windows_runtime_after_update_with(
        &root,
        WindowsUpdateRepairAuthorization {
            package_full_name: OLD_PACKAGE,
            epoch: installed.epoch,
            registration_id: &installed.registration_id,
            helper_source: &installed.helper_path,
        },
        NEW_PACKAGE,
        |_| Ok(vec![1234]),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .unwrap_err();
    let intent = read_windows_update_repair_intent(&root).unwrap().unwrap();
    fs::remove_file(root.join("windows-install.json")).unwrap();
    let mut bytes = fs::read(&intent.helper_path).unwrap();
    bytes.extend_from_slice(b"changed-after-intent");
    fs::write(&intent.helper_path, bytes).unwrap();
    let mut disabled = 0;
    let result = resume_windows_update_repair_with(
        &root,
        &intent,
        &intent.helper_path,
        |_| Ok(vec![]),
        |_| Ok(false),
        |_| {
            disabled += 1;
            Ok(())
        },
        |_| Ok(()),
    );
    assert!(
        result.is_err(),
        "changed helper must not be adopted by retry"
    );
    assert_eq!(disabled, 0);
    assert_eq!(
        read_windows_update_repair_intent(&root).unwrap(),
        Some(intent)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn only_a_successful_new_codex_generation_authorizes_repair() {
    assert_eq!(
        classify_package_update(
            FAMILY,
            OLD_PACKAGE,
            &observation("Other.App_publisher", NEW_PACKAGE, true, 0),
        ),
        PackageUpdateOutcome::Ignore,
    );
    assert_eq!(
        classify_package_update(
            FAMILY,
            OLD_PACKAGE,
            &observation(FAMILY, NEW_PACKAGE, false, 0),
        ),
        PackageUpdateOutcome::Updating,
    );
    assert_eq!(
        classify_package_update(
            FAMILY,
            OLD_PACKAGE,
            &observation(FAMILY, NEW_PACKAGE, true, -1),
        ),
        PackageUpdateOutcome::Failed,
    );
    assert_eq!(
        classify_package_update(
            FAMILY,
            OLD_PACKAGE,
            &observation(FAMILY, OLD_PACKAGE, true, 0),
        ),
        PackageUpdateOutcome::Ignore,
    );
    assert_eq!(
        classify_package_update(
            FAMILY,
            OLD_PACKAGE,
            &observation(FAMILY, NEW_PACKAGE, true, 0),
        ),
        PackageUpdateOutcome::Ready {
            target_package_full_name: NEW_PACKAGE.to_string(),
        },
    );
}

#[test]
fn repair_reuses_the_install_transaction_only_for_the_authorized_epoch() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let installed = install_windows_runtime_with(
        &user_root,
        OLD_PACKAGE,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install old Store generation");

    let error = repair_windows_runtime_after_update_with(
        &user_root,
        WindowsUpdateRepairAuthorization {
            package_full_name: OLD_PACKAGE,
            epoch: installed.epoch + 1,
            registration_id: &installed.registration_id,
            helper_source: &installed.helper_path,
        },
        NEW_PACKAGE,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| panic!("stale update worker must not disable registration"),
        |_| panic!("stale update worker must not enable registration"),
    )
    .expect_err("stale worker epoch must fail closed");
    assert!(error.contains("authorization changed"), "{error}");
    let retained = read_windows_install_state(&user_root)
        .expect("read retained state")
        .expect("old state remains");
    assert_eq!(retained.registration_id, installed.registration_id);

    let repaired = repair_windows_runtime_after_update_with(
        &user_root,
        WindowsUpdateRepairAuthorization {
            package_full_name: OLD_PACKAGE,
            epoch: installed.epoch,
            registration_id: &installed.registration_id,
            helper_source: &installed.helper_path,
        },
        NEW_PACKAGE,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |registration| {
            assert_eq!(registration.package_full_name(), NEW_PACKAGE);
            Ok(())
        },
    )
    .expect("repair current Store generation");
    assert_eq!(repaired.package_full_name, NEW_PACKAGE);

    fs::remove_dir_all(user_root).expect("remove update repair fixture");
}

fn selected_runtime_variant(root: &std::path::Path, original: &str, version: &str) -> String {
    use incodex_core::windows_session::{apply_private_windows_acl, ensure_private_windows_dir};
    use sha2::{Digest, Sha256};
    let releases = root.join("runtime/releases");
    let source = releases.join(original);
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(source.join("runtime-manifest.json")).unwrap()).unwrap();
    let mut inject = fs::read(source.join("incodex-inject.js")).unwrap();
    inject.extend_from_slice(b"\n// controlled Runtime B fixture\n");
    manifest["runtimeVersion"] = version.into();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    manifest["files"]["incodex-inject.js"] = digest(&inject).into();
    let body = serde_json::to_vec_pretty(&manifest).unwrap();
    let name = format!("{}-{}", version, digest(&body));
    let destination = ensure_private_windows_dir(&releases.join(&name)).unwrap();
    for entry in fs::read_dir(&source).unwrap() {
        let entry = entry.unwrap();
        let path = destination.join(entry.file_name());
        fs::copy(entry.path(), &path).unwrap();
        apply_private_windows_acl(&path).unwrap();
    }
    fs::write(destination.join("incodex-inject.js"), inject).unwrap();
    fs::write(destination.join("runtime-manifest.json"), body).unwrap();
    let pointer_path = root.join("runtime/current.json");
    let mut pointer: serde_json::Value =
        serde_json::from_slice(&fs::read(&pointer_path).unwrap()).unwrap();
    pointer["version"] = version.into();
    pointer["release"] = format!("releases/{name}").into();
    pointer["manifestSha256"] = name.strip_prefix(&format!("{version}-")).unwrap().into();
    pointer["files"] = manifest["files"].clone();
    fs::write(pointer_path, serde_json::to_vec_pretty(&pointer).unwrap()).unwrap();
    name
}

fn assert_repair_preserves_selected_runtime(version: &str) {
    let user_root = scratch_root();
    let helper = std::env::current_exe().unwrap();
    let initial = install_windows_runtime_with(
        &user_root,
        OLD_PACKAGE,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .unwrap();
    let selected = selected_runtime_variant(&user_root, &initial.runtime_release, version);
    let installed =
        incodex_cli::windows_install_state::synchronize_windows_install_runtime_release(
            &user_root, &selected,
        )
        .expect("B is a valid recorded Runtime")
        .unwrap();
    let pointer_before = fs::read(user_root.join("runtime/current.json")).unwrap();
    let result = repair_windows_runtime_after_update_with(
        &user_root,
        WindowsUpdateRepairAuthorization {
            package_full_name: OLD_PACKAGE,
            epoch: installed.epoch,
            registration_id: &installed.registration_id,
            helper_source: &installed.helper_path,
        },
        NEW_PACKAGE,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("rebind must not republish embedded Runtime A");
    assert_eq!(result.runtime_release, selected);
    assert_eq!(
        fs::read(user_root.join("runtime/current.json")).unwrap(),
        pointer_before
    );
    fs::remove_dir_all(user_root).unwrap();
}

#[test]
fn repair_preserves_a_newer_selected_runtime() {
    assert_repair_preserves_selected_runtime("9.9.9");
}

#[test]
fn retry_rejects_a_runtime_selection_changed_after_the_intent() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().unwrap();
    let initial = install_windows_runtime_with(
        &user_root,
        OLD_PACKAGE,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .unwrap();
    repair_windows_runtime_after_update_with(
        &user_root,
        WindowsUpdateRepairAuthorization {
            package_full_name: OLD_PACKAGE,
            epoch: initial.epoch,
            registration_id: &initial.registration_id,
            helper_source: &initial.helper_path,
        },
        NEW_PACKAGE,
        |package| {
            Ok(if package == NEW_PACKAGE {
                vec![1234]
            } else {
                vec![]
            })
        },
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect_err("running target leaves a retained intent");
    let intent = read_windows_update_repair_intent(&user_root)
        .unwrap()
        .unwrap();
    let selected = selected_runtime_variant(&user_root, &initial.runtime_release, "9.9.9");
    let updated = incodex_cli::windows_install_state::synchronize_windows_install_runtime_release(
        &user_root, &selected,
    )
    .unwrap()
    .unwrap();
    let retry = resume_windows_update_repair_with(
        &user_root,
        &intent,
        &initial.helper_path,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    );
    assert!(
        retry.is_err(),
        "stale intent must not replace the later selected Runtime"
    );
    assert_eq!(
        read_windows_install_state(&user_root).unwrap(),
        Some(updated)
    );
    fs::remove_dir_all(user_root).unwrap();
}

#[test]
fn repair_preserves_a_same_version_runtime_with_a_different_hash() {
    assert_repair_preserves_selected_runtime(env!("CARGO_PKG_VERSION"));
}

#[test]
fn repair_rejects_a_corrupted_selected_runtime_before_retiring_old_state() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let installed = install_windows_runtime_with(
        &user_root,
        OLD_PACKAGE,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install old Store generation");
    let selected_runtime_file = user_root
        .join("runtime")
        .join("releases")
        .join(&installed.runtime_release)
        .join("incodex-inject.js");
    assert!(
        selected_runtime_file.is_file(),
        "published Runtime includes the selected injection artifact"
    );
    fs::write(selected_runtime_file, b"corrupted selected Runtime")
        .expect("corrupt selected Runtime");

    let mut disable_calls = 0;
    let error = repair_windows_runtime_after_update_with(
        &user_root,
        WindowsUpdateRepairAuthorization {
            package_full_name: OLD_PACKAGE,
            epoch: installed.epoch,
            registration_id: &installed.registration_id,
            helper_source: &installed.helper_path,
        },
        NEW_PACKAGE,
        |_| Ok(Vec::new()),
        |package| {
            assert_eq!(package, OLD_PACKAGE);
            Ok(true)
        },
        |_| {
            disable_calls += 1;
            Ok(())
        },
        |_| Ok(()),
    )
    .expect_err("repair must reject a corrupted selected Runtime");

    let retained =
        read_windows_install_state(&user_root).expect("read install state after rejected repair");
    assert_eq!(
        (
            retained
                .as_ref()
                .map(|state| state.registration_id.as_str()),
            disable_calls,
        ),
        (Some(installed.registration_id.as_str()), 0),
        "repair error: {error}"
    );

    fs::remove_dir_all(user_root).expect("remove update repair fixture");
}

#[test]
fn stale_coordinator_cannot_cross_an_uninstall_and_same_generation_reinstall() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let first = install_windows_runtime_with(
        &user_root,
        OLD_PACKAGE,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install first registration");
    let old_registration_id = first.registration_id.clone();
    let old_helper_path = first.helper_path.clone();
    let old_epoch = first.epoch;

    assert_eq!(
        uninstall_windows_runtime_with(&user_root, |_| Ok(Vec::new()), |_| Ok(false), |_| Ok(()),)
            .expect("uninstall first registration"),
        WindowsUninstallOutcome::Removed,
    );
    let second = install_windows_runtime_with(
        &user_root,
        OLD_PACKAGE,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install replacement registration");
    assert_ne!(second.registration_id, old_registration_id);

    let error = repair_windows_runtime_after_update_with(
        &user_root,
        WindowsUpdateRepairAuthorization {
            package_full_name: OLD_PACKAGE,
            epoch: old_epoch,
            registration_id: &old_registration_id,
            helper_source: &old_helper_path,
        },
        NEW_PACKAGE,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| panic!("stale coordinator must not disable the replacement registration"),
        |_| panic!("stale coordinator must not enable a new registration"),
    )
    .expect_err("stale registration authorization must fail closed");
    assert!(error.contains("authorization changed"), "{error}");
    let retained = read_windows_install_state(&user_root)
        .expect("read replacement state")
        .expect("replacement state remains");
    assert_eq!(retained.registration_id, second.registration_id);

    fs::remove_dir_all(user_root).expect("remove update repair fixture");
}

#[test]
fn interrupted_repair_retains_a_durable_update_intent() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let installed = install_windows_runtime_with(
        &user_root,
        OLD_PACKAGE,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install old Store generation");

    let error = repair_windows_runtime_after_update_with(
        &user_root,
        WindowsUpdateRepairAuthorization {
            package_full_name: OLD_PACKAGE,
            epoch: installed.epoch,
            registration_id: &installed.registration_id,
            helper_source: &installed.helper_path,
        },
        NEW_PACKAGE,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Err("injected registration failure".to_string()),
    )
    .expect_err("injected repair failure must propagate");
    assert!(error.contains("injected registration failure"), "{error}");

    let intent = read_windows_update_repair_intent(&user_root)
        .expect("read durable update intent")
        .expect("failed repair retains update intent");
    assert_eq!(intent.source_registration_id, installed.registration_id);
    assert_eq!(intent.source_package_full_name, OLD_PACKAGE);
    assert_eq!(intent.target_package_full_name, NEW_PACKAGE);

    let resumed = install_windows_runtime_with(
        &user_root,
        NEW_PACKAGE,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(true),
        |_| Ok(()),
        |registration| {
            assert_eq!(registration.package_full_name(), NEW_PACKAGE);
            Ok(())
        },
    )
    .expect("a later install process consumes the durable repair intent");
    assert_eq!(resumed.package_full_name, NEW_PACKAGE);
    assert!(
        read_windows_update_repair_intent(&user_root)
            .expect("read completed update intent")
            .is_none(),
        "successful recovery retires the update intent"
    );

    fs::remove_dir_all(user_root).expect("remove update repair fixture");
}

#[test]
fn successful_uninstall_cancels_an_interrupted_update_intent() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let installed = install_windows_runtime_with(
        &user_root,
        OLD_PACKAGE,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install old Store generation");
    repair_windows_runtime_after_update_with(
        &user_root,
        WindowsUpdateRepairAuthorization {
            package_full_name: OLD_PACKAGE,
            epoch: installed.epoch,
            registration_id: &installed.registration_id,
            helper_source: &installed.helper_path,
        },
        NEW_PACKAGE,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Err("injected registration failure".to_string()),
    )
    .expect_err("leave an interrupted update repair");

    assert_eq!(
        uninstall_windows_runtime_with(&user_root, |_| Ok(Vec::new()), |_| Ok(true), |_| Ok(()),)
            .expect("uninstall interrupted repair"),
        WindowsUninstallOutcome::Removed,
    );
    assert!(
        read_windows_update_repair_intent(&user_root)
            .expect("read cancelled update intent")
            .is_none(),
        "successful uninstall retires update intent"
    );

    fs::remove_dir_all(user_root).expect("remove update repair fixture");
}

#[test]
fn automatic_retry_cannot_reverse_a_completed_uninstall() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let installed = install_windows_runtime_with(
        &user_root,
        OLD_PACKAGE,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install old Store generation");
    repair_windows_runtime_after_update_with(
        &user_root,
        WindowsUpdateRepairAuthorization {
            package_full_name: OLD_PACKAGE,
            epoch: installed.epoch,
            registration_id: &installed.registration_id,
            helper_source: &installed.helper_path,
        },
        NEW_PACKAGE,
        |package| {
            Ok((package == NEW_PACKAGE)
                .then_some(1234)
                .into_iter()
                .collect())
        },
        |_| Ok(false),
        |_| panic!("running target must stop repair before disable"),
        |_| panic!("running target must stop repair before enable"),
    )
    .expect_err("running target leaves a resumable intent");
    let intent = read_windows_update_repair_intent(&user_root)
        .expect("read update intent")
        .expect("update intent remains");

    assert_eq!(
        uninstall_windows_runtime_with(&user_root, |_| Ok(Vec::new()), |_| Ok(false), |_| Ok(()),)
            .expect("user uninstall wins"),
        WindowsUninstallOutcome::Removed,
    );
    let error = resume_windows_update_repair_with(
        &user_root,
        &intent,
        &installed.helper_path,
        |_| panic!("cancelled retry must not inspect processes"),
        |_| panic!("cancelled retry must not inspect packages"),
        |_| panic!("cancelled retry must not disable registration"),
        |_| panic!("cancelled retry must not enable registration"),
    )
    .expect_err("cancelled intent must not reinstall");
    assert!(error.contains("intent changed"), "{error}");
    assert!(
        read_windows_install_state(&user_root)
            .expect("read cancelled install state")
            .is_none(),
        "cancelled retry leaves integration uninstalled"
    );

    fs::remove_dir_all(user_root).expect("remove update repair fixture");
}

#[test]
fn automatic_retry_checks_the_target_before_retiring_old_state() {
    let user_root = scratch_root();
    let helper = std::env::current_exe().expect("test helper path");
    let installed = install_windows_runtime_with(
        &user_root,
        OLD_PACKAGE,
        &helper,
        |_| Ok(Vec::new()),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .expect("install old Store generation");
    repair_windows_runtime_after_update_with(
        &user_root,
        WindowsUpdateRepairAuthorization {
            package_full_name: OLD_PACKAGE,
            epoch: installed.epoch,
            registration_id: &installed.registration_id,
            helper_source: &installed.helper_path,
        },
        NEW_PACKAGE,
        |package| {
            Ok((package == NEW_PACKAGE)
                .then_some(1234)
                .into_iter()
                .collect())
        },
        |_| Ok(false),
        |_| panic!("running target must stop repair before disable"),
        |_| panic!("running target must stop repair before enable"),
    )
    .expect_err("running target leaves old registration intact");
    let intent = read_windows_update_repair_intent(&user_root)
        .expect("read update intent")
        .expect("update intent remains");

    let error = resume_windows_update_repair_with(
        &user_root,
        &intent,
        &installed.helper_path,
        |package| {
            Ok((package == NEW_PACKAGE)
                .then_some(5678)
                .into_iter()
                .collect())
        },
        |_| Ok(false),
        |_| panic!("retry must not disable while target runs"),
        |_| panic!("retry must not enable while target runs"),
    )
    .expect_err("running target blocks retry before mutation");
    assert!(error.contains("close Codex"), "{error}");
    let retained = read_windows_install_state(&user_root)
        .expect("read retained old state")
        .expect("old state remains");
    assert_eq!(retained.registration_id, installed.registration_id);

    fs::remove_dir_all(user_root).expect("remove update repair fixture");
}

#[test]
fn auto_relaunched_target_is_waited_by_handle_before_repair() {
    let mut probes = 0;
    let mut waited = Vec::new();
    await_package_quiescence_with(
        NEW_PACKAGE,
        |package| {
            assert_eq!(package, NEW_PACKAGE);
            probes += 1;
            Ok(if probes == 1 {
                vec![42, 84]
            } else {
                Vec::new()
            })
        },
        |process_ids| {
            waited.push(process_ids.to_vec());
            Ok(())
        },
    )
    .expect("deferred target eventually becomes quiescent");

    assert_eq!(probes, 2);
    assert_eq!(waited, vec![vec![42, 84]]);
}
