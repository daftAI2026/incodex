//! [INPUT]: 依赖 open 测试夹具与真实进程owner handoff。
//! [OUTPUT]: 验证 macOS open 生产spawn的责任归属。
//! [POS]: open 生命周期集成回归；不启动官方App或调用权限API。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use super::super::process::{spawn_child, ChildProcess as _};
use super::super::{
    burn_session_home, prepare_incognito_open, spawn_plan_with_owner_and_native_close,
    BurnExpected, OpenProcessResult,
};
use super::{fake_app, temp_root};
use serde_json::Value;
use std::fs;
use std::fs::OpenOptions;
use std::os::fd::AsRawFd;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

const REPORT_ENV: &str = "INCODEX_TEST_OPEN_RESPONSIBILITY_REPORT";
const ADAPTER_REPORT_ENV: &str = "INCODEX_TEST_ADAPTER_RESPONSIBILITY_REPORT";
const CONTRACT_REPORT_ENV: &str = "INCODEX_TEST_OPEN_PROCESS_REPORT";
const CONTRACT_OVERRIDE_ENV: &str = "INCODEX_TEST_OPEN_PROCESS_OVERRIDE";
const CONTRACT_FD_ENV: &str = "INCODEX_TEST_OPEN_PROCESS_FD";

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
            c"responsibility_get_pid_responsible_for_pid".as_ptr(),
        )
    };
    assert!(
        !symbol.is_null(),
        "responsibility getter is required for this test"
    );
    let getter: Getter = unsafe { std::mem::transmute(symbol) };
    unsafe { getter(pid) }
}

