#![cfg(target_os = "macos")]
#![recursion_limit = "256"]

/**
 * [INPUT]: 编译当前 Coordinator，并复用真实 AppQuiescence 与隔离假宿主。
 * [OUTPUT]: 验证活宿主退出后续接 helper，并保留用户关闭意图。
 * [POS]: 官方更新重开竞态的原生进程回归；不调用真实 App 或恢复 CLI。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[path = "native/macos_update_continuation_support.rs"]
#[allow(dead_code)]
mod support;
use support::{
    canonical, private_dir, process_path, read_json, sha256, wait_for_file, wait_for_process_exit,
    write_private, xcrun, RunRoot,
};

#[path = "native/macos_update_relaunch_race_support.rs"]
mod race_support;
use race_support::*;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn relaunch_retries_only_after_host_exit_and_rechecks_pending_owner() {
    let run = RunRoot::create();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let native_dir = repository.join("crates/incodex-cli/native");
    let coordinator_source = native_dir.join("macos_update_coordinator.m");
    let source_hash = sha256(&fs::read(&coordinator_source).expect("read Coordinator source"));
    let clang = xcrun(&["--find", "clang"]);
    let sdk = xcrun(&["--sdk", "macosx", "--show-sdk-path"]);
    let coordinator_build = run.path.join("coordinator-build");
    private_dir(&coordinator_build);
    let coordinator_binary = coordinator_build.join("incodex-update-coordinator");
    compile_coordinator(
        &clang,
        &sdk,
        &native_dir,
        &coordinator_source,
        &coordinator_binary,
        &run.path,
    );
    let coordinator_hash = sha256(&fs::read(&coordinator_binary).expect("read Coordinator binary"));
    let test_binary = std::env::current_exe()
        .expect("locate this integration-test binary")
        .canonicalize()
        .expect("canonicalize integration-test binary");

    let live_retry = run_live_case(
        &run.path,
        "retry-after-host-exit",
        &coordinator_binary,
        &coordinator_hash,
        &test_binary,
        PendingMutation::Keep,
    );
    let cancelled = run_live_case(
        &run.path,
        "cancelled-pending",
        &coordinator_binary,
        &coordinator_hash,
        &test_binary,
        PendingMutation::Remove,
    );
    let owner_changed = run_live_case(
        &run.path,
        "changed-owner",
        &coordinator_binary,
        &coordinator_hash,
        &test_binary,
        PendingMutation::ChangeHandoff,
    );
    let generic_failure = run_generic_failure_case(
        &run.path,
        &coordinator_binary,
        &coordinator_hash,
        &test_binary,
    );

    let result = json!({
        "schemaVersion": 1,
        "sourceCommit": git_head(&repository),
        "coordinatorSourceSha256": source_hash,
        "coordinatorBinarySha256": coordinator_hash,
        "testBinary": test_binary.display().to_string(),
        "scope": "isolated Coordinator, real AppQuiescence and owned fake Host SIGTERM; no real App Quit/TCC/CLI install",
        "liveHostRetry": live_retry,
        "pendingCancelledWhileWaiting": cancelled,
        "pendingOwnerChangedWhileWaiting": owner_changed,
        "genericHelperFailureWithoutHost": generic_failure,
    });
    write_json_private(&run.path.join("result.json"), &result);

    assert!(
        live_retry["coordinatorWaitedWithOwnedPending"] == true
            && live_retry["helperCountWhileHostLive"] == 1
            && live_retry["hostPathMatches"] == true
            && live_retry["quiescenceSawLiveHost"] == true
            && live_retry["hostQuiescencePositiveControl"]["realProbeFoundExactHost"] == true
            && live_retry["helperParentWasCoordinator"] == true
            && live_retry["helperParentExecutableMatchesCoordinator"] == true
            && live_retry["probeHadNotRunBeforeHostStart"] == true
            && live_retry["firstHelperExitCode"]
                .as_i64()
                .is_some_and(|code| code != 0)
            && live_retry["firstProbe"]["quiescent"] == false
            && live_retry["firstProbe"]["runningPids"]
                .as_array()
                .is_some_and(|pids| {
                    live_retry["hostPid"].as_u64().is_some_and(|host_pid| {
                        pids.iter().any(|pid| pid.as_i64() == Some(host_pid as i64))
                    })
                })
            && live_retry["firstProbePidMatchesHelperChild"] == true
            && live_retry["firstProbeParentIsHelper"] == true
            && live_retry["firstHelperProcessExited"] == true
            && live_retry["firstProbeProcessExited"] == true
            && live_retry["ownedHostExitObserved"] == true
            && live_retry["hostMarkerRemovedAfterNormalExit"] == true
            && live_retry["secondHelperUsedSameCoordinator"] == true
            && live_retry["secondHelperExitCode"] == 0
            && live_retry["secondHelperProcessExited"] == true
            && live_retry["secondProbeProcessExited"] == true
            && live_retry["secondQuiescencePassed"] == true
            && live_retry["pendingRemovedAfterRetry"] == true
            && live_retry["pendingIdentityRetainedAfterHostExit"] == true
            && live_retry["pendingRelaunchHostClearedAfterNormalExit"] == true
            && live_retry["hostRelaunched"] == false,
        "Coordinator must retain the handoff, clear relaunch after normal Host exit, and retry without reopening Host; evidence={}",
        run.path.join("result.json").display()
    );
    assert!(
        cancelled["coordinatorWaitedWithOwnedPending"] == true
            && cancelled["pendingCancelled"] == true
            && cancelled["helperCountAfterHostExit"] == 1
            && cancelled["hostRelaunched"] == false,
        "a cancelled handoff must not recover after the Host exits; evidence={}",
        run.path.join("result.json").display()
    );
    assert!(
        owner_changed["coordinatorWaitedWithOwnedPending"] == true
            && owner_changed["ownerChanged"] == true
            && owner_changed["helperCountAfterHostExit"] == 1
            && owner_changed["hostRelaunched"] == false,
        "a changed handoff owner must not recover after the Host exits; evidence={}",
        run.path.join("result.json").display()
    );
    assert!(
        generic_failure["coordinatorExited"] == true
            && generic_failure["targetQuiescentBeforeHelper"] == true
            && generic_failure["helperCount"] == 1
            && generic_failure["helperExitCode"] == 23
            && generic_failure["noRetryDuringBoundedWindow"] == true
            && generic_failure["pendingRetained"] == true,
        "a generic helper failure without a live Host must not retry without bound; evidence={}",
        run.path.join("result.json").display()
    );
}

#[test]
fn helper_quiescence_probe() {
    let Some(events) = std::env::var_os("INCODEX_TEST_RACE_EVENTS").map(PathBuf::from) else {
        return;
    };
    let host_app = std::env::var_os("INCODEX_TEST_RACE_HOST_APP")
        .map(PathBuf::from)
        .expect("fixture helper receives its synthetic Host path");
    let host_executable = host_executable(&host_app);
    let current = std::env::current_exe().expect("locate helper test process");
    let result = match incodex_macos::AppQuiescence::for_app(&host_app) {
        Ok(target) => {
            let running = target.running_pids();
            let check = target.ensure_quiescent();
            match (running, check) {
                (Ok(pids), Ok(())) => (Some(pids), None),
                (Ok(pids), Err(error)) => (Some(pids), Some(error)),
                (Err(error), _) => (None, Some(error)),
            }
        }
        Err(error) => (None, Some(error)),
    };
    let report = json!({
        "pid": std::process::id(),
        "ppid": unsafe { libc::getppid() },
        "executable": current.display().to_string(),
        "hostApp": host_app.display().to_string(),
        "hostExecutable": host_executable.display().to_string(),
        "runningPids": result.0,
        "quiescenceError": result.1,
        "quiescent": result.1.is_none(),
        "probe": "incodex_macos::AppQuiescence::for_app + running_pids + ensure_quiescent",
    });
    let path = events.join(format!("helper-probe-{}.json", std::process::id()));
    write_json_private(&path, &report);
    assert!(
        report["quiescent"] == true,
        "real AppQuiescence refused the live target: {}",
        path.display()
    );
}

#[derive(Clone, Copy)]
enum PendingMutation {
    Keep,
    Remove,
    ChangeHandoff,
}

struct CasePaths {
    root: PathBuf,
    home: PathBuf,
    tmp: PathBuf,
    events: PathBuf,
    coordinator_executable: PathBuf,
    host_app: PathBuf,
    host_executable: PathBuf,
    host_marker: PathBuf,
    pending: PathBuf,
    install_id: String,
    handoff_id: String,
}

fn run_live_case(
    root: &Path,
    name: &str,
    coordinator_binary: &Path,
    coordinator_hash: &str,
    test_binary: &Path,
    mutation: PendingMutation,
) -> Value {
    let paths = prepare_case(
        root,
        name,
        coordinator_binary,
        coordinator_hash,
        test_binary,
    );
    let host_positive = host_quiescence_positive_control(&paths);
    let mut coordinator = start_coordinator(&paths, "live");
    let coordinator_pid = coordinator.id();
    let first_start = wait_for_start(&paths.events, 1, CHILD_TIMEOUT);
    let Some(first_start) = first_start else {
        let exit = coordinator.wait_timeout(Duration::from_secs(1));
        return write_case_result(
            &paths,
            json!({
                "case": name,
                "failure": "Coordinator did not start the registered helper",
                "coordinatorPid": coordinator_pid,
                "coordinatorExit": exit,
                "hostPositiveControl": host_positive,
                "pending": read_json(&paths.pending).ok(),
                "helperStarts": read_start_markers(&paths.events),
            }),
        );
    };
    let helper_pid = first_start["pid"].as_u64().unwrap_or_default() as u32;
    let helper_parent = first_start["ppid"].as_u64().unwrap_or_default() as u32;
    let helper_process_path = process_path(helper_pid);
    let helper_parent_path = process_path(helper_parent);
    let no_probe_before_host = fs::read_dir(&paths.events).is_ok_and(|entries| {
        entries.flatten().all(|entry| {
            !entry
                .file_name()
                .to_string_lossy()
                .starts_with("helper-probe-")
        })
    });
    let mut host = start_host(&paths);
    let host_pid = host.id();
    let host_path_matches = process_path(host_pid).as_deref().map(canonical).as_ref()
        == Some(&canonical(&paths.host_executable));
    let quiescence_sees_host = incodex_macos::AppQuiescence::for_app(&paths.host_app)
        .and_then(|target| {
            let pids = target.running_pids()?;
            if pids.contains(&(host_pid as i32)) {
                Ok(())
            } else {
                Err("real process probe did not find the fake Host PID".to_string())
            }
        })
        .is_ok();
    write_private(&paths.events.join("release-first-helper"), b"release\n");
    let first_end = wait_for_end(&paths.events, 1, CHILD_TIMEOUT);
    let first_end_code = first_end
        .as_ref()
        .and_then(|event| event["exitCode"].as_i64());
    let probe_pid = first_end
        .as_ref()
        .and_then(|event| event["childPid"].as_u64())
        .and_then(|pid| u32::try_from(pid).ok());
    let first_probe_path =
        probe_pid.map(|pid| paths.events.join(format!("helper-probe-{pid}.json")));
    let first_probe_observed = first_probe_path
        .as_deref()
        .is_some_and(|path| wait_for_file(path, CHILD_TIMEOUT));
    let first_probe = first_probe_path
        .as_deref()
        .and_then(|path| read_json(path).ok());
    let helper_process_exited = helper_process_path
        .as_deref()
        .is_some_and(|executable| wait_for_process_exit(helper_pid, executable, CHILD_TIMEOUT));
    let probe_process_exited =
        probe_pid.is_some_and(|pid| wait_for_process_exit(pid, test_binary, CHILD_TIMEOUT));
    thread::sleep(HELPER_QUIET_WINDOW);
    let coordinator_waited = coordinator.is_running();
    let pending_after_failure = read_pending_locked(&paths.pending).ok();
    let pending_owned_after_failure = pending_after_failure.as_ref().is_some_and(|pending| {
        pending["coordinatorPid"] == json!(coordinator_pid)
            && pending["installId"] == paths.install_id
            && pending["appPath"] == paths.host_app.display().to_string()
            && pending["handoffId"] == paths.handoff_id
            && pending["relaunchHost"] == false
    });
    let starts_while_live = read_start_markers(&paths.events);
    let helper_count_while_live = starts_while_live.len();
    let waiting_with_owned_pending = coordinator_waited && pending_owned_after_failure;

    let mut mutation_applied = false;
    if let (true, PendingMutation::Remove) = (coordinator_waited, mutation) {
        mutation_applied = mutate_pending(&paths, PendingMutation::Remove, coordinator_pid);
    } else if let (true, PendingMutation::ChangeHandoff) = (coordinator_waited, mutation) {
        mutation_applied = mutate_pending(&paths, PendingMutation::ChangeHandoff, coordinator_pid);
    }
    let host_termination_requested = host.request_normal_exit();
    let host_exit_status = host.wait_timeout(CHILD_TIMEOUT);
    let host_normal_exit = host_termination_requested
        && host_exit_status.is_some_and(|status| status.signal == Some(libc::SIGTERM));
    let host_marker_removed_after_normal_exit =
        host_normal_exit && fs::remove_file(&paths.host_marker).is_ok();
    let mut pending_after_host_exit_before_retry = read_pending_locked(&paths.pending).ok();

    let mut second_start = None;
    let mut second_end = None;
    let mut second_probe = None;
    let mut second_helper_process_path = None;
    let mut second_helper_process_exited = false;
    let mut second_probe_process_exited = false;
    let mut retry_coordinator_same = false;
    let mut pending_removed_after_retry = false;
    let mut coordinator_exited_after_resolution = false;
    let mut host_relaunched = false;
    let mut no_relaunch_observed_ms = None;
    match mutation {
        PendingMutation::Keep if waiting_with_owned_pending && host_normal_exit => {
            let deadline = Instant::now() + RETRY_WINDOW;
            while Instant::now() < deadline {
                if let Ok(pending) = read_pending_locked(&paths.pending) {
                    if pending["relaunchHost"] == false {
                        pending_after_host_exit_before_retry = Some(pending);
                        break;
                    }
                }
                thread::sleep(Duration::from_millis(20));
            }
            second_start = wait_for_start(&paths.events, 2, RETRY_WINDOW);
            if let Some(start) = &second_start {
                retry_coordinator_same = start["ppid"] == json!(coordinator_pid);
                let pid = start["pid"].as_u64().unwrap_or_default() as u32;
                second_helper_process_path = process_path(pid);
                write_private(&paths.events.join("release-retry-helper"), b"release\n");
                second_end = wait_for_end(&paths.events, 2, RETRY_WINDOW);
                if let Some(child) = second_end
                    .as_ref()
                    .and_then(|event| event["childPid"].as_u64())
                {
                    let probe_path = paths.events.join(format!("helper-probe-{child}.json"));
                    if wait_for_file(&probe_path, RETRY_WINDOW) {
                        second_probe = read_json(&probe_path).ok();
                    }
                }
                second_helper_process_exited = second_helper_process_path
                    .as_deref()
                    .is_some_and(|executable| wait_for_process_exit(pid, executable, RETRY_WINDOW));
                second_probe_process_exited = second_end
                    .as_ref()
                    .and_then(|event| event["childPid"].as_u64())
                    .and_then(|child| u32::try_from(child).ok())
                    .is_some_and(|child| wait_for_process_exit(child, test_binary, RETRY_WINDOW));
            }
            pending_removed_after_retry = wait_for_absence(&paths.pending, RETRY_WINDOW);
            coordinator_exited_after_resolution = coordinator.wait_timeout(RETRY_WINDOW).is_some();
            let observation_started = Instant::now();
            host_relaunched = wait_for_file(&paths.host_marker, HOST_NO_RELAUNCH_WINDOW);
            no_relaunch_observed_ms = Some(observation_started.elapsed().as_millis());
        }
        PendingMutation::Remove | PendingMutation::ChangeHandoff => {
            let current = read_pending_locked(&paths.pending).ok();
            mutation_applied = mutation_applied
                && match mutation {
                    PendingMutation::Remove => current.is_none(),
                    PendingMutation::ChangeHandoff => current.as_ref().is_some_and(|pending| {
                        pending["handoffId"] == format!("{}-replacement", paths.handoff_id)
                    }),
                    PendingMutation::Keep => false,
                };
            coordinator_exited_after_resolution = coordinator.wait_timeout(CANCEL_WINDOW).is_some();
            thread::sleep(CANCEL_WINDOW);
            second_start = wait_for_start(&paths.events, 2, Duration::from_millis(1));
            host_relaunched = paths.host_marker.exists();
        }
        PendingMutation::Keep => {}
    }
    cleanup_launch_services_host(&paths);
    if !host_normal_exit && host.is_running() {
        let _ = host.request_normal_exit();
        let _ = host.wait_timeout(CHILD_TIMEOUT);
    }
    let helper_count_after_host_exit = read_start_markers(&paths.events).len();
    let coordinator_status = coordinator.exit_snapshot();
    let second_helper_exit_code = second_end
        .as_ref()
        .and_then(|event| event["exitCode"].as_i64());
    let pending_relaunch_host_cleared = pending_after_host_exit_before_retry
        .as_ref()
        .is_some_and(|pending| pending["relaunchHost"] == false);
    let pending_identity_retained_after_host_exit = pending_after_host_exit_before_retry
        .as_ref()
        .is_some_and(|pending| {
            pending["coordinatorPid"] == json!(coordinator_pid)
                && pending["installId"] == paths.install_id
                && pending["appPath"] == paths.host_app.display().to_string()
                && pending["handoffId"] == paths.handoff_id
        });
    let receipt = json!({
        "case": name,
        "sourceCommit": git_head(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")),
        "coordinatorPid": coordinator_pid,
        "coordinatorExecutable": paths.coordinator_executable.display().to_string(),
        "coordinatorWaitedWithOwnedPending": waiting_with_owned_pending,
        "coordinatorStatus": coordinator_status,
        "coordinatorExitedAfterResolution": coordinator_exited_after_resolution,
        "pendingBeforeHostExit": pending_after_failure,
        "pendingMutationApplied": mutation_applied,
        "pendingCancelled": matches!(mutation, PendingMutation::Remove) && mutation_applied,
        "ownerChanged": matches!(mutation, PendingMutation::ChangeHandoff) && mutation_applied,
        "pendingRemovedAfterRetry": pending_removed_after_retry,
        "hostPid": host_pid,
        "hostExecutable": paths.host_executable.display().to_string(),
        "hostPathMatches": host_path_matches,
        "hostQuiescencePositiveControl": host_positive,
        "quiescenceSawLiveHost": quiescence_sees_host,
        "helperStartBeforeHost": first_start,
        "helperParentWasCoordinator": helper_parent == coordinator_pid,
        "helperParentExecutable": helper_parent_path.as_ref().map(|path| path.display().to_string()),
        "helperParentExecutableMatchesCoordinator": helper_parent_path
            .as_deref()
            .is_some_and(|path| canonical(path) == canonical(&paths.coordinator_executable)),
        "probeHadNotRunBeforeHostStart": no_probe_before_host,
        "firstProbeObserved": first_probe_observed,
        "firstProbePath": first_probe_path.as_ref().map(|path| path.display().to_string()),
        "firstProbe": first_probe,
        "firstProbePidMatchesHelperChild": first_probe
            .as_ref()
            .zip(first_end.as_ref())
            .is_some_and(|(probe, end)| probe["pid"] == end["childPid"]),
        "firstProbeParentIsHelper": first_probe
            .as_ref()
            .is_some_and(|probe| probe["ppid"] == json!(helper_pid)),
        "firstHelperExitCode": first_end_code,
        "helperProcessPath": helper_process_path.map(|path| path.display().to_string()),
        "firstHelperProcessExited": helper_process_exited,
        "firstProbePid": probe_pid,
        "firstProbeProcessExited": probe_process_exited,
        "helperCountWhileHostLive": helper_count_while_live,
        "ownedHostExitObserved": host_normal_exit,
        "hostMarkerRemovedAfterNormalExit": host_marker_removed_after_normal_exit,
        "hostTerminationRequested": host_termination_requested,
        "hostExitStatus": host_exit_status,
        "pendingAfterHostExitBeforeRetry": pending_after_host_exit_before_retry,
        "pendingRelaunchHostClearedAfterNormalExit": pending_relaunch_host_cleared,
        "pendingIdentityRetainedAfterHostExit": pending_identity_retained_after_host_exit,
        "secondHelperStart": second_start,
        "secondHelperEnd": second_end,
        "secondHelperUsedSameCoordinator": retry_coordinator_same,
        "secondHelperExitCode": second_helper_exit_code,
        "secondHelperProcessPath": second_helper_process_path
            .as_ref()
            .map(|path| path.display().to_string()),
        "secondHelperProcessExited": second_helper_process_exited,
        "secondProbeProcessExited": second_probe_process_exited,
        "secondQuiescencePassed": second_probe.as_ref().is_some_and(|probe| probe["quiescent"] == true),
        "secondProbe": second_probe,
        "helperCountAfterHostExit": helper_count_after_host_exit,
        "hostRelaunched": host_relaunched,
        "noHostRelaunchObservationMs": no_relaunch_observed_ms,
        "pendingAfter": read_pending_locked(&paths.pending).ok(),
        "evidenceDirectory": paths.root.display().to_string(),
    });
    write_case_result(&paths, receipt)
}

fn run_generic_failure_case(
    root: &Path,
    coordinator_binary: &Path,
    coordinator_hash: &str,
    test_binary: &Path,
) -> Value {
    let paths = prepare_case(
        root,
        "generic-failure",
        coordinator_binary,
        coordinator_hash,
        test_binary,
    );
    let target_quiescence =
        incodex_macos::AppQuiescence::for_app(&paths.host_app).and_then(|target| {
            let pids = target.running_pids()?;
            if pids.is_empty() {
                Ok(pids)
            } else {
                Err(format!("unexpected live fake Host PIDs: {pids:?}"))
            }
        });
    let mut coordinator = start_coordinator(&paths, "generic");
    let coordinator_pid = coordinator.id();
    let _ = wait_for_start(&paths.events, 1, CHILD_TIMEOUT);
    let end = wait_for_end(&paths.events, 1, CHILD_TIMEOUT);
    let coordinator_exited = coordinator.wait_timeout(CHILD_TIMEOUT).is_some();
    let mut no_retry_window = true;
    let deadline = Instant::now() + CANCEL_WINDOW;
    while Instant::now() < deadline {
        if read_start_markers(&paths.events).len() != 1 {
            no_retry_window = false;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let helper_exit_code = end.as_ref().and_then(|event| event["exitCode"].as_i64());
    let target_quiescent_before_helper = target_quiescence.is_ok();
    let target_quiescence_error = target_quiescence.err();
    let receipt = json!({
        "case": "generic-helper-failure-without-live-host",
        "coordinatorPid": coordinator_pid,
        "coordinatorExecutable": paths.coordinator_executable.display().to_string(),
        "coordinatorExited": coordinator_exited,
        "helperEnd": end,
        "helperExitCode": helper_exit_code,
        "helperCount": read_start_markers(&paths.events).len(),
        "noRetryDuringBoundedWindow": no_retry_window,
        "targetQuiescentBeforeHelper": target_quiescent_before_helper,
        "targetQuiescenceError": target_quiescence_error,
        "pendingRetained": paths.pending.exists(),
        "hostRelaunched": paths.host_marker.exists(),
        "pending": read_pending_locked(&paths.pending).ok(),
        "evidenceDirectory": paths.root.display().to_string(),
    });
    write_case_result(&paths, receipt)
}
