// 登录观察者的事件顺序；恢复执行仍属于既有安装事务。
fn run_observer_with<S, R, W, T>(subscribe: S, mut reconcile: R, mut wait: W) -> Result<(), String>
where
    S: FnOnce() -> Result<T, String>,
    R: FnMut() -> Result<bool, String>,
    W: FnMut() -> Result<bool, String>,
{
    let _subscription = subscribe()?;
    // 复现旧模型：只响应启动之后的事件，漏掉已经完成的换包。
    while wait()? {
        if !reconcile()? {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn observer_subscribes_before_startup_reconciliation_without_a_new_event() {
        let calls = RefCell::new(Vec::new());
        run_observer_with(
            || {
                calls.borrow_mut().push("subscribe");
                Ok(())
            },
            || {
                calls.borrow_mut().push("reconcile");
                Ok(true)
            },
            || {
                calls.borrow_mut().push("stop");
                Ok(false)
            },
        )
        .unwrap();
        assert_eq!(*calls.borrow(), ["subscribe", "reconcile", "stop"]);
    }

    #[test]
    fn observer_reconciles_every_generation_and_stops_without_authorization() {
        let mut reconciliations = 0;
        let mut wakeups = 0;
        run_observer_with(
            || Ok(()),
            || {
                reconciliations += 1;
                Ok(reconciliations < 3)
            },
            || {
                wakeups += 1;
                Ok(true)
            },
        )
        .unwrap();
        assert_eq!(reconciliations, 3);
        assert_eq!(
            wakeups, 2,
            "startup reconciliation must not require an event"
        );
    }
}
