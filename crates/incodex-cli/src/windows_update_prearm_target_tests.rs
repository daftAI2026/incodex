use super::tests::{observation, Fixture, NEW, OLD};
/**
 * [INPUT]: 依赖 windows_update_prearm 的现有 prepare 接口、repair 的泛型回调与隔离 Fixture。
 * [OUTPUT]: 提供目标前移、迟到事件、来源授权和普通 repair 交接的回归断言。
 * [POS]: Windows 预挂钩事务的边界测试；以虚拟包和闭集回调验证授权及 pending 生命周期。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;

const NEXT: &str = "OpenAI.Codex_1.2.3.6_x64__2p2nqsd0c76g0";
const FOLLOWING: &str = "OpenAI.Codex_1.2.3.7_x64__2p2nqsd0c76g0";

type UpdateObservation = crate::windows_update_repair::PackageUpdateObservation;

#[test]
fn matching_target_with_another_registration_cannot_consume_prearm() {
    let fixture = Fixture::new();
    fixture.prearm();
    let (path, before) = prearm_snapshot(&fixture);
    let mut installed = fixture.state.clone();
    installed.package_full_name = NEW.into();
    retire_applied_prearm(&fixture.root, &installed).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn matching_target_and_reserved_registration_consume_prearm() {
    let fixture = Fixture::new();
    fixture.prearm();
    let intent = read_windows_update_prearm_intent(&fixture.root)
        .unwrap()
        .unwrap();
    let mut installed = fixture.state.clone();
    installed.package_full_name = NEW.into();
    installed.registration_id = intent.operation_id;
    retire_applied_prearm(&fixture.root, &installed).unwrap();
    assert!(read_windows_update_prearm_intent(&fixture.root)
        .unwrap()
        .is_none());
}

fn update_observation(source: &str, target: &str) -> UpdateObservation {
    let mut event = observation();
    event.source_package_full_name = source.into();
    event.target_package_full_name = target.into();
    event
}

fn prearm_snapshot(fixture: &Fixture) -> (std::path::PathBuf, Vec<u8>) {
    let intent = read_windows_update_prearm_intent(&fixture.root)
        .unwrap()
        .expect("fixture must have a durable prearm intent");
    let path = intent.intent_path;
    let bytes = std::fs::read(&path).unwrap();
    (path, bytes)
}

#[test]
fn same_source_forward_update_retargets_prearm_with_a_new_operation() {
    let _runtime = crate::windows_update_repair::WindowsRuntimeApartment::initialize()
        .expect("initialize WinRT before native package inspection");
    let fixture = Fixture::new();
    fixture.prearm();
    let source_before = read_windows_install_state(&fixture.root).unwrap();
    let old_intent = read_windows_update_prearm_intent(&fixture.root)
        .unwrap()
        .unwrap();
    let event = update_observation(OLD, NEXT);
    let mut enabled = false;

    let prepared = prepare_update_with(
        &fixture.root,
        &fixture.state.helper_path,
        &event,
        |registration| {
            enabled = true;
            assert_eq!(registration.package_full_name(), NEXT);
            let staged = read_windows_update_prearm_intent(&fixture.root)
                .unwrap()
                .unwrap();
            assert_eq!(staged.target_package_full_name, NEXT);
            assert_ne!(staged.operation_id, old_intent.operation_id);
            Ok(())
        },
    )
    .expect("a later generation from the same authorized source must advance the prearm");

    assert!(prepared, "the exact forward event must be accepted");
    assert!(enabled, "the new target registration must be prepared");
    let current = read_windows_update_prearm_intent(&fixture.root)
        .unwrap()
        .unwrap();
    assert_eq!(current.target_package_full_name, NEXT);
    assert_ne!(current.operation_id, old_intent.operation_id);
    assert_eq!(
        read_windows_install_state(&fixture.root).unwrap(),
        source_before
    );
}

#[test]
fn ordinary_repair_retires_old_prearm_and_allows_the_next_generation() {
    let _runtime = crate::windows_update_repair::WindowsRuntimeApartment::initialize()
        .expect("initialize WinRT for the virtual package fixture");
    let fixture = Fixture::new();
    fixture.prearm();
    let old_intent = read_windows_update_prearm_intent(&fixture.root)
        .unwrap()
        .unwrap();
    let authorization = crate::windows_update_repair::WindowsUpdateRepairAuthorization {
        package_full_name: &fixture.state.package_full_name,
        epoch: fixture.state.epoch,
        registration_id: &fixture.state.registration_id,
        helper_source: &fixture.state.helper_path,
    };

    let installed = crate::windows_update_repair::repair_windows_runtime_after_update_with(
        &fixture.root,
        authorization,
        NEXT,
        |_| Ok(vec![]),
        |_| Ok(false),
        |package| {
            assert!(package == OLD || package == NEW || package == NEXT);
            Ok(())
        },
        |registration| {
            assert_eq!(registration.package_full_name(), NEXT);
            Ok(())
        },
    )
    .expect("ordinary repair callbacks are isolated stubs");

    assert_eq!(installed.package_full_name, NEXT);
    assert_eq!(
        read_windows_install_state(&fixture.root).unwrap(),
        Some(installed.clone())
    );
    assert!(
        read_windows_update_prearm_intent(&fixture.root)
            .unwrap()
            .is_none(),
        "the superseded OLD-to-NEW authorization must be retired"
    );

    let next_event = update_observation(NEXT, FOLLOWING);
    let prepared = prepare_update_with(
        &fixture.root,
        &installed.helper_path,
        &next_event,
        |registration| {
            assert_eq!(registration.package_full_name(), FOLLOWING);
            Ok(())
        },
    )
    .expect("the repaired generation must be able to prearm its successor");
    assert!(prepared);
    let successor = read_windows_update_prearm_intent(&fixture.root)
        .unwrap()
        .unwrap();
    assert_eq!(successor.source_package_full_name, NEXT);
    assert_eq!(successor.target_package_full_name, FOLLOWING);
    assert_ne!(successor.operation_id, old_intent.operation_id);
}

#[test]
fn changed_source_identity_cannot_replace_foreign_pending_bytes() {
    let _runtime = crate::windows_update_repair::WindowsRuntimeApartment::initialize()
        .expect("initialize WinRT before package inspection");
    let fixture = Fixture::new();
    fixture.prearm();
    let (path, pending_before) = prearm_snapshot(&fixture);

    let current = read_windows_install_state(&fixture.root).unwrap().unwrap();
    let changed = crate::windows_install_state::transition_windows_install_state(
        &fixture.root,
        current.epoch,
        WindowsInstallPhase::EnabledObserved,
    )
    .unwrap();
    assert_ne!(changed.epoch, current.epoch);

    let result = prepare_update_with(
        &fixture.root,
        &fixture.state.helper_path,
        &update_observation(OLD, NEXT),
        |_| panic!("a changed source identity cannot enable a replacement target"),
    );
    assert!(matches!(result, Err(_) | Ok(false)));
    assert_eq!(std::fs::read(path).unwrap(), pending_before);
    let pending = read_windows_update_prearm_intent(&fixture.root)
        .unwrap()
        .unwrap();
    assert_eq!(pending.target_package_full_name, NEW);
}

#[test]
fn late_same_source_event_cannot_roll_back_target_or_operation() {
    let _runtime = crate::windows_update_repair::WindowsRuntimeApartment::initialize()
        .expect("initialize WinRT before package inspection");
    let fixture = Fixture::new();
    let advanced = update_observation(OLD, NEXT);
    assert!(prepare_update_with(
        &fixture.root,
        &fixture.state.helper_path,
        &advanced,
        |registration| {
            assert_eq!(registration.package_full_name(), NEXT);
            Ok(())
        },
    )
    .unwrap());
    let (path, pending_before) = prearm_snapshot(&fixture);
    let operation_before = read_windows_update_prearm_intent(&fixture.root)
        .unwrap()
        .unwrap()
        .operation_id;

    let late_old_event = prepare_update_with(
        &fixture.root,
        &fixture.state.helper_path,
        &observation(),
        |_| panic!("a stale event must not enable its older target"),
    );
    assert!(matches!(late_old_event, Err(_) | Ok(false)));
    assert_eq!(std::fs::read(path).unwrap(), pending_before);
    let pending = read_windows_update_prearm_intent(&fixture.root)
        .unwrap()
        .unwrap();
    assert_eq!(pending.target_package_full_name, NEXT);
    assert_eq!(pending.operation_id, operation_before);
}