#[test]
fn private_open_child_reports_process_responsibility() {
    if let Some(path) = std::env::var_os(ADAPTER_REPORT_ENV) {
        let report = serde_json::json!({
            "pid": std::process::id(),
            "ppid": unsafe { libc::getppid() },
            "responsible_pid": responsible_pid(std::process::id() as libc::pid_t),
            "executable": std::env::current_exe().unwrap(),
        });
        fs::write(path, serde_json::to_vec(&report).unwrap()).unwrap();
        return;
    }

    let root = temp_root();
    let _remove_fixture = RemoveFixture(root.clone());
    let report_path = root.join("responsibility.json");
    let parent_pid = std::process::id();
    let parent_responsible_pid = responsible_pid(parent_pid as libc::pid_t);
    let executable = std::env::current_exe().unwrap();
    let mut child = spawn_child(
        &executable,
        &[
            "--exact",
            "open::open_tests::open_process_tests::private_open_child_reports_process_responsibility",
            "--nocapture",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>(),
        &[(
            ADAPTER_REPORT_ENV.to_string(),
            report_path.display().to_string(),
        )],
    )
    .unwrap();
    let child_pid = child.id();
    let status = child.wait().unwrap();
    assert!(status.success(), "isolated child test must exit normally");

    let report: Value = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    assert_eq!(report["pid"].as_u64(), Some(u64::from(child_pid)));
    assert_eq!(report["ppid"].as_i64(), Some(i64::from(parent_pid)));
    assert_eq!(
        report["responsible_pid"].as_i64(),
        report["pid"].as_i64(),
        "private-open child must own responsibility instead of inheriting its CLI parent; parent responsibility is {parent_responsible_pid}"
    );
    assert_eq!(
        report["executable"].as_str(),
        Some(executable.to_str().unwrap())
    );
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

fn stdio_is_null_device() -> bool {
    let mut null_stat = unsafe { std::mem::zeroed::<libc::stat>() };
    if unsafe { libc::stat(c"/dev/null".as_ptr(), &mut null_stat) } != 0 {
        return false;
    }
    (libc::STDIN_FILENO..=libc::STDERR_FILENO).all(|fd| {
        let mut descriptor_stat = unsafe { std::mem::zeroed::<libc::stat>() };
        (unsafe { libc::fstat(fd, &mut descriptor_stat) }) == 0
            && descriptor_stat.st_rdev == null_stat.st_rdev
            && descriptor_stat.st_mode & libc::S_IFMT == libc::S_IFCHR
    })
}

fn inherited_file_matches_fd_spec() -> bool {
    let Some(spec) = std::env::var_os(CONTRACT_FD_ENV) else {
        return false;
    };
    file_descriptor_matches_spec(&spec.to_string_lossy())
}

fn file_descriptor_matches_spec(spec: &str) -> bool {
    let fields: Vec<_> = spec.split(':').collect();
    let [fd, device, inode] = fields.as_slice() else {
        return false;
    };
    let (Ok(fd), Ok(device), Ok(inode)) = (
        (*fd).parse::<libc::c_int>(),
        (*device).parse::<u64>(),
        (*inode).parse::<u64>(),
    ) else {
        return false;
    };
    let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
    (unsafe { libc::fstat(fd, &mut stat) }) == 0
        && stat.st_dev as u64 == device
        && stat.st_ino as u64 == inode
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

#[test]
fn spawn_fixture_reports_environment_stdio_and_inherited_fd_contract() {
    if let Some(report_path) = std::env::var_os(CONTRACT_REPORT_ENV) {
        let report = serde_json::json!({
            "pid": std::process::id(),
            "ppid": unsafe { libc::getppid() },
            "path": std::env::var("PATH").unwrap(),
            "override": std::env::var(CONTRACT_OVERRIDE_ENV).unwrap(),
            "stdio_null": stdio_is_null_device(),
            "inherited_fd_matches": inherited_file_matches_fd_spec(),
        });
        fs::write(report_path, serde_json::to_vec(&report).unwrap()).unwrap();
        return;
    }

    let root = temp_root();
    let _remove_fixture = RemoveFixture(root.clone());
    let report_path = root.join("process-contract.json");
    let inherited_path = root.join("inherited-descriptor");
    fs::write(&inherited_path, "do not inherit\n").unwrap();
    let inherited_file = OpenOptions::new().read(true).open(&inherited_path).unwrap();
    let fd = inherited_file.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    assert!(flags >= 0);
    assert_eq!(
        unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) },
        0,
        "fixture must expose a non-CLOEXEC file descriptor"
    );
    let mut inherited_stat = unsafe { std::mem::zeroed::<libc::stat>() };
    assert_eq!(unsafe { libc::fstat(fd, &mut inherited_stat) }, 0);
    let fd_spec = format!(
        "{}:{}:{}",
        fd, inherited_stat.st_dev as u64, inherited_stat.st_ino as u64
    );
    assert!(
        file_descriptor_matches_spec(&fd_spec),
        "same-process control must identify the open fixture descriptor"
    );
    let executable = std::env::current_exe().unwrap();
    let args = [
        "--exact",
        "open::open_tests::open_process_tests::spawn_fixture_reports_environment_stdio_and_inherited_fd_contract",
        "--nocapture",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    let env = [
        (
            CONTRACT_REPORT_ENV.to_string(),
            report_path.display().to_string(),
        ),
        (
            CONTRACT_OVERRIDE_ENV.to_string(),
            "from-open-plan".to_string(),
        ),
        (CONTRACT_FD_ENV.to_string(), fd_spec),
    ];
    let mut child = spawn_child(&executable, &args, &env).unwrap();
    let child_pid = child.id();
    let status = child.wait().unwrap();
    assert!(
        status.success(),
        "process contract fixture must exit normally"
    );
    let report: Value = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();

    assert_eq!(report["pid"].as_u64(), Some(u64::from(child_pid)));
    assert_eq!(report["ppid"].as_i64(), Some(i64::from(std::process::id())));
    assert_eq!(
        report["path"].as_str(),
        std::env::var("PATH").ok().as_deref()
    );
    assert_eq!(report["override"].as_str(), Some("from-open-plan"));
    assert_eq!(report["stdio_null"].as_bool(), Some(true));
    assert_eq!(report["inherited_fd_matches"].as_bool(), Some(false));
}

#[test]
fn spawn_preserves_arguments_with_spaces() {
    let root = temp_root();
    let _remove_fixture = RemoveFixture(root.clone());
    let report_path = root.join("arguments.txt");
    let script = r#"printf '%s\n%s\n%s\n' "$0" "$1" "$2" > "$INCODEX_TEST_ARG_REPORT""#;
    let args = vec![
        "-c".into(),
        script.into(),
        "argv zero with spaces".into(),
        "first argument with spaces".into(),
        "second argument with spaces".into(),
    ];
    let env = [(
        "INCODEX_TEST_ARG_REPORT".into(),
        report_path.display().to_string(),
    )];
    let mut child = spawn_child(&PathBuf::from("/bin/sh"), &args, &env).unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(0));
    assert_eq!(
        fs::read_to_string(report_path).unwrap(),
        "argv zero with spaces\nfirst argument with spaces\nsecond argument with spaces\n"
    );
}

#[test]
fn spawn_rejects_invalid_inputs_and_does_not_fabricate_a_child() {
    assert!(spawn_child(&PathBuf::from("/no/such/incodex-executable"), &[], &[]).is_err());
    assert!(spawn_child(&PathBuf::from("/bin/\0sh"), &[], &[]).is_err());
    assert!(spawn_child(&PathBuf::from("/bin/sh"), &["bad\0arg".into()], &[]).is_err());
    assert!(spawn_child(
        &PathBuf::from("/bin/sh"),
        &[],
        &[("INCODEX_TEST_NUL".into(), "bad\0value".into())]
    )
    .is_err());
    assert!(spawn_child(
        &PathBuf::from("/bin/sh"),
        &[],
        &[("INVALID=KEY".into(), "value".into())]
    )
    .is_err());
}

#[test]
fn spawn_accepts_relative_executable_path_with_slash() {
    let mut relative_shell = PathBuf::new();
    for component in std::env::current_dir().unwrap().components() {
        if matches!(component, std::path::Component::Normal(_)) {
            relative_shell.push("..");
        }
    }
    relative_shell.push("bin/sh");

    let mut child = spawn_child(&relative_shell, &["-c".into(), "exit 7".into()], &[]).unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(7));
}

#[test]
fn waitpid_preserves_exit_codes_and_caches_reaped_status() {
    for expected_code in [0, 7] {
        let args = vec!["-c".into(), format!("exit {expected_code}")];
        let mut child = spawn_child(&PathBuf::from("/bin/sh"), &args, &[]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "real child did not exit in time");
            thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(status.code(), Some(expected_code));
        assert_eq!(child.wait().unwrap(), status);
        assert_eq!(child.try_wait().unwrap(), Some(status));
        assert!(child.kill().is_err(), "a reaped PID must not be signaled");
    }
}

#[test]
fn waitpid_preserves_real_term_and_kill_signals() {
    for signal in [libc::SIGTERM, libc::SIGKILL] {
        let mut child = spawn_child(&PathBuf::from("/bin/sleep"), &["30".into()], &[]).unwrap();
        child
            .signal(signal)
            .unwrap_or_else(|error| panic!("signal {signal}: {error}"));
        let status = child.wait().unwrap();
        assert_eq!(status.signal(), Some(signal));
    }
}

#[test]
fn spawn_restores_sigpipe_default_for_official_child() {
    let mut child = spawn_child(
        &PathBuf::from("/bin/sh"),
        &["-c".into(), "kill -PIPE $$".into()],
        &[],
    )
    .unwrap();
    let status = child.wait().unwrap();
    assert_eq!(status.signal(), Some(libc::SIGPIPE));
}
