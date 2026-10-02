// 安装态路由与更新协调器共用 helper；此边界负责两者的执行顺序。
use std::sync::mpsc::{self, Sender};

use crate::windows_update_repair::CoordinatorEvent;

pub(crate) fn run_installed_route_with<R, C>(route: R, coordinator: C) -> Result<(), String>
where
    R: FnOnce() -> Result<(), String>,
    C: FnOnce(Sender<Sender<CoordinatorEvent>>) -> Result<(), String> + Send,
{
    std::thread::scope(|scope| {
        let (ready, receiver) = mpsc::channel();
        // 协调器的 WinRT 初始化、订阅、等待和析构始终属于同一线程。
        let worker = match std::thread::Builder::new()
            .name("incodex-update-repair".to_string())
            .spawn_scoped(scope, move || coordinator(ready))
        {
            Ok(worker) => worker,
            Err(error) => {
                eprintln!("Windows update repair unavailable: {error}");
                return route();
            }
        };
        // 订阅完成后才恢复官方进程；提前失败只禁用自动恢复，不阻止官方入口。
        let mut cancellation = CancelCoordinator(receiver.recv().ok());
        let result = route();
        if result.is_ok() {
            cancellation.0 = None;
        }
        drop(cancellation);
        match worker.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("Windows update repair stopped: {error}"),
            Err(_) => eprintln!("Windows update repair coordinator panicked"),
        }
        result
    })
}

// 启动失败或路由展开时取消订阅；正常退出保留同一 helper 等待更新重绑。
struct CancelCoordinator(Option<Sender<CoordinatorEvent>>);

impl Drop for CancelCoordinator {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(CoordinatorEvent::Cancelled);
        }
    }
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
        let worker_finished = &finished;
        let (closed, owner_exit) = mpsc::channel();
        let result = run_installed_route_with(
            || {
                closed.send(()).unwrap();
                Ok(())
            },
            move |ready| {
                let (sender, receiver) = mpsc::channel();
                let _ = ready.send(sender);
                owner_exit.recv_timeout(Duration::from_secs(2)).unwrap();
                assert!(receiver.try_recv().is_err());
                worker_finished.store(true, Ordering::SeqCst);
                Ok(())
            },
        );
        assert_eq!(result, Ok(()));
        assert!(finished.load(Ordering::SeqCst));
    }
}
