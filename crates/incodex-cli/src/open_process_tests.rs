//! [INPUT]: 依赖 open 测试夹具与真实进程owner handoff。
//! [OUTPUT]: 验证 macOS open 生产spawn的责任归属。
//! [POS]: open 生命周期集成回归；不启动官方App或调用权限API。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use super::super::{
    burn_session_home, prepare_incognito_open, spawn_plan_with_owner_and_native_close,
    BurnExpected, OpenProcessResult,
};
use super::{fake_app, temp_root};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

const REPORT_ENV: &str = "INCODEX_TEST_OPEN_RESPONSIBILITY_REPORT";

struct RemoveFixture(PathBuf);

impl Drop for RemoveFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn responsible_pid(pid: libc::pid_t) -> libc::pid_t {
    type Getter = unsafe extern "C" fn(libc::pid_t) -> libc::pid_t;
    let symbol = unsafe {
        libc::dlsym(
            libc::RTLD_DEFAULT,
            b"responsibility_get_pid_responsible_for_pid\0"
                .as_ptr()
                .cast(),
        )
    };
    assert!(
        !symbol.is_null(),
        "responsibility getter is required for this test"
    );
    let getter: Getter = unsafe { std::mem::transmute(symbol) };
    unsafe { getter(pid) }
}

fn wait_for_owner_handoff() {
    let session_root = std::env::var_os("INCODEX_SESSION_ROOT")
        .expect("open plan must pass its isolated session root");
    let owner_path = PathBuf::from(session_root).join("owner.json");
    let expected_pid = std::process::id() as i64;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(bytes) = fs::read(&owner_path) {
            if serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|owner| owner.get("pid").and_then(Value::as_i64))
                == Some(expected_pid)
            {
                return;
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("open production path did not hand session ownership to child");
}

#[test]
fn private_open_production_child_owns_process_responsibility() {
    if let Some(report_path) = std::env::var_os(REPORT_ENV) {
        wait_for_owner_handoff();
        let report = serde_json::json!({
            "pid": std::process::id(),
            "ppid": unsafe { libc::getppid() },
            "responsible_pid": responsible_pid(std::process::id() as libc::pid_t),
            "executable": std::env::current_exe().unwrap(),
        });
        fs::write(report_path, serde_json::to_vec(&report).unwrap()).unwrap();
        return;
    }

    let root = temp_root();
    let _remove_fixture = RemoveFixture(root.clone());
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let parent_pid = std::process::id();
    let parent_responsible_pid = responsible_pid(parent_pid as libc::pid_t);
    let executable = std::env::current_exe().unwrap();
    let report_path = root.join("responsibility.json");
    let mut plan = prepare_incognito_open(&app, &user, &source, parent_pid as i32).unwrap();
    plan.bin = executable.clone();
    plan.args = vec![
        "--exact".into(),
        "open::open_tests::open_process_tests::private_open_production_child_owns_process_responsibility".into(),
        "--nocapture".into(),
    ];
    plan.env
        .push((REPORT_ENV.into(), report_path.display().to_string()));
    plan.debug_port = 0;
    plan.profile_mask = None;

    let outcome = spawn_plan_with_owner_and_native_close(&plan, None).unwrap();
    assert!(matches!(
        outcome.process,
        OpenProcessResult::Exited {
            code: 0,
            ui_ready: false
        }
    ));
    let report: Value = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    burn_session_home(
        &plan.session_root,
        &BurnExpected {
            user_root: &user,
            session_id: Some(&plan.session_id),
            ino: Some(plan.session_ino),
            dev: Some(plan.session_dev),
        },
    )
    .unwrap();

    assert!(report["pid"].as_i64().is_some_and(|pid| pid > 0));
    assert_ne!(report["pid"].as_i64(), Some(i64::from(parent_pid)));
    assert_eq!(report["ppid"].as_i64(), Some(i64::from(parent_pid)));
    assert_eq!(
        report["responsible_pid"].as_i64(),
        report["pid"].as_i64(),
        "production open child inherited responsibility from parent PID {parent_responsible_pid}"
    );
    assert_eq!(
        report["executable"].as_str(),
        Some(executable.to_str().unwrap())
    );
}
