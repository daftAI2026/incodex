#![cfg(target_os = "macos")]
#![recursion_limit = "256"]

/**
 * [INPUT]: 编译当前 Coordinator，并使用隔离 HOME、假宿主与登记 helper。
 * [OUTPUT]: 验证排队更新在宿主退出后续接 helper，且不重开宿主。
 * [POS]: macOS 更新恢复的原生进程回归；只启动本测试创建的进程。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
const PASSIVE_WINDOW: Duration = Duration::from_secs(10);
const CHILD_TIMEOUT: Duration = Duration::from_secs(10);

const MARKER_HELPER_SOURCE: &str = include_str!("native/macos-update-continuation-helper.c");
const FAKE_HOST_SOURCE: &str = include_str!("native/macos-update-continuation-host.c");

#[path = "native/macos_update_continuation_support.rs"]
mod support;
use support::*;

#[test]
fn queued_update_continues_after_normal_host_exit_without_reopening_host() {
    let run = RunRoot::create();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let native_dir = repository.join("crates/incodex-cli/native");
    let coordinator_source = native_dir.join("macos_update_coordinator.m");
    let source_bytes = fs::read(&coordinator_source).expect("read current native Coordinator");
    let source_sha256 = sha256(&source_bytes);
    let clang = xcrun(&["--find", "clang"]);
    let sdk = xcrun(&["--sdk", "macosx", "--show-sdk-path"]);

    let coordinator_build = run.path.join("coordinator-build");
    private_dir(&coordinator_build);
    let coordinator_unbundled = coordinator_build.join("incodex-update-coordinator");
    let compile = Command::new(&clang)
        .args(["-isysroot", sdk.to_str().expect("SDK path is UTF-8"), "-I"])
        .arg(&native_dir)
        .args(["-fobjc-arc"])
        .arg(&coordinator_source)
        .args(["-framework", "AppKit", "-framework", "Foundation", "-o"])
        .arg(&coordinator_unbundled)
        .output()
        .expect("start clang for current Coordinator");
    write_private(
        &run.path.join("coordinator-compile.stdout"),
        &compile.stdout,
    );
    write_private(
        &run.path.join("coordinator-compile.stderr"),
        &compile.stderr,
    );
    assert!(
        compile.status.success(),
        "current Coordinator must compile before lifecycle assertions; status={:?}; stderr={}",
        compile.status.code(),
        String::from_utf8_lossy(&compile.stderr)
    );

    let helper_source = coordinator_build.join("marker-helper.c");
    let host_source = coordinator_build.join("fake-host.c");
    fs::write(&helper_source, MARKER_HELPER_SOURCE).expect("write marker helper source");
    fs::write(&host_source, FAKE_HOST_SOURCE).expect("write fake Host source");
    let helper_unbundled = coordinator_build.join("incodex-marker-helper");
    let host_unbundled = coordinator_build.join("fake-host");
    let case_root = canonical(&run.path).join("case");
    let helper_defines = [
        (
            "INCODEX_TEST_HELPER_LOG",
            case_root.join("events/helper-launches.jsonl"),
        ),
        (
            "INCODEX_TEST_HELPER_MARKER",
            case_root.join("events/helper-marker.json"),
        ),
        (
            "INCODEX_TEST_HELPER_GATE",
            case_root.join("gates/helper-release"),
        ),
        (
            "INCODEX_TEST_HELPER_END",
            case_root.join("events/helper-end.json"),
        ),
    ]
    .into_iter()
    .map(|(name, path)| format!("-D{name}={}", c_string_literal(&path.to_string_lossy())))
    .collect::<Vec<_>>();
    compile_c(
        &clang,
        &sdk,
        &helper_source,
        &helper_unbundled,
        "helper",
        &helper_defines,
    );
    let relaunch_marker = case_root.join("events/host-relaunch.json");
    let marker_define = format!(
        "-DINCODEX_TEST_RELAUNCH_MARKER={}",
        c_string_literal(&relaunch_marker.to_string_lossy())
    );
    compile_c(
        &clang,
        &sdk,
        &host_source,
        &host_unbundled,
        "host",
        &[marker_define],
    );

    let coordinator_bytes = fs::read(&coordinator_unbundled).expect("read compiled Coordinator");
    let coordinator_hash = sha256(&coordinator_bytes);
    let helper_bytes = fs::read(&helper_unbundled).expect("read compiled marker helper");
    let helper_hash = sha256(&helper_bytes);
    let fixture = prepare_fixture(
        &run.path,
        &coordinator_unbundled,
        &coordinator_hash,
        &helper_unbundled,
        &helper_hash,
        &host_unbundled,
    );
    let coordinator_info = fixture
        .coordinator_executable
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Info.plist");
    let coordinator_info_text =
        fs::read_to_string(&coordinator_info).expect("read synthetic Coordinator plist");
    let console_launch_environment_isolated = coordinator_info_text.contains(&format!(
        "<key>HOME</key><string>{}</string>",
        fixture.home.display()
    )) && coordinator_info_text.contains(&format!(
        "<key>TMPDIR</key><string>{}</string>",
        fixture.tmp.display()
    )) && !coordinator_info_text
        .contains("INCODEX_MACOS_UPDATE_COORDINATOR");
    assert!(
        console_launch_environment_isolated,
        "console Coordinator launch must retain isolated HOME/TMPDIR without the initial flag"
    );
    let host_preflight = verify_host_marker_positive_control(&fixture);
    write_private(
        &run.path.join("host-preflight.json"),
        serde_json::to_vec_pretty(&host_preflight)
            .expect("serialize Host marker preflight")
            .as_slice(),
    );
    assert!(
        host_preflight["markerWritten"] == true
            && host_preflight["pidMatches"] == true
            && host_preflight["executableMatches"] == true
            && host_preflight["normalExit"] == true
            && host_preflight["markerRemoved"] == true,
        "test Host marker positive control must pass before lifecycle scenario; evidence={}",
        run.path.join("host-preflight.json").display()
    );

    let mut host_command = Command::new("/bin/sleep");
    host_command
        .arg("120")
        .env_clear()
        .env("HOME", &fixture.home)
        .env("TMPDIR", &fixture.tmp)
        .env("PATH", "/usr/bin:/bin")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut host = OwnedChild::spawn(&mut host_command, "synthetic sleep Host");

    let mut coordinator_command = Command::new(&fixture.coordinator_executable);
    coordinator_command
        .env_clear()
        .env("HOME", &fixture.home)
        .env("TMPDIR", &fixture.tmp)
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "en_US.UTF-8")
        .env("INCODEX_MACOS_UPDATE_COORDINATOR", "1")
        .env("INCODEX_MACOS_UPDATE_HOST_PID", host.id().to_string())
        .env("INCODEX_MACOS_UPDATE_INSTALL_ID", &fixture.install_id)
        .env("INCODEX_MACOS_UPDATE_HOST_APP", &fixture.host_app)
        .env("INCODEX_MACOS_UPDATE_HELPER_PATH", &fixture.helper)
        .env("INCODEX_MACOS_UPDATE_HELPER_SHA256", &helper_hash)
        .env("INCODEX_MACOS_UPDATE_HANDOFF_ID", &fixture.handoff_id)
        .env("INCODEX_MACOS_UPDATE_READY_PATH", &fixture.ready)
        .env("INCODEX_MACOS_UPDATE_PENDING_PATH", &fixture.pending)
        .stdout(Stdio::from(create_private_file(
            &run.path.join("initial-coordinator.stdout"),
        )))
        .stderr(Stdio::from(create_private_file(
            &run.path.join("initial-coordinator.stderr"),
        )));
    let mut old_coordinator = OwnedChild::spawn(&mut coordinator_command, "initial Coordinator");
    let old_coordinator_pid = old_coordinator.id();

    wait_for_json_pid(&fixture.ready, &mut old_coordinator, CHILD_TIMEOUT)
        .expect("initial Coordinator must publish its ready PID");
    let ready: Value = read_json(&fixture.ready).expect("parse ready receipt");
    assert_eq!(ready["pid"].as_u64(), Some(u64::from(old_coordinator_pid)));
    let pending_identity = set_queued_update_fields(&fixture);

    host.request_normal_exit()
        .expect("SIGTERM only the owned sleep Host PID");
    let host_status = host
        .wait_timeout(Duration::from_secs(5))
        .expect("owned sleep Host must end normally");
    assert_eq!(host_status.signal(), Some(libc::SIGTERM));

    let old_coordinator_status = old_coordinator
        .wait_timeout(CHILD_TIMEOUT)
        .expect("old Coordinator must finish after Host exit");
    assert!(
        old_coordinator_status.success(),
        "old Coordinator exit: {old_coordinator_status}"
    );

    let pending_after_old_exit = pending_snapshot_under_lock(&fixture);
    let queued_fields_retained = pending_after_old_exit
        .as_ref()
        .map(|pending| queued_fields_match(pending, &pending_identity))
        .unwrap_or(false);
    let old_bundle_metadata = fs::metadata(&fixture.host_app).expect("stat old fake Host bundle");
    replace_host_bundle(&fixture, "10002");
    let new_bundle_metadata =
        fs::metadata(&fixture.host_app).expect("stat replacement fake Host bundle");
    let replacement_build = new_bundle_metadata.dev() == old_bundle_metadata.dev()
        && new_bundle_metadata.ino() != old_bundle_metadata.ino()
        && fs::read_to_string(fixture.host_app.join("Contents/Info.plist"))
            .expect("read replacement fake Host plist")
            .contains("<key>CFBundleVersion</key><string>10002</string>")
        && fs::read_to_string(
            fixture
                .host_app
                .parent()
                .unwrap()
                .join("FakeHost.app.retired/Contents/Info.plist"),
        )
        .expect("read retired fake Host plist")
        .contains("<key>CFBundleVersion</key><string>10001</string>");
    let passive_started = Instant::now();
    wait_for_file(&fixture.helper_marker, PASSIVE_WINDOW);
    let passive_observed_ms = passive_started.elapsed().as_millis();

    let launches = read_lines(&fixture.helper_log);
    let marker = wait_for_json(&fixture.helper_marker, Duration::from_secs(1));
    let helper_pid = marker
        .as_ref()
        .and_then(|value| value["pid"].as_u64())
        .map(|pid| pid as u32);
    let helper_ppid = marker
        .as_ref()
        .and_then(|value| value["ppid"].as_u64())
        .map(|pid| pid as u32);
    let helper_parent_executable = helper_ppid.and_then(process_path);
    let helper_parent_is_new_coordinator = helper_ppid
        .map(|pid| pid != old_coordinator_pid)
        .unwrap_or(false)
        && helper_parent_executable
            .as_deref()
            .map(|path| canonical(path) == canonical(&fixture.coordinator_executable))
            .unwrap_or(false);
    let pending_snapshot = if marker.is_some() {
        pending_snapshot_under_lock(&fixture)
    } else {
        None
    };
    let pending_cas_matches = pending_snapshot
        .as_ref()
        .zip(helper_ppid)
        .map(|(pending, parent_pid)| {
            pending["coordinatorPid"].as_u64() == Some(u64::from(parent_pid))
                && pending["installQueued"] == true
                && pending["sourceBuild"] == "10001"
                && pending["relaunchHost"] == false
                && queued_fields_match(pending, &pending_identity)
        })
        .unwrap_or(false);

    let pending_present_before_helper_release = fixture.pending.exists();
    let helper_gate_released = helper_parent_is_new_coordinator
        && pending_cas_matches
        && release_helper(&fixture.helper_gate, CHILD_TIMEOUT).is_ok();
    if let Some(pid) = helper_pid {
        if !helper_gate_released {
            terminate_exact_process(pid, &fixture.helper);
        }
    }
    let helper_exited = helper_pid
        .map(|pid| wait_for_process_exit(pid, &fixture.helper, Duration::from_secs(5)))
        .unwrap_or(false);
    if let Some(pid) = helper_pid.filter(|_| !helper_exited) {
        terminate_exact_process(pid, &fixture.helper);
    }
    let continuation_coordinator_exited = helper_ppid
        .filter(|pid| *pid != old_coordinator_pid && helper_parent_is_new_coordinator)
        .map(|pid| {
            let exited =
                wait_for_process_exit(pid, &fixture.coordinator_executable, Duration::from_secs(5));
            if !exited {
                terminate_exact_process(pid, &fixture.coordinator_executable);
            }
            exited
        })
        .unwrap_or(false);

    let pending_cleaned_after_recovery =
        helper_gate_released && pending_present_before_helper_release && !fixture.pending.exists();
    let helper_end = read_json(&fixture.helper_end).ok();
    let helper_end_matches_marker = helper_end
        .as_ref()
        .zip(marker.as_ref())
        .map(|(end, begin)| {
            end["pid"] == begin["pid"] && end["ppid"] == begin["ppid"] && end["exitCode"] == 0
        })
        .unwrap_or(false);
    let host_passive_started = Instant::now();
    wait_for_file(&fixture.host_relaunch_marker, PASSIVE_WINDOW);
    let host_passive_observed_ms = host_passive_started.elapsed().as_millis();
    let host_relaunch = wait_for_json(&fixture.host_relaunch_marker, Duration::from_secs(1));
    let host_relaunch_pid = host_relaunch
        .as_ref()
        .and_then(|value| value["pid"].as_u64())
        .map(|pid| pid as u32);
    let host_relaunch_executable_matches = host_relaunch_pid
        .and_then(process_path)
        .as_deref()
        .map(|path| canonical(path) == canonical(&fixture.host_executable))
        .unwrap_or(false);
    if let Some(pid) = host_relaunch_pid.filter(|_| host_relaunch_executable_matches) {
        terminate_exact_process(pid, &fixture.host_executable);
    }
    let host_relaunch_marker_exists = fixture.host_relaunch_marker.exists();
    if host_relaunch_marker_exists {
        let _ = fs::remove_file(&fixture.host_relaunch_marker);
    }
    let result = json!({
        "nativeSourcePath": coordinator_source,
        "nativeSourceSha256": source_sha256,
        "nativeCoordinatorSha256": coordinator_hash,
        "helperSha256": helper_hash,
        "oldCoordinatorPid": old_coordinator_pid,
        "oldCoordinatorExitCode": old_coordinator_status.code(),
        "oldCoordinatorExited": true,
        "ownedSleepHostPid": host.id(),
        "ownedSleepHostSignal": host_status.signal(),
        "queuedFieldsWrittenAfterReady": true,
        "pendingAfterOldCoordinatorExit": pending_after_old_exit,
        "queuedFieldsRetainedAfterOldCoordinatorExit": queued_fields_retained,
        "hostBundleReplacedByDirectoryRename": replacement_build,
        "oldHostBundleInode": old_bundle_metadata.ino(),
        "newHostBundleInode": new_bundle_metadata.ino(),
        "helperPassiveObservationMs": passive_observed_ms,
        "hostRelaunchPassiveObservationMs": host_passive_observed_ms,
        "secondStageStartedByTest": false,
        "helperLaunchCount": launches.len(),
        "helperLaunches": launches,
        "hostPreflight": host_preflight,
        "consoleLaunchEnvironmentIsolatedAndWithoutInitialFlag": console_launch_environment_isolated,
        "helperMarker": marker,
        "helperEnd": helper_end,
        "helperEndMatchesMarker": helper_end_matches_marker,
        "helperGateReleasedAfterIdentityCheck": helper_gate_released,
        "helperExited": helper_exited,
        "continuationCoordinatorExited": continuation_coordinator_exited,
        "helperParentPid": helper_ppid,
        "helperParentExecutable": helper_parent_executable.as_ref().map(|path| path.display().to_string()),
        "helperParentIsDifferentCoordinatorSameExecutable": helper_parent_is_new_coordinator,
        "pendingSnapshotUnderLock": pending_snapshot,
        "pendingCasMatchesNewCoordinatorAndOriginalHandoff": pending_cas_matches,
        "pendingCleanedAfterRecovery": pending_cleaned_after_recovery,
        "fakeHostRelaunchMarkerExists": host_relaunch_marker_exists,
        "fakeHostRelaunchExecutableMatchedForCleanup": host_relaunch_executable_matches,
        "expected": {
            "hostBundleReplacedByDirectoryRename": true,
            "consoleLaunchEnvironmentIsolatedAndWithoutInitialFlag": true,
            "helperLaunchCount": 1,
            "helperParentIsDifferentCoordinatorSameExecutable": true,
            "pendingCasMatchesNewCoordinatorAndOriginalHandoff": true,
            "helperEndMatchesMarker": true,
            "helperExited": true,
            "continuationCoordinatorExited": true,
            "pendingCleanedAfterRecovery": true,
            "fakeHostRelaunchMarkerExists": false,
        },
        "claim": "the current native Coordinator was compiled and executed against an isolated owned Host; the test did not launch a second-stage Coordinator"
    });
    write_private(
        &run.path.join("result.json"),
        serde_json::to_vec_pretty(&result)
            .expect("serialize continuation receipt")
            .as_slice(),
    );
    println!("NATIVE_CONTINUATION_EVIDENCE={}", run.path.display());
    println!("NATIVE_CONTINUATION_RESULT={result}");

    assert!(
        replacement_build,
        "the fixture must replace the entire fake Host bundle by directory rename; receipt={}",
        run.path.join("result.json").display()
    );
    assert!(
        queued_fields_retained,
        "queued continuation fields must survive the first Coordinator exit; receipt={}",
        run.path.join("result.json").display()
    );
    assert_eq!(
        launches.len(),
        1,
        "native Coordinator must automatically continue the queued update after the Host exits; observed {} helper launches in a {PASSIVE_WINDOW:?} passive window; receipt={}",
        launches.len(),
        run.path.join("result.json").display()
    );
    assert!(
        helper_parent_is_new_coordinator,
        "helper PPID must be a different process running this same Coordinator executable; receipt={}",
        run.path.join("result.json").display()
    );
    assert!(
        pending_cas_matches,
        "before FIFO release, locked pending must retain installId/appPath/handoffId and belong to helper PPID; receipt={}",
        run.path.join("result.json").display()
    );
    assert!(
        helper_gate_released,
        "release helper only after Coordinator and locked pending identity checks; receipt={}",
        run.path.join("result.json").display()
    );
    assert!(
        helper_end_matches_marker,
        "helper end PID/PPID and exitCode must match its launch marker; receipt={}",
        run.path.join("result.json").display()
    );
    assert!(
        helper_exited,
        "registered helper process must actually exit; receipt={}",
        run.path.join("result.json").display()
    );
    assert!(
        continuation_coordinator_exited,
        "continuation Coordinator process must actually exit; receipt={}",
        run.path.join("result.json").display()
    );
    assert!(
        pending_cleaned_after_recovery,
        "successful continuation must remove its owned pending handoff; receipt={}",
        run.path.join("result.json").display()
    );
    assert!(
        !host_relaunch_marker_exists,
        "relaunchHost=false must not reopen the user's fake Host; receipt={}",
        run.path.join("result.json").display()
    );
}

struct Paths {
    home: PathBuf,
    tmp: PathBuf,
    coordinator_executable: PathBuf,
    host_app: PathBuf,
    host_executable: PathBuf,
    ready: PathBuf,
    pending: PathBuf,
    helper: PathBuf,
    helper_marker: PathBuf,
    helper_end: PathBuf,
    helper_log: PathBuf,
    helper_gate: PathBuf,
    host_relaunch_marker: PathBuf,
    install_id: String,
    handoff_id: String,
    host_bundle_id: String,
}

fn prepare_fixture(
    run: &Path,
    coordinator_binary: &Path,
    coordinator_sha256: &str,
    helper_binary: &Path,
    helper_sha256: &str,
    host_binary: &Path,
) -> Paths {
    let root = canonical(run).join("case");
    let home = root.join("home");
    let tmp = root.join("tmp");
    let incodex = home.join(".incodex");
    let coordinator_app = incodex
        .join("helpers/macos-update")
        .join(coordinator_sha256)
        .join("Incodex Update Coordinator.app");
    let coordinator_executable = coordinator_app.join("Contents/MacOS/incodex-update-coordinator");
    let host_app = root.join("Applications/FakeHost.app");
    let host_executable = host_app.join("Contents/MacOS/fake-host");
    let ready = root.join("events/coordinator-ready.json");
    let pending = incodex.join("macos-update/pending.json");
    let helper_dir = incodex.join("helpers/macos-update").join(helper_sha256);
    let helper = helper_dir.join("incodex");
    let helper_marker = root.join("events/helper-marker.json");
    let helper_end = root.join("events/helper-end.json");
    let helper_log = root.join("events/helper-launches.jsonl");
    let helper_gate = root.join("gates/helper-release");
    let host_relaunch_marker = root.join("events/host-relaunch.json");

    for directory in [
        root.clone(),
        home.clone(),
        tmp.clone(),
        incodex.clone(),
        incodex.join("helpers"),
        incodex.join("helpers/macos-update"),
        coordinator_app.join("Contents/MacOS"),
        incodex.join("macos-update"),
        host_app.join("Contents/MacOS"),
        root.join("events"),
        root.join("gates"),
        helper_dir,
    ] {
        private_dir(&directory);
    }
    copy_private_executable(coordinator_binary, &coordinator_executable);
    copy_private_executable(helper_binary, &helper);
    copy_private_executable(host_binary, &host_executable);
    make_fifo(&helper_gate);

    let install_id = format!(
        "test-install-{}-{}",
        std::process::id(),
        SEQUENCE.load(Ordering::Relaxed)
    );
    let handoff_id = format!(
        "test-handoff-{}-{}",
        std::process::id(),
        SEQUENCE.load(Ordering::Relaxed)
    );
    write_private(
        &coordinator_app.join("Contents/Info.plist"),
        coordinator_plist(coordinator_sha256, &home, &tmp).as_bytes(),
    );
    let host_bundle_id = format!(
        "com.daftai.incodex.test.host.{}.{}",
        std::process::id(),
        SEQUENCE.load(Ordering::Relaxed)
    );
    write_host_plist(&host_app, &host_bundle_id, "10001");
    let registration = json!({
        "schemaVersion": 2,
        "installId": install_id.clone(),
        "appPath": host_app.display().to_string(),
        "helperPath": helper.display().to_string(),
        "helperSha256": helper_sha256,
    });
    write_private(
        &incodex.join("macos-update/registration.json"),
        serde_json::to_vec(&registration)
            .expect("serialize synthetic registration")
            .as_slice(),
    );
    write_private(&incodex.join("macos-update/.pending.lock"), b"");
    set_mode(&incodex.join("macos-update/.pending.lock"), 0o600);

    Paths {
        home,
        tmp,
        coordinator_executable,
        host_app,
        host_executable,
        ready,
        pending,
        helper,
        helper_marker,
        helper_end,
        helper_log,
        helper_gate,
        host_relaunch_marker,
        install_id,
        handoff_id,
        host_bundle_id,
    }
}

#[derive(Clone)]
struct PendingIdentity {
    install_id: String,
    app_path: String,
    handoff_id: String,
}

fn set_queued_update_fields(paths: &Paths) -> PendingIdentity {
    let lock = lock_pending(paths);
    let mut pending: Value =
        read_json(&paths.pending).expect("Coordinator persisted pending before ready");
    let ready_pid = read_json(&paths.ready).unwrap()["pid"].as_u64().unwrap();
    assert_eq!(pending["coordinatorPid"].as_u64(), Some(ready_pid));
    let identity = PendingIdentity {
        install_id: pending["installId"].as_str().unwrap().to_owned(),
        app_path: pending["appPath"].as_str().unwrap().to_owned(),
        handoff_id: pending["handoffId"].as_str().unwrap().to_owned(),
    };
    pending["installQueued"] = json!(true);
    pending["sourceBuild"] = json!("10001");
    pending["relaunchHost"] = json!(false);
    let temporary = paths.pending.with_extension("pending-test-write");
    write_private(&temporary, serde_json::to_vec(&pending).unwrap().as_slice());
    fs::rename(&temporary, &paths.pending)
        .expect("atomically replace only the test-owned pending file");
    unlock_pending(lock);
    identity
}

fn lock_pending(paths: &Paths) -> std::fs::File {
    let lock_path = paths.pending.parent().unwrap().join(".pending.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("open owned pending lock");
    assert_eq!(mode(&lock_path), 0o600);
    let locked = unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX) } == 0;
    assert!(locked, "acquire the test-owned pending lock");
    lock
}

fn unlock_pending(lock: std::fs::File) {
    let unlocked =
        unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_UN) } == 0;
    assert!(unlocked, "release the test-owned pending lock");
}

fn pending_snapshot_under_lock(paths: &Paths) -> Option<Value> {
    let lock = lock_pending(paths);
    let snapshot = read_json(&paths.pending).ok();
    unlock_pending(lock);
    snapshot
}

fn queued_fields_match(pending: &Value, identity: &PendingIdentity) -> bool {
    pending["installId"] == identity.install_id
        && pending["appPath"] == identity.app_path
        && pending["handoffId"] == identity.handoff_id
        && pending["installQueued"] == true
        && pending["sourceBuild"] == "10001"
        && pending["relaunchHost"] == false
}

fn verify_host_marker_positive_control(paths: &Paths) -> Value {
    let mut command = Command::new(&paths.host_executable);
    command
        .env_clear()
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut host = OwnedChild::spawn(&mut command, "fake Host marker positive control");
    let marker = wait_for_json(&paths.host_relaunch_marker, Duration::from_secs(5));
    let marker_pid = marker
        .as_ref()
        .and_then(|value| value["pid"].as_u64())
        .map(|pid| pid as u32);
    let pid_matches = marker_pid == Some(host.id());
    let observed_executable = marker_pid.and_then(process_path);
    let executable_matches = observed_executable
        .as_deref()
        .map(|path| canonical(path) == canonical(&paths.host_executable))
        .unwrap_or(false);
    let normal_exit = host
        .request_normal_exit()
        .and_then(|()| host.wait_timeout(Duration::from_secs(5)))
        .map(|status| status.signal() == Some(libc::SIGTERM))
        .unwrap_or(false);
    let marker_removed = fs::remove_file(&paths.host_relaunch_marker).is_ok();
    json!({
        "purpose": "test-only positive control; not a product Host-reopen result",
        "markerWritten": marker.is_some(),
        "marker": marker,
        "pidMatches": pid_matches,
        "observedExecutable": observed_executable.as_ref().map(|path| path.display().to_string()),
        "executableMatches": executable_matches,
        "normalExit": normal_exit,
        "markerRemoved": marker_removed,
    })
}

fn replace_host_bundle(paths: &Paths, build: &str) {
    let parent = paths.host_app.parent().unwrap();
    let staging = parent.join("FakeHost.app.next");
    let retired = parent.join("FakeHost.app.retired");
    private_dir(&staging.join("Contents/MacOS"));
    copy_private_executable(
        &paths.host_executable,
        &staging.join("Contents/MacOS/fake-host"),
    );
    write_host_plist(&staging, &paths.host_bundle_id, build);
    fs::rename(&paths.host_app, &retired).expect("retire only the owned old fake Host bundle");
    fs::rename(&staging, &paths.host_app).expect("publish the owned replacement fake Host bundle");
}

fn write_host_plist(host_app: &Path, bundle_id: &str, build: &str) {
    let info = host_app.join("Contents/Info.plist");
    let temporary = info.with_extension("plist-next");
    let bundle_id = xml_escape(bundle_id);
    let contents = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>{bundle_id}</string><key>CFBundleExecutable</key><string>fake-host</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>{build}</string><key>LSBackgroundOnly</key><true/><key>NSPrincipalClass</key><string>NSApplication</string></dict></plist>\n"
    );
    write_private(&temporary, contents.as_bytes());
    fs::rename(&temporary, &info).expect("atomic same-directory fake Host plist replacement");
}

fn coordinator_plist(hash: &str, home: &Path, tmp: &Path) -> String {
    let home = xml_escape(&home.to_string_lossy());
    let tmp = xml_escape(&tmp.to_string_lossy());
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>com.daftai.incodex.test.coordinator.{hash}</string><key>CFBundleExecutable</key><string>incodex-update-coordinator</string><key>CFBundleName</key><string>Incodex Update Coordinator Test</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>1</string><key>LSBackgroundOnly</key><true/><key>NSPrincipalClass</key><string>NSApplication</string><key>LSEnvironment</key><dict><key>HOME</key><string>{home}</string><key>TMPDIR</key><string>{tmp}</string><key>PATH</key><string>/usr/bin:/bin</string></dict></dict></plist>\n"
    )
}

fn compile_c(
    clang: &Path,
    sdk: &Path,
    source: &Path,
    output: &Path,
    name: &str,
    defines: &[String],
) {
    let result = Command::new(clang)
        .arg("-isysroot")
        .arg(sdk)
        .args(["-Wall", "-Wextra", "-O0"])
        .args(defines)
        .arg(source)
        .arg("-o")
        .arg(output)
        .output()
        .unwrap_or_else(|error| panic!("start clang for {name}: {error}"));
    assert!(
        result.status.success(),
        "compile {name} fixture: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    set_mode(output, 0o700);
}
