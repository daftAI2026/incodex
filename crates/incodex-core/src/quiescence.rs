use std::time::{Duration, Instant};

pub const QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(60);
pub const QUIESCENCE_POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuiescenceError {
    Probe(String),
    Request(String),
    TimedOut,
}

/// Injectable monotonic clock used by every platform's bounded normal-exit flow.
pub trait QuiescenceClock {
    fn now(&self) -> Instant;
    fn sleep(&mut self, duration: Duration);
}

/// Requests normal application exit once, then waits for process identity evidence to disappear.
/// Probe or request uncertainty fails closed; this function never escalates to forced termination.
pub fn request_normal_exit_and_wait_with<T, P, R, C>(
    mut probe: P,
    mut request: R,
    clock: &mut C,
) -> Result<(), QuiescenceError>
where
    P: FnMut() -> Result<Vec<T>, String>,
    R: FnMut(&[T]) -> Result<(), String>,
    C: QuiescenceClock,
{
    let running = probe().map_err(QuiescenceError::Probe)?;
    if running.is_empty() {
        return Ok(());
    }
    request(&running).map_err(QuiescenceError::Request)?;

    let deadline = clock.now() + QUIESCENCE_TIMEOUT;
    loop {
        if probe().map_err(QuiescenceError::Probe)?.is_empty() {
            return Ok(());
        }
        if clock.now() >= deadline {
            return Err(QuiescenceError::TimedOut);
        }
        clock.sleep(QUIESCENCE_POLL_INTERVAL);
    }
}
