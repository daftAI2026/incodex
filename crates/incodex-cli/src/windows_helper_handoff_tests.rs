/*
 * [INPUT]: 依赖真实私有文件/PE 发布、既有安装 fixture 和闭集观察器/登记替身。
 * [OUTPUT]: 验证热交接、同一授权的 pending 保留、故障续接与卸载取消。
 * [POS]: 原生 helper 换代事务回归；不替代真实 Store 或宿主 ACK 验收。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;

const OLD: &str = "OpenAI.Codex_1.2.3.4_x64__2p2nqsd0c76g0";
const NEW: &str = "OpenAI.Codex_1.2.3.5_x64__2p2nqsd0c76g0";

struct Fixture {
    root: PathBuf,
    source: WindowsInstallState,
    executable: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "incodex-helper-handoff-{}-{}",
            std::process::id(),
            random_registration_id().unwrap()
        ));
        let source = crate::windows_install::install_windows_runtime_with(
            &path,
            OLD,
            &std::env::current_exe().unwrap(),
            |_| Ok(vec![]),
            |_| Ok(false),
            |_| Ok(()),
            |_| Ok(()),
        )
        .unwrap();
        let root = fs::canonicalize(path).unwrap();
        let executable = root.join("candidate.exe");
        let mut bytes = fs::read(std::env::current_exe().unwrap()).unwrap();
        bytes.extend_from_slice(b"native-candidate-fixture");
        fs::write(&executable, bytes).unwrap();
        incodex_core::windows_session::apply_private_windows_acl(&executable).unwrap();
        Self {
            root,
            source,
            executable,
        }
    }
    fn backend(&self) -> Fake {
        Fake {
            old: self.source.helper_path.clone(),
            registered: self.source.helper_path.clone(),
            package: OLD.into(),
            root: self.root.clone(),
            compatible: false,
            disabled: false,
            source_idle: true,
            live_owner: false,
            fail_prepare: false,
            fail_ack: false,
            fail_login: false,
            fail_enable: false,
            preserved: false,
            stops: 0,
            prepares: 0,
            restarts: 0,
            enables: vec![],
        }
    }
    fn prearm(&self) -> WindowsUpdateRepairIntent {
        let prearm = stage_windows_update_prearm_intent(&self.root, &self.source, NEW).unwrap();
        stage_windows_update_repair_intent(&self.root, &self.source, NEW).unwrap();
        prearm
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

struct Fake {
    old: PathBuf,
    registered: PathBuf,
    package: String,
    root: PathBuf,
    compatible: bool,
    disabled: bool,
    source_idle: bool,
    live_owner: bool,
    fail_prepare: bool,
    fail_ack: bool,
    fail_login: bool,
    fail_enable: bool,
    preserved: bool,
    stops: usize,
    prepares: usize,
    restarts: usize,
    enables: Vec<String>,
}
impl Backend for Fake {
    type Observer = u32;
    fn current_package(&mut self) -> Result<String, String> {
        Ok(self.package.clone())
    }
    fn source_is_idle(&mut self, _: &str) -> Result<bool, String> {
        Ok(self.source_idle)
    }
    fn registered(&mut self, helper: &Path) -> Result<bool, String> {
        Ok(!self.disabled && helper == self.registered)
    }
    fn compatible(&mut self, helper: &Path) -> Result<bool, String> {
        Ok(self.compatible || helper != self.old)
    }
    fn supports_live_owner(&mut self, _: &Path) -> Result<bool, String> {
        Ok(self.live_owner)
    }
    fn stop(&mut self, _: &Path) -> Result<(), String> {
        self.stops += 1;
        self.preserved = false;
        Ok(())
    }
    fn login(
        &mut self,
        source: &WindowsInstallState,
        target: &WindowsInstallState,
    ) -> Result<(), String> {
        if self.fail_login {
            assert!(
                self.preserved,
                "committed observer must survive login write failure"
            );
            return Err("login write failed".into());
        }
        if self.disabled
            || (self.registered != source.helper_path && self.registered != target.helper_path)
        {
            return Err("login changed".into());
        }
        self.registered = target.helper_path.clone();
        Ok(())
    }
    fn enable(&mut self, state: &WindowsInstallState) -> Result<(), String> {
        if self.fail_enable {
            assert!(
                self.preserved,
                "resuming a committed target must preserve its observer before fallible work"
            );
            return Err("enable failed".into());
        }
        self.enables.push(state.package_full_name.clone());
        Ok(())
    }
    fn prepare(&mut self, _: &WindowsInstallState) -> Result<u32, String> {
        self.preserved = false;
        self.prepares += 1;
        if self.fail_prepare {
            Err("prepare failed".into())
        } else {
            Ok(4242)
        }
    }
    fn preserve(&mut self, _: &mut u32) {
        self.preserved = true;
    }
    fn active(&mut self, pid: u32) -> Result<u32, String> {
        if self.fail_ack {
            return Err("no active ACK".into());
        }
        let state = read_windows_install_state(&self.root)?.unwrap();
        if state.helper_path != self.registered {
            return Err("activation preceded commit".into());
        }
        Ok(pid)
    }
    fn restart_source(&mut self, state: &WindowsInstallState) -> Result<(), String> {
        self.restarts += 1;
        assert_eq!(self.registered, state.helper_path);
        Ok(())
    }
}

#[test]
fn hot_handoff_activates_the_new_helper_without_changing_install_epoch() {
    let f = Fixture::new();
    let mut b = f.backend();
    let result = refresh_with(&f.root, &f.executable, &mut b).unwrap();
    let current = read_windows_install_state(&f.root).unwrap().unwrap();
    assert_eq!(result, Some(4242));
    assert_ne!(current.helper_sha256, f.source.helper_sha256);
    assert_eq!(current.epoch, f.source.epoch);
    assert_eq!(current.registration_id, f.source.registration_id);
    assert_eq!(current.runtime_release, f.source.runtime_release);
    assert_eq!(current.helper_path, b.registered);
    assert!(!f.root.join(JOURNAL).exists());
}

#[test]
fn hot_handoff_preserves_the_next_update_operation() {
    let f = Fixture::new();
    let intent = f.prearm();
    let mut b = f.backend();
    refresh_with(&f.root, &f.executable, &mut b).unwrap();
    let current = read_windows_install_state(&f.root).unwrap().unwrap();
    for item in [
        read_windows_update_prearm_intent(&f.root).unwrap().unwrap(),
        read_windows_update_repair_intent(&f.root).unwrap().unwrap(),
    ] {
        assert_eq!(item.operation_id, intent.operation_id);
        assert_eq!(item.target_package_full_name, NEW);
        assert!(item.authorizes_source(&current, NEW));
    }
    assert_eq!(current.epoch, f.source.epoch);
}

#[test]
fn captured_update_can_arm_future_launches_of_an_already_running_new_package() {
    let f = Fixture::new();
    let intent = f.prearm();
    let mut b = f.backend();
    b.package = NEW.into();
    refresh_with(&f.root, &f.executable, &mut b).unwrap();
    let current = read_windows_install_state(&f.root).unwrap().unwrap();
    assert_eq!(current.package_full_name, NEW);
    assert_eq!(current.registration_id, intent.operation_id);
    assert_eq!(current.epoch, f.source.epoch);
    assert_eq!(current.phase, WindowsInstallPhase::EnabledUnobserved);
    assert!(read_windows_update_prearm_intent(&f.root)
        .unwrap()
        .is_none());
    assert!(read_windows_update_repair_intent(&f.root)
        .unwrap()
        .is_none());
}

#[test]
fn compatible_helper_and_user_disabled_observer_are_retained() {
    for disabled in [false, true] {
        let f = Fixture::new();
        let mut b = f.backend();
        b.compatible = !disabled;
        b.disabled = disabled;
        assert_eq!(refresh_with(&f.root, &f.executable, &mut b).unwrap(), None);
        assert_eq!(
            read_windows_install_state(&f.root).unwrap(),
            Some(f.source.clone())
        );
        assert_eq!((b.stops, b.prepares), (0, 0));
    }
}

#[test]
fn foreign_pending_cannot_be_rewritten_by_a_helper_refresh() {
    let f = Fixture::new();
    let mut intent = f.prearm();
    let mut b = f.backend();
    intent.source_registration_id = "ffffffffffffffffffffffffffffffff".into();
    write_update_repair_intent(&f.root, &intent).unwrap();
    let before = fs::read(&intent.intent_path).unwrap();
    assert!(refresh_with(&f.root, &f.executable, &mut b).is_err());
    assert_eq!(fs::read(&intent.intent_path).unwrap(), before);
    assert_eq!(b.stops, 0);
}

#[test]
fn failed_preparation_retains_a_journal_that_the_same_transaction_can_resume() {
    let f = Fixture::new();
    let intent = f.prearm();
    let mut b = f.backend();
    b.fail_prepare = true;
    assert!(refresh_with(&f.root, &f.executable, &mut b).is_err());
    assert!(f.root.join(JOURNAL).is_file());
    assert_eq!(
        read_windows_install_state(&f.root).unwrap(),
        Some(f.source.clone())
    );
    b.fail_prepare = false;
    refresh_with(&f.root, &f.executable, &mut b).unwrap();
    assert_eq!(
        read_windows_update_prearm_intent(&f.root)
            .unwrap()
            .unwrap()
            .operation_id,
        intent.operation_id
    );
    assert!(!f.root.join(JOURNAL).exists());
}

#[test]
fn missing_active_ack_is_not_a_successful_hot_handoff() {
    let f = Fixture::new();
    let mut b = f.backend();
    b.fail_ack = true;
    assert!(refresh_with(&f.root, &f.executable, &mut b).is_err());
    assert!(f.root.join(JOURNAL).is_file());
    assert_ne!(
        read_windows_install_state(&f.root)
            .unwrap()
            .unwrap()
            .helper_sha256,
        f.source.helper_sha256
    );
    assert_eq!(
        b.restarts, 0,
        "a committed generation must not be rolled back for a missing ACK"
    );
    b.fail_ack = false;
    assert_eq!(
        refresh_with(&f.root, &f.executable, &mut b).unwrap(),
        Some(4242)
    );
}

#[test]
fn an_uninstall_cancels_a_pending_helper_handoff() {
    let f = Fixture::new();
    let mut b = f.backend();
    b.fail_prepare = true;
    assert!(refresh_with(&f.root, &f.executable, &mut b).is_err());
    transition_windows_install_state(
        &f.root,
        f.source.epoch,
        WindowsInstallPhase::DisableRequested,
    )
    .unwrap();
    let before = fs::read(&f.source.state_path).unwrap();
    let stops = b.stops;
    b.fail_prepare = false;
    assert!(refresh_with(&f.root, &f.executable, &mut b).is_err());
    assert_eq!(fs::read(&f.source.state_path).unwrap(), before);
    assert_eq!(b.stops, stops);
}

#[test]
fn a_running_previous_package_cannot_be_relabelled_as_the_current_one() {
    let f = Fixture::new();
    f.prearm();
    let mut b = f.backend();
    b.package = NEW.into();
    b.source_idle = false;
    assert!(refresh_with(&f.root, &f.executable, &mut b).is_err());
    assert_eq!(
        read_windows_install_state(&f.root).unwrap(),
        Some(f.source.clone())
    );
    assert_eq!(b.stops, 0);
}

#[test]
fn legacy_live_controller_is_not_invalidated_by_native_content_migration() {
    let f = Fixture::new();
    let mut b = f.backend();
    b.source_idle = false;
    assert!(refresh_with(&f.root, &f.executable, &mut b).is_err());
    assert_eq!(
        read_windows_install_state(&f.root).unwrap(),
        Some(f.source.clone())
    );
    assert!(!f.root.join(JOURNAL).exists());
    assert_eq!(b.stops, 0);
}

#[test]
fn compatible_owner_keeps_running_while_recovery_content_changes() {
    let f = Fixture::new();
    let mut b = f.backend();
    b.source_idle = false;
    b.live_owner = true;
    assert_eq!(
        refresh_with(&f.root, &f.executable, &mut b).unwrap(),
        Some(4242)
    );
    assert_eq!(
        read_windows_install_state(&f.root)
            .unwrap()
            .unwrap()
            .registration_id,
        f.source.registration_id
    );
    assert_eq!(b.restarts, 0);
}

#[test]
fn resuming_a_committed_target_keeps_its_observer_even_if_enable_fails() {
    let f = Fixture::new();
    let mut b = f.backend();
    b.fail_ack = true;
    assert!(refresh_with(&f.root, &f.executable, &mut b).is_err());
    let committed = read_windows_install_state(&f.root).unwrap().unwrap();
    b.fail_ack = false;
    b.fail_enable = true;
    assert!(refresh_with(&f.root, &f.executable, &mut b).is_err());
    assert!(b.preserved);
    assert_eq!(
        read_windows_install_state(&f.root).unwrap(),
        Some(committed)
    );
    assert!(f.root.join(JOURNAL).is_file());
    assert_eq!(b.restarts, 0);
}

#[test]
fn resuming_a_committed_target_keeps_its_observer_even_if_login_fails() {
    let f = Fixture::new();
    let mut b = f.backend();
    b.fail_ack = true;
    assert!(refresh_with(&f.root, &f.executable, &mut b).is_err());
    let committed = read_windows_install_state(&f.root).unwrap().unwrap();
    b.fail_ack = false;
    b.fail_login = true;
    assert!(refresh_with(&f.root, &f.executable, &mut b).is_err());
    assert!(b.preserved);
    assert_eq!(
        read_windows_install_state(&f.root).unwrap(),
        Some(committed)
    );
    assert!(f.root.join(JOURNAL).is_file());
    assert_eq!(b.restarts, 0);
}
