//! [INPUT]: 依赖open masked worker、保留CDP session与loopback主页面夹具。
//! [OUTPUT]: 验证reload后按同一document epoch重验严格健康，不重复注册或Ready。
//! [POS]: open reload专项回归；不启动官方Host，不访问权限接口。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::cdp::InjectionOptions;
use crate::profile_mask::{ProfileAvatar, ProfileMask};

#[test]
fn masked_worker_rechecks_same_session_until_new_document_is_strictly_ready() {
    let options = InjectionOptions {
        profile_mask: Some(ProfileMask {
            name: "Fixture Identity".into(),
            avatar: ProfileAvatar::Generated,
        }),
        ..InjectionOptions::default()
    };
    let fixture = ReloadFixture::start_with_options(options.clone());
    let alive = Arc::new(AtomicBool::new(true));
    let (status_tx, status_rx) = mpsc::channel();
    let mut worker = InjectionWorker {
        alive: alive.clone(),
        thread: Some(start_injection_worker(
            fixture.port,
            options,
            status_tx,
            Arc::new(AtomicBool::new(false)),
            alive,
        )),
    };

    assert!(wait_for_ready(&status_rx, Duration::from_secs(5)));
    let registration = fixture.next_connection(Duration::from_secs(2));
    assert!(fixture.wait_for_method(
        registration.id,
        "Page.addScriptToEvaluateOnNewDocument",
        Duration::from_secs(2)
    ));
    assert!(fixture.wait_for_expression("performance.timeOrigin", Duration::from_secs(2)));

    // 同epoch事件和子frame导航不能启动document recovery。
    assert!(registration
        .control
        .send(Control::SameEpochMainFrameEvent)
        .is_ok());
    assert!(registration.control.send(Control::SubframeNavigate).is_ok());
    thread::sleep(Duration::from_millis(100));
    assert_eq!(fixture.state.document_epoch.load(Ordering::Acquire), 1);

    // 新主文档在React表面挂载前报告false。已注册脚本会在挂载后恢复它。
    assert!(registration.control.send(Control::MainFrameReload).is_ok());
    let epoch_deadline = Instant::now() + Duration::from_secs(2);
    let mut runtime_failure_seen = false;
    while Instant::now() < epoch_deadline && !fixture.session_observed_epoch(registration.id, 2) {
        runtime_failure_seen |= status_rx.try_iter().any(|status| {
            matches!(
                status,
                InjectionStatus::RuntimeFailed(_) | InjectionStatus::Failed(_)
            )
        });
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        fixture.session_observed_epoch(registration.id, 2),
        "same retained session must sample the new document epoch; runtime failure before sample={runtime_failure_seen}; observed={:?}",
        fixture.state.sessions.lock().unwrap().get(&registration.id)
    );
    assert!(fixture.wait_for_reload_replay(registration.id, Duration::from_secs(1)));

    let ready_deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < ready_deadline
        && !fixture.session_observed_healthy_epoch(registration.id, 2)
    {
        runtime_failure_seen |= status_rx.try_iter().any(|status| {
            matches!(
                status,
                InjectionStatus::RuntimeFailed(_) | InjectionStatus::Failed(_)
            )
        });
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        fixture.session_observed_healthy_epoch(registration.id, 2),
        "the same strict readiness probe must confirm recovery for epoch 2"
    );

    let statuses: Vec<_> = status_rx.try_iter().collect();
    assert!(
        !runtime_failure_seen
            && !statuses.iter().any(|status| {
                matches!(
                    status,
                    InjectionStatus::RuntimeFailed(_) | InjectionStatus::Failed(_)
                )
            }),
        "bootstrap false must remain pending inside the fixed recovery episode"
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| matches!(status, InjectionStatus::Ready))
            .count(),
        0,
        "reload recovery must not publish OPENED a second time"
    );
    assert_eq!(
        fixture.registered_script_count(),
        1,
        "same-session reload already runs the registered script"
    );
    assert_eq!(
        fixture.payload_evaluation_count(registration.id),
        1,
        "reload readiness must not replay the full Runtime payload"
    );
    assert!(
        !fixture
            .state
            .sessions
            .lock()
            .unwrap()
            .get(&registration.id)
            .is_some_and(|session| session.closed),
        "reload recovery must retain the existing registration session"
    );

    worker.alive.store(false, Ordering::Release);
    worker.stop_and_join();
    assert!(fixture.wait_for_close(registration.id, Duration::from_secs(3)));
}

#[test]
fn settled_mask_failure_still_closes_after_two_same_epoch_probes() {
    let options = InjectionOptions {
        profile_mask: Some(ProfileMask {
            name: "Fixture Identity".into(),
            avatar: ProfileAvatar::Generated,
        }),
        ..InjectionOptions::default()
    };
    let fixture = ReloadFixture::start_with_options(options.clone());
    let alive = Arc::new(AtomicBool::new(true));
    let (status_tx, status_rx) = mpsc::channel();
    let mut worker = InjectionWorker {
        alive: alive.clone(),
        thread: Some(start_injection_worker(
            fixture.port,
            options,
            status_tx,
            Arc::new(AtomicBool::new(false)),
            alive,
        )),
    };

    assert!(wait_for_ready(&status_rx, Duration::from_secs(5)));
    let registration = fixture.next_connection(Duration::from_secs(2));
    assert!(registration.control.send(Control::MaskFailure).is_ok());
    let deadline = Instant::now() + Duration::from_secs(2);
    let failure = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "mask failure must be reported promptly"
        );
        match status_rx.recv_timeout(remaining).unwrap() {
            status @ InjectionStatus::RuntimeFailed(_) => break status,
            _ => {}
        }
    };
    assert!(matches!(failure, InjectionStatus::RuntimeFailed(_)));
    worker.stop_and_join();
    assert!(fixture.wait_for_close(registration.id, Duration::from_secs(2)));
}
