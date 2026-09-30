// 提前挂钩只持久记录来源授权；首次精确挂起的目标进程沿用已有安装事务完成换代。
use std::path::Path;

use crate::windows_activation::WindowsInstalledRuntimeRegistration;
use crate::windows_install_state::{
    acquire_windows_install_state, read_windows_install_state, read_windows_update_prearm_intent,
    retire_windows_update_prearm_intent, stage_windows_update_prearm_intent, WindowsInstallPhase,
    WindowsInstallState, WindowsUpdateRepairIntent,
};
use crate::windows_update_repair::PackageUpdateObservation;

pub(crate) fn source_authorizes_prearm(
    source: &WindowsInstallState,
    intent: &WindowsUpdateRepairIntent,
    helper: &Path,
    target: &str,
) -> bool {
    source.desired_enabled()
        && matches!(
            source.phase,
            WindowsInstallPhase::EnabledObserved | WindowsInstallPhase::EnabledUnobserved
        )
        && source.package_full_name == intent.source_package_full_name
        && source.epoch == intent.source_epoch
        && source.registration_id == intent.source_registration_id
        && source.helper_path == helper
        && source.helper_path == intent.helper_path
        && source.helper_sha256 == intent.helper_sha256
        && source.runtime_release == intent.runtime_release
        && intent.target_package_full_name == target
        && source.package_full_name != target
}

pub(crate) fn prepare_update_with(
    root: &Path,
    helper: &Path,
    observation: &PackageUpdateObservation,
    enable: impl FnOnce(&WindowsInstalledRuntimeRegistration) -> Result<(), String>,
) -> Result<bool, String> {
    let _gate = acquire_windows_install_state()?;
    let Some(source) = read_windows_install_state(root)? else {
        return Ok(false);
    };
    if !source.desired_enabled()
        || !matches!(
            source.phase,
            WindowsInstallPhase::EnabledObserved | WindowsInstallPhase::EnabledUnobserved
        )
        || observation.error_code != 0
        || observation.target_package_family_name != crate::windows_app::CODEX_PACKAGE_FAMILY_NAME
        || source.package_full_name != observation.source_package_full_name
        || source.helper_path != helper
        || source.package_full_name == observation.target_package_full_name
    {
        return Ok(false);
    }
    crate::windows_app::validate_codex_package_full_name(&observation.target_package_full_name)?;
    crate::windows_runtime::verify_installed_windows_runtime(root, &source.runtime_release)?;
    // 与常规恢复 intent 分开：旧窗口还活着时，不能让完成后协调器消费未注册的目标。
    let existing = read_windows_update_prearm_intent(root)?;
    let intent = match existing {
        Some(intent)
            if source_authorizes_prearm(
                &source,
                &intent,
                helper,
                &observation.target_package_full_name,
            ) =>
        {
            intent
        }
        Some(_) => {
            return Err("Windows prearm intent changed; retain it for authorized recovery".into())
        }
        None => stage_windows_update_prearm_intent(
            root,
            &source,
            &observation.target_package_full_name,
        )?,
    };
    // 复用产品生成的短路径、命令行与 Runtime 环境；不使用诊断脚本的 debugger 命令。
    let mut target = source.clone();
    target.package_full_name = intent.target_package_full_name.clone();
    enable(&WindowsInstalledRuntimeRegistration::from_install_state(
        &target,
    )?)?;
    Ok(true)
}

pub(crate) struct PrearmedLaunch<'a> {
    pub root: &'a Path,
    pub helper: &'a Path,
    pub target: &'a str,
    pub held_pid: u32,
}

pub(crate) fn promote_prearmed_update_with<R, P, D, E>(
    launch: PrearmedLaunch<'_>,
    mut prove_held: impl FnMut() -> Result<(), String>,
    mut inspect: R,
    package_is_installed: P,
    disable: D,
    enable: E,
) -> Result<Option<WindowsInstallState>, String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnMut(&str) -> Result<bool, String>,
    D: FnMut(&str) -> Result<(), String>,
    E: FnOnce(&WindowsInstalledRuntimeRegistration) -> Result<(), String>,
{
    let PrearmedLaunch {
        root,
        helper,
        target,
        held_pid,
    } = launch;
    let _gate = acquire_windows_install_state()?;
    let Some(intent) = read_windows_update_prearm_intent(root)? else {
        return Ok(None);
    };
    let source =
        read_windows_install_state(root)?.ok_or("Windows prearm source authorization is absent")?;
    if !source_authorizes_prearm(&source, &intent, helper, target) || held_pid == 0 {
        return Err("Windows prearm source authorization changed".into());
    }
    prove_held()?;
    let installed = crate::windows_update_repair::repair_windows_runtime_after_update_with(
        root,
        crate::windows_update_repair::WindowsUpdateRepairAuthorization {
            package_full_name: &source.package_full_name,
            epoch: source.epoch,
            registration_id: &source.registration_id,
            helper_source: helper,
        },
        target,
        |package| {
            // 只豁免当前精确已证挂起的启动进程；每次事务检查都复核，不放行其它 PID。
            prove_held().map_err(std::io::Error::other)?;
            let mut pids = inspect(package)?;
            if package == target {
                pids.retain(|pid| *pid != held_pid);
            }
            Ok(pids)
        },
        package_is_installed,
        disable,
        enable,
    )?;
    retire_windows_update_prearm_intent(root, Some(&intent.operation_id))?;
    Ok(Some(installed))
}

