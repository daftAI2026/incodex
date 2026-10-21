//! [INPUT]: 依赖当前CDP target与profile mask的严格观测结果。
//! [OUTPUT]: macOS masked-open生命周期分类；同epoch两次失败才关闭。
//! [POS]: CDP生命周期监督边界；reload恢复有固定90秒episode。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use super::*;

#[cfg(test)]
const EXIT_GRACE: Duration = Duration::from_secs(5);

struct RecoveryEpisode {
    latest_epoch: f64,
    deadline: Instant,
}

/// Strict mask readiness for one pinned CDP target and its current document.
pub(super) struct DocumentReadiness {
    confirmed_epoch: f64,
    recovery: Option<RecoveryEpisode>,
    unhealthy_samples: u8,
}

impl DocumentReadiness {
    pub(super) fn new(confirmed_epoch: f64) -> Self {
        Self {
            confirmed_epoch,
            recovery: None,
            unhealthy_samples: 0,
        }
    }

    pub(super) fn observe(
        &mut self,
        epoch: f64,
        healthy: bool,
        now: Instant,
    ) -> Result<(), String> {
        if !epoch.is_finite() {
            return Err("malformed Incodex document epoch".into());
        }

        if let Some(recovery) = self.recovery.as_mut() {
            if now >= recovery.deadline {
                return Err("Incodex profile mask did not recover after document reload".into());
            }
            recovery.latest_epoch = epoch;
            if healthy {
                self.confirmed_epoch = recovery.latest_epoch;
                self.recovery = None;
                self.unhealthy_samples = 0;
            }
            return Ok(());
        }

        if epoch != self.confirmed_epoch {
            if healthy {
                self.confirmed_epoch = epoch;
                self.unhealthy_samples = 0;
            } else {
                self.recovery = Some(RecoveryEpisode {
                    latest_epoch: epoch,
                    deadline: now + super::mode::ACTIVE_TIMEOUT,
                });
                self.unhealthy_samples = 0;
            }
            return Ok(());
        }

        if healthy {
            self.unhealthy_samples = 0;
            return Ok(());
        }

        self.unhealthy_samples = self.unhealthy_samples.saturating_add(1);
        if self.unhealthy_samples >= PROFILE_MASK_FAILURE_POLLS {
            return Err("profile mask failed during runtime: mask could not be restored".into());
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn monitor(
    port: u16,
    alive: &AtomicBool,
    mut on_failure: impl FnMut(&str),
) -> Result<(), String> {
    // One suspect observation, scoped to its page. A second uninterrupted
    // observation must confirm it before protective shutdown.
    let mut unhealthy_target: Option<String> = None;
    let mut missing = 0u8;
    let mut disconnected_since = None;
    let mut exit_deadline = None;
    while alive.load(Ordering::Acquire) {
        thread::sleep(LIFECYCLE_POLL_INTERVAL);
        if !alive.load(Ordering::Acquire) {
            break;
        }
        let probe = probe_profile_mask_snapshot(port, alive, &|_| Ok(()));
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
            Ok((target_id, healthy)) => {
                missing = 0;
                exit_deadline = None;
                disconnected_since = None;
                if healthy {
                    unhealthy_target = None;
                    continue;
                }
                if unhealthy_target.as_deref() != Some(target_id.as_str()) {
                    unhealthy_target = Some(target_id);
                    continue;
                }
                "profile mask failed during runtime: mask could not be restored".to_string()
            }
            Err(ProfileMaskProbeError::TargetMissing) => {
                unhealthy_target = None;
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
                unhealthy_target = None;
                missing = 0;
                // A disconnect or crashed renderer does not prove a broken mask.
                // Allow normal process exit or transport recovery, then report the
                // lifecycle fault independently from initial UI acceptance.
                let since = disconnected_since.get_or_insert_with(Instant::now);
                if since.elapsed() < super::MASK_CONNECTION_GRACE {
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

#[cfg(test)]
mod tests {
    use super::DocumentReadiness;
    use std::time::{Duration, Instant};

    #[test]
    fn same_document_requires_two_consecutive_unhealthy_samples() {
        let start = Instant::now();
        let mut readiness = DocumentReadiness::new(1.0);

        assert!(readiness.observe(1.0, false, start).is_ok());
        assert!(readiness
            .observe(1.0, true, start + Duration::from_millis(1))
            .is_ok());
        assert!(readiness
            .observe(1.0, false, start + Duration::from_millis(2))
            .is_ok());
        assert!(readiness
            .observe(1.0, false, start + Duration::from_millis(3))
            .unwrap_err()
            .contains("mask could not be restored"));
    }

    #[test]
    fn reload_recovery_uses_one_deadline_and_strict_ready_rebases_epoch() {
        let start = Instant::now();
        let mut readiness = DocumentReadiness::new(1.0);

        assert!(readiness
            .observe(2.0, false, start + Duration::from_secs(1))
            .is_ok());
        assert!(readiness
            .observe(3.0, false, start + Duration::from_secs(30))
            .is_ok());
        assert!(readiness
            .observe(3.0, true, start + Duration::from_secs(40))
            .is_ok());
        assert!(readiness
            .observe(3.0, false, start + Duration::from_secs(41))
            .is_ok());
        assert!(readiness
            .observe(3.0, false, start + Duration::from_secs(42))
            .unwrap_err()
            .contains("mask could not be restored"));
    }

    #[test]
    fn later_epochs_do_not_extend_the_recovery_deadline() {
        let start = Instant::now();
        let mut readiness = DocumentReadiness::new(1.0);

        assert!(readiness
            .observe(2.0, false, start + Duration::from_secs(1))
            .is_ok());
        assert!(readiness
            .observe(3.0, false, start + Duration::from_secs(80))
            .is_ok());
        assert!(readiness
            .observe(4.0, false, start + Duration::from_secs(90))
            .is_ok());
        assert!(readiness
            .observe(4.0, true, start + Duration::from_secs(91))
            .unwrap_err()
            .contains("did not recover"));
    }

    #[test]
    fn non_finite_epoch_is_a_probe_error_not_pending_recovery() {
        let mut readiness = DocumentReadiness::new(1.0);
        assert!(readiness
            .observe(f64::NAN, false, Instant::now())
            .unwrap_err()
            .contains("malformed Incodex document epoch"));
    }
}
