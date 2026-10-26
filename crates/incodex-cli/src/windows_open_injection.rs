//! [INPUT]: 依赖共享注入器、Job连接guard和既有目标/profile监督。
//! [OUTPUT]: 私密窗口Ready及拥有原新文档注册的监督worker。
//! [POS]: Windows open的注入适配；不创建额外服务或轮询器。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use super::*;
use crate::cdp::inject_windows_ui_session;

pub(super) fn inject_windows_ui(
    port: u16,
    options: InjectionOptions,
    alive: Arc<AtomicBool>,
    close_requested: Arc<AtomicBool>,
    cdp_failed: Arc<AtomicBool>,
    ownership_guard: Arc<WindowsCdpOwnershipGuard>,
    progress_tx: mpsc::Sender<WindowsInjectionProgress>,
) -> Result<WindowsMonitorWorkers, String> {
    inject_windows_ui_with_guard(
        port,
        options,
        alive,
        close_requested,
        cdp_failed,
        Arc::new(move |stream: &std::net::TcpStream| {
            ownership_guard.require_connection_owner(stream)
        }),
        progress_tx,
    )
}

pub(super) fn inject_windows_ui_with_guard<G>(
    port: u16,
    options: InjectionOptions,
    alive: Arc<AtomicBool>,
    close_requested: Arc<AtomicBool>,
    cdp_failed: Arc<AtomicBool>,
    ownership_guard: Arc<G>,
    progress_tx: mpsc::Sender<WindowsInjectionProgress>,
) -> Result<WindowsMonitorWorkers, String>
where
    G: Fn(&std::net::TcpStream) -> Result<(), String> + Send + Sync + 'static,
{
    let mut last_error = "Codex CDP page is not ready".to_string();
    let mut mode_readiness = CodexModeReadiness::default();
    let mut blocker_reported = false;
    while alive.load(Ordering::Acquire) {
        let mut primary_target = None;
        match inject_windows_ui_session(
            port,
            &options,
            &alive,
            |target_id| {
                primary_target = Some(target_id.to_string());
            },
            &mut mode_readiness,
            ownership_guard.as_ref(),
        ) {
            Ok(session) => {
                let mut monitor_workers = Vec::with_capacity(2);
                if let Some(target_id) = primary_target {
                    if options.profile_mask.is_some() {
                        let mask_ownership_guard = ownership_guard.clone();
                        monitor_workers.push(start_profile_mask_signal_monitor(
                            port,
                            alive.clone(),
                            cdp_failed.clone(),
                            move |stream| mask_ownership_guard(stream),
                        ));
                    }
                    monitor_workers.push(start_lifecycle_signal_monitor(
                        port,
                        target_id,
                        alive.clone(),
                        close_requested,
                        cdp_failed,
                        Some(session.into_registration()),
                    ));
                }
                return Ok(monitor_workers);
            }
            Err(error) if is_codex_mode_blocked_error(&error) => {
                if !blocker_reported {
                    let _ = progress_tx.send(WindowsInjectionProgress::BlockedByOfficialUi);
                    blocker_reported = true;
                }
                thread::sleep(CODEX_MODE_POLL_INTERVAL);
                continue;
            }
            Err(error) if is_codex_mode_waiting_error(&error) => {
                thread::sleep(CODEX_MODE_POLL_INTERVAL);
                continue;
            }
            Err(error) if is_terminal_codex_mode_error(&error) => return Err(error),
            Err(error) => last_error = error,
        }
        thread::sleep(Duration::from_millis(400));
    }
    Err(format!("Windows UI injection failed: {last_error}"))
}