pub(crate) fn cancel_prearmed_update_with(
    root: &Path,
    mut inspect: impl FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    disable: &mut impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    let Some(intent) = read_windows_update_prearm_intent(root)? else {
        return Ok(());
    };
    if !inspect(&intent.target_package_full_name)
        .map_err(|error| error.to_string())?
        .is_empty()
    {
        return Err("close Codex before removing the prearmed Windows update target".into());
    }
    disable(&intent.target_package_full_name)?;
    retire_windows_update_prearm_intent(root, Some(&intent.operation_id))
}

pub(crate) fn retire_applied_prearm(
    root: &Path,
    installed: &WindowsInstallState,
) -> Result<(), String> {
    if let Some(intent) = read_windows_update_prearm_intent(root)? {
        if installed.desired_enabled()
            && matches!(
                installed.phase,
                WindowsInstallPhase::EnabledObserved | WindowsInstallPhase::EnabledUnobserved
            )
            && installed.package_full_name == intent.target_package_full_name
            && installed.helper_path == intent.helper_path
            && installed.helper_sha256 == intent.helper_sha256
            && installed.runtime_release == intent.runtime_release
        {
            // 常规恢复已完成时只退役 pending 证据，不能 Disable 正在使用的新代挂钩。
            retire_windows_update_prearm_intent(root, Some(&intent.operation_id))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows_install_state::{
        read_windows_update_repair_intent, transition_windows_install_state,
    };
    use std::cell::Cell;
    use std::path::PathBuf;

    const OLD: &str = "OpenAI.Codex_1.2.3.4_x64__2p2nqsd0c76g0";
    const NEW: &str = "OpenAI.Codex_1.2.3.5_x64__2p2nqsd0c76g0";
    struct Fixture {
        root: PathBuf,
        state: WindowsInstallState,
    }
    impl Fixture {
        fn launch(&self) -> PrearmedLaunch<'_> {
            PrearmedLaunch {
                root: &self.root,
                helper: &self.state.helper_path,
                target: NEW,
                held_pid: 42,
            }
        }
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "incodex-prearm-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let state = crate::windows_install::install_windows_runtime_with(
                &root,
                OLD,
                &std::env::current_exe().unwrap(),
                |_| Ok(vec![]),
                |_| Ok(false),
                |_| Ok(()),
                |_| Ok(()),
            )
            .unwrap();
            Self { root, state }
        }
        fn prearm(&self) {
            prepare_update_with(
                &self.root,
                &self.state.helper_path,
                &observation(),
                |registration| {
                    assert_eq!(registration.package_full_name(), NEW);
                    assert!(
                        read_windows_update_prearm_intent(&self.root)
                            .unwrap()
                            .is_some(),
                        "durable authorization must precede OS hook"
                    );
                    assert!(
                        read_windows_update_repair_intent(&self.root)
                            .unwrap()
                            .is_none(),
                        "staged target must not masquerade as a completed repair"
                    );
                    Ok(())
                },
            )
            .unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn observation() -> PackageUpdateObservation {
        PackageUpdateObservation {
            source_package_full_name: OLD.into(),
            target_package_full_name: NEW.into(),
            target_package_family_name: crate::windows_app::CODEX_PACKAGE_FAMILY_NAME.into(),
            complete: false,
            error_code: 0,
        }
    }

    #[test]
    fn prearm_persists_exact_authorization_before_enable_without_changing_the_source() {
        let fixture = Fixture::new();
        fixture.prearm();
        assert_eq!(
            read_windows_install_state(&fixture.root).unwrap(),
            Some(fixture.state.clone())
        );
        let intent = read_windows_update_prearm_intent(&fixture.root)
            .unwrap()
            .unwrap();
        assert!(source_authorizes_prearm(
            &fixture.state,
            &intent,
            &fixture.state.helper_path,
            NEW
        ));
        for index in 0..5 {
            let mut changed = fixture.state.clone();
            match index {
                0 => changed.epoch += 1,
                1 => changed.registration_id = "f".repeat(32),
                2 => changed.helper_sha256 = "f".repeat(64),
                3 => changed.runtime_release = "changed".into(),
                _ => changed.phase = WindowsInstallPhase::DisableRequested,
            }
            assert!(!source_authorizes_prearm(
                &changed,
                &intent,
                &fixture.state.helper_path,
                NEW
            ));
        }
    }

    #[test]
    fn unrelated_failed_and_same_generation_events_never_enable_a_target() {
        let fixture = Fixture::new();
        for index in 0..4 {
            let mut event = observation();
            match index {
                0 => event.error_code = -1,
                1 => event.target_package_family_name = "Other.App".into(),
                2 => event.source_package_full_name = NEW.into(),
                _ => event.target_package_full_name = OLD.into(),
            }
            assert!(!prepare_update_with(
                &fixture.root,
                &fixture.state.helper_path,
                &event,
                |_| panic!("unrelated event cannot prearm")
            )
            .unwrap());
        }
        assert!(read_windows_update_prearm_intent(&fixture.root)
            .unwrap()
            .is_none());
    }

    #[test]
    fn first_suspended_target_is_adopted_through_the_existing_transaction() {
        let fixture = Fixture::new();
        fixture.prearm();
        let proofs = Cell::new(0);
        let installed = promote_prearmed_update_with(
            fixture.launch(),
            || {
                proofs.set(proofs.get() + 1);
                Ok(())
            },
            |package| Ok(if package == NEW { vec![42] } else { vec![] }),
            |_| Ok(false),
            |_| Ok(()),
            |_| Ok(()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(installed.package_full_name, NEW);
        assert_ne!(installed.registration_id, fixture.state.registration_id);
        assert_eq!(installed.runtime_release, fixture.state.runtime_release);
        assert!(
            proofs.get() > 1,
            "prove held PID at each transactional check"
        );
        assert!(read_windows_update_prearm_intent(&fixture.root)
            .unwrap()
            .is_none());
        assert!(read_windows_update_repair_intent(&fixture.root)
            .unwrap()
            .is_none());
    }

    #[test]
    fn another_running_target_is_never_exempted() {
        let fixture = Fixture::new();
        fixture.prearm();
        let result = promote_prearmed_update_with(
            fixture.launch(),
            || Ok(()),
            |package| Ok(if package == NEW { vec![42, 43] } else { vec![] }),
            |_| Ok(false),
            |_| panic!("must not disable with another running target"),
            |_| panic!("must not install with another running target"),
        );
        assert!(result.is_err());
        assert_eq!(
            read_windows_install_state(&fixture.root).unwrap(),
            Some(fixture.state.clone())
        );
        assert!(read_windows_update_prearm_intent(&fixture.root)
            .unwrap()
            .is_some());
    }

    #[test]
    fn a_resumed_target_or_changed_source_is_not_adopted() {
        let fixture = Fixture::new();
        fixture.prearm();
        assert!(promote_prearmed_update_with(
            fixture.launch(),
            || Err("target no longer suspended".into()),
            |_| panic!("must prove before inspecting"),
            |_| Ok(false),
            |_| panic!("must not disable"),
            |_| panic!("must not enable")
        )
        .is_err());
        transition_windows_install_state(
            &fixture.root,
            fixture.state.epoch,
            WindowsInstallPhase::EnabledObserved,
        )
        .unwrap();
        assert!(promote_prearmed_update_with(
            fixture.launch(),
            || panic!("stale source rejected before process access"),
            |_| Ok(vec![]),
            |_| Ok(false),
            |_| panic!("must not disable"),
            |_| panic!("must not enable")
        )
        .is_err());
    }

    #[test]
    fn uninstall_cancels_only_a_quiescent_pending_target_and_retains_failed_cancellation() {
        let fixture = Fixture::new();
        fixture.prearm();
        assert!(
            cancel_prearmed_update_with(&fixture.root, |_| Ok(vec![42]), &mut |_| panic!(
                "must not touch running official app"
            ))
            .is_err()
        );
        assert!(
            cancel_prearmed_update_with(&fixture.root, |_| Ok(vec![]), &mut |_| Err(
                "OS disable failed".into()
            ))
            .is_err()
        );
        assert!(read_windows_update_prearm_intent(&fixture.root)
            .unwrap()
            .is_some());
        cancel_prearmed_update_with(&fixture.root, |_| Ok(vec![]), &mut |target: &str| {
            assert_eq!(target, NEW);
            Ok(())
        })
        .unwrap();
        assert!(read_windows_update_prearm_intent(&fixture.root)
            .unwrap()
            .is_none());
        assert_eq!(
            read_windows_install_state(&fixture.root).unwrap(),
            Some(fixture.state.clone())
        );
    }
}
