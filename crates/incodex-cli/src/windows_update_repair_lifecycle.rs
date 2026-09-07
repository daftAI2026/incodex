// 安装态路由与更新协调器共用 helper；此边界负责两者的执行顺序。
use std::sync::mpsc::{self, Sender};

use crate::windows_update_repair::CoordinatorEvent;

pub(crate) fn run_installed_route_with<R, C>(route: R, coordinator: C) -> Result<(), String>
where
    R: FnOnce() -> Result<(), String>,
    C: FnOnce(Sender<Sender<CoordinatorEvent>>) -> Result<(), String> + Send,
{
    let (ready, _receiver) = mpsc::channel();
    route()?;
    let _ = coordinator(ready);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::Duration;

    #[test]
    fn subscription_is_ready_and_serves_events_while_bridge_is_active() {
        let subscribed = Arc::new(AtomicBool::new(false));
        let worker_subscribed = subscribed.clone();
        let (event, events) = mpsc::channel();
        let (ack, acknowledged) = mpsc::channel();
        let result = run_installed_route_with(
            || {
                assert!(
                    subscribed.load(Ordering::SeqCst),
                    "bridge started before subscription"
                );
                event.send(()).unwrap();
                acknowledged
                    .recv_timeout(Duration::from_secs(2))
                    .map_err(|_| "coordinator blocked behind the live bridge".to_string())
            },
            move |ready| {
                let (cancel, _cancelled) = mpsc::channel();
                worker_subscribed.store(true, Ordering::SeqCst);
                ready.send(cancel).unwrap();
                events.recv_timeout(Duration::from_secs(2)).unwrap();
                ack.send(()).unwrap();
                Ok(())
            },
        );
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn failed_bridge_cancels_and_joins_the_subscribed_coordinator() {
        let cancelled = AtomicBool::new(false);
        let result = run_installed_route_with(
            || Err("bridge failed".to_string()),
            |ready| {
                let (sender, receiver) = mpsc::channel();
                ready.send(sender).unwrap();
                receiver.recv_timeout(Duration::from_secs(2)).unwrap();
                cancelled.store(true, Ordering::SeqCst);
                Ok(())
            },
        );
        assert_eq!(result, Err("bridge failed".to_string()));
        assert!(
            cancelled.load(Ordering::SeqCst),
            "failed bridge left coordinator alive"
        );
    }

    #[test]
    fn unavailable_subscription_preserves_official_launch() {
        let launched = AtomicBool::new(false);
        let result = run_installed_route_with(
            || {
                launched.store(true, Ordering::SeqCst);
                Ok(())
            },
            |_ready| Err("subscription unavailable".to_string()),
        );
        assert_eq!(result, Ok(()));
        assert!(launched.load(Ordering::SeqCst));
    }

    #[test]
    fn normal_bridge_exit_waits_for_update_work_instead_of_cancelling_it() {
        let finished = AtomicBool::new(false);
        let (closed, owner_exit) = mpsc::channel();
        let result = run_installed_route_with(
            || {
                closed.send(()).unwrap();
                Ok(())
            },
            |ready| {
                let (sender, receiver) = mpsc::channel();
                let _ = ready.send(sender);
                owner_exit.recv_timeout(Duration::from_secs(2)).unwrap();
                assert!(receiver.try_recv().is_err());
                finished.store(true, Ordering::SeqCst);
                Ok(())
            },
        );
        assert_eq!(result, Ok(()));
        assert!(finished.load(Ordering::SeqCst));
    }
}
