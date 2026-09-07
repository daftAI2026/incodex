//! macOS masked-open supervision. Missing pages belong to shutdown, not mask health.
use super::*;

const CONNECTION_GRACE: Duration = Duration::from_secs(3);
const EXIT_GRACE: Duration = Duration::from_secs(5);

pub(super) fn monitor(
    port: u16,
    alive: &AtomicBool,
    mut on_failure: impl FnMut(&str),
) -> Result<(), String> {
    let mut unhealthy = 0u8;
    let mut missing = 0u8;
    let mut disconnected_since = None;
    let mut exit_deadline = None;
    while alive.load(Ordering::Acquire) {
        thread::sleep(LIFECYCLE_POLL_INTERVAL);
        if !alive.load(Ordering::Acquire) {
            break;
        }
        let probe = probe_profile_mask_health(port, alive, &|_| Ok(()));
        if matches!(&probe, Err(ProfileMaskProbeError::ProbeFailed(detail)) if detail == TARGET_CRASHED_ERROR)
        {
            let error = "page crashed during runtime".to_string();
            on_failure(&error);
            return Err(error);
        }
        if !alive.load(Ordering::Acquire) {
            break;
        }
        let error = match probe {
            Ok(true) => {
                unhealthy = 0;
                missing = 0;
                exit_deadline = None;
                disconnected_since = None;
                continue;
            }
            Ok(false) => {
                missing = 0;
                exit_deadline = None;
                disconnected_since = None;
                unhealthy = unhealthy.saturating_add(1);
                if unhealthy < PROFILE_MASK_FAILURE_POLLS {
                    continue;
                }
                "profile mask failed during runtime: mask could not be restored".to_string()
            }
            Err(ProfileMaskProbeError::TargetMissing) => {
                disconnected_since = None;
                missing = missing.saturating_add(1);
                if missing < PRIMARY_TARGET_MISSING_POLLS {
                    continue;
                }
                // The primary lifecycle monitor requests Browser.close. While no
                // page exists, the next pass only lists targets (no mask eval).
                // A replacement or cancelled close resumes live supervision and
                // clears this deadline; PID liveness alone cannot prove shutdown.
                let deadline = exit_deadline.get_or_insert_with(|| Instant::now() + EXIT_GRACE);
                if Instant::now() < *deadline {
                    continue;
                }
                "window disappeared but the process did not exit within 5 seconds".to_string()
            }
            Err(ProfileMaskProbeError::ProbeFailed(detail)) => {
                missing = 0;
                // A disconnect or crashed renderer does not prove a broken mask.
                // Allow normal process exit or transport recovery, then report the
                // lifecycle fault independently from initial UI acceptance.
                let since = disconnected_since.get_or_insert_with(Instant::now);
                if since.elapsed() < CONNECTION_GRACE {
                    continue;
                }
                format!("page connection failed during runtime: {detail}")
            }
        };
        on_failure(&error);
        return Err(error);
    }
    Ok(())
}
