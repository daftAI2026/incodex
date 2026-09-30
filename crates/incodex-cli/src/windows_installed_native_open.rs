#[cfg(test)]
mod tests {
    use super::{NativeOpenAttempt, NativeOpenOutcome, NativeOpenState};
    use std::sync::mpsc;

    #[test]
    fn delayed_ready_keeps_the_same_attempt_pending_until_opened() {
        let (sender, receiver) = mpsc::channel();
        let mut state = NativeOpenState::default();
        let before_ready = state.request(
            || Ok(NativeOpenAttempt::new(41_u32, receiver)),
            |_| Ok(true),
        );
        sender.send(Ok(())).expect("readiness receiver stays owned");
        let after_ready = state.request(|| panic!("must reuse the same child"), |_| Ok(true));

        assert_eq!(
            (before_ready, after_ready),
            (NativeOpenOutcome::Pending, NativeOpenOutcome::Ready)
        );
    }

    #[test]
    fn duplicate_live_request_reuses_child_without_acknowledging_early() {
        let (sender, receiver) = mpsc::channel();
        let mut state = NativeOpenState::default();
        let mut launches = 0;
        let first = state.request(
            || {
                launches += 1;
                Ok(NativeOpenAttempt::new(7_u32, receiver))
            },
            |_| Ok(true),
        );
        let duplicate = state.request(
            || {
                launches += 1;
                Err("duplicate launch".to_string())
            },
            |_| Ok(true),
        );
        sender.send(Ok(())).expect("readiness receiver stays owned");
        let completed = state.request(|| panic!("must reuse the same child"), |_| Ok(true));

        assert_eq!(
            (launches, first, duplicate, completed),
            (
                1,
                NativeOpenOutcome::Pending,
                NativeOpenOutcome::Pending,
                NativeOpenOutcome::Ready
            ),
            "duplicate requests reuse one child and wait for OPENED"
        );
    }

    #[test]
    fn child_exit_before_opened_is_a_failure() {
        let (_sender, receiver) = mpsc::channel();
        let mut state = NativeOpenState::default();
        let outcome = state.request(
            || Ok(NativeOpenAttempt::new(12_u32, receiver)),
            |_| Ok(false),
        );

        assert!(matches!(outcome, NativeOpenOutcome::Failure(_)));
    }

    #[test]
    fn disconnected_readiness_channel_is_a_failure() {
        let (sender, receiver) = mpsc::channel();
        drop(sender);
        let mut state = NativeOpenState::default();
        let outcome = state.request(
            || Ok(NativeOpenAttempt::new(15_u32, receiver)),
            |_| Ok(true),
        );

        assert!(matches!(outcome, NativeOpenOutcome::Failure(_)));
    }
}

use std::sync::mpsc::{Receiver, TryRecvError};

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum NativeOpenOutcome {
    Pending,
    Ready,
    Failure(String),
}

pub(crate) struct NativeOpenAttempt<T> {
    child: T,
    ready: Receiver<Result<(), String>>,
}

impl<T> NativeOpenAttempt<T> {
    pub(crate) fn new(child: T, ready: Receiver<Result<(), String>>) -> Self {
        Self { child, ready }
    }
}

pub(crate) struct NativeOpenState<T> {
    active: Option<NativeOpenAttempt<T>>,
}

impl<T> Default for NativeOpenState<T> {
    fn default() -> Self {
        Self { active: None }
    }
}

impl<T> NativeOpenState<T> {
    pub(crate) fn request<L, A>(
        &mut self,
        launch: L,
        mut child_is_alive: A,
    ) -> NativeOpenOutcome
    where
        L: FnOnce() -> Result<NativeOpenAttempt<T>, String>,
        A: FnMut(&T) -> Result<bool, String>,
    {
        let is_new_attempt = self.active.is_none();
        if is_new_attempt {
            match launch() {
                Ok(attempt) => self.active = Some(attempt),
                Err(error) => return NativeOpenOutcome::Failure(error),
            }
        }

        let Some(active) = self.active.as_mut() else {
            return NativeOpenOutcome::Failure("native Incodex open has no active child".into());
        };
        let child_alive = match child_is_alive(&active.child) {
            Ok(alive) => alive,
            Err(error) => return NativeOpenOutcome::Failure(error),
        };
        if !child_alive {
            self.active.take();
            return NativeOpenOutcome::Failure(
                "native Incodex open exited before the window was ready".into(),
            );
        }

        // RED stub: preserve the existing behavior under test. A fresh live child
        // reports startup failure; a later request treats liveness as readiness.
        if !is_new_attempt {
            return NativeOpenOutcome::Ready;
        }
        match active.ready.try_recv() {
            Ok(Ok(())) => NativeOpenOutcome::Ready,
            Ok(Err(error)) => NativeOpenOutcome::Failure(error),
            Err(TryRecvError::Disconnected) => NativeOpenOutcome::Failure(
                "native Incodex open readiness channel disconnected".into(),
            ),
            Err(TryRecvError::Empty) => NativeOpenOutcome::Failure(
                "native Incodex open is still completing its bounded startup".into(),
            ),
        }
    }
}
