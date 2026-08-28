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
    classify_package_update, repair_windows_runtime_after_update_with, PackageUpdateObservation,
    PackageUpdateOutcome, WindowsUpdateRepairAuthorization,
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
