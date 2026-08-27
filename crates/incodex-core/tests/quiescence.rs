use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use incodex_core::quiescence::{
    request_normal_exit_and_wait_with, QuiescenceClock, QuiescenceError, QUIESCENCE_POLL_INTERVAL,
    QUIESCENCE_TIMEOUT,
};

struct FakeClock {
    now: Instant,
    sleeps: Vec<Duration>,
}

impl FakeClock {
    fn new() -> Self {
        Self {
            now: Instant::now(),
            sleeps: Vec::new(),
        }
    }
}

impl QuiescenceClock for FakeClock {
    fn now(&self) -> Instant {
        self.now
    }

    fn sleep(&mut self, duration: Duration) {
        self.sleeps.push(duration);
        self.now += duration;
    }
}

#[test]
fn no_running_process_sends_no_exit_request() {
    let requests = Cell::new(0);
    let mut clock = FakeClock::new();

    request_normal_exit_and_wait_with(
        || Ok::<_, String>(Vec::<u32>::new()),
        |_| {
            requests.set(requests.get() + 1);
            Ok(())
        },
        &mut clock,
    )
    .expect("already quiescent");

    assert_eq!(requests.get(), 0);
    assert!(clock.sleeps.is_empty());
}

#[test]
fn one_normal_request_is_followed_by_shared_bounded_polling() {
    let observations = RefCell::new(vec![vec![42_u32], vec![42], Vec::new()].into_iter());
    let requested = RefCell::new(Vec::new());
    let mut clock = FakeClock::new();

    request_normal_exit_and_wait_with(
        || {
            observations
                .borrow_mut()
                .next()
                .ok_or("unexpected poll".to_string())
        },
        |pids| {
            requested.borrow_mut().push(pids.to_vec());
            Ok(())
        },
        &mut clock,
    )
    .expect("normal exit observed");

    assert_eq!(requested.into_inner(), vec![vec![42]]);
    assert_eq!(clock.sleeps, vec![QUIESCENCE_POLL_INTERVAL]);
}

#[test]
fn cancellation_or_no_response_times_out_without_a_second_request() {
    let requests = Cell::new(0);
    let mut clock = FakeClock::new();

    let error = request_normal_exit_and_wait_with(
        || Ok::<_, String>(vec![42_u32]),
        |_| {
            requests.set(requests.get() + 1);
            Ok(())
        },
        &mut clock,
    )
    .expect_err("a live process must stop the mutation");

    assert_eq!(error, QuiescenceError::TimedOut);
    assert_eq!(requests.get(), 1);
    assert_eq!(clock.sleeps.iter().sum::<Duration>(), QUIESCENCE_TIMEOUT);
    assert!(clock
        .sleeps
        .iter()
        .all(|duration| *duration == QUIESCENCE_POLL_INTERVAL));
}

#[test]
fn request_and_probe_uncertainty_fail_closed() {
    let mut request_clock = FakeClock::new();
    let request_error = request_normal_exit_and_wait_with(
        || Ok::<_, String>(vec![42_u32]),
        |_| Err("request refused".to_string()),
        &mut request_clock,
    )
    .expect_err("request uncertainty");
    assert_eq!(
        request_error,
        QuiescenceError::Request("request refused".to_string())
    );

    let mut probe_clock = FakeClock::new();
    let probe_error = request_normal_exit_and_wait_with(
        || Err::<Vec<u32>, _>("probe failed".to_string()),
        |_| Ok(()),
        &mut probe_clock,
    )
    .expect_err("probe uncertainty");
    assert_eq!(
        probe_error,
        QuiescenceError::Probe("probe failed".to_string())
    );
}
