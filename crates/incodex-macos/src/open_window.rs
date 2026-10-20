/*
 * [INPUT]: 依赖 CDP 主窗边界、CoreGraphics 精确候选与 WindowServer 按 ID 返回的只读元数据。
 * [OUTPUT]: 提供 OpenWindowObservation 枚举；新增固定窗口成功空查询证据值。
 * [POS]: open 生命周期窗口监视器；缺能力或歧义时保留 session，不请求授权。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

use crate::live_window::WindowBounds;
#[cfg(target_os = "macos")]
use crate::window_server::{
    classify_pinned_window, pin_initial_window, unique_initial_candidate, PinnedWindow,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenWindowObservation {
    Present,
    Minimized,
    Missing,
    PinnedWindowRecordAbsent,
    Unknown,
}

pub struct OpenWindowObserver {
    #[cfg(target_os = "macos")]
    window_list: crate::live_window_macos::WindowListApi,
    #[cfg(target_os = "macos")]
    window_server: crate::window_server_macos::WindowServerApi,
    #[cfg(target_os = "macos")]
    pinned: Option<PinnedWindow>,
}

impl OpenWindowObserver {
    /// 加载只读窗口 API 一次。缺少任一私有 WindowServer 符号时明确失败。
    pub fn new() -> Result<Self, String> {
        #[cfg(target_os = "macos")]
        {
            Ok(Self {
                window_list: crate::live_window_macos::WindowListApi::load_for_observer()?,
                window_server: crate::window_server_macos::WindowServerApi::load()?,
                pinned: None,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err("native window observation is only available on macOS".into())
        }
    }

    /// 只观察指定进程；首次固定窗口还要求 CDP 主窗边界精确匹配。
    pub fn observe(
        &mut self,
        pid: i32,
        primary_bounds: Option<WindowBounds>,
    ) -> OpenWindowObservation {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (pid, primary_bounds);
            OpenWindowObservation::Unknown
        }
        #[cfg(target_os = "macos")]
        {
            self.observe_macos(pid, primary_bounds)
        }
    }

    #[cfg(target_os = "macos")]
    fn observe_macos(
        &mut self,
        pid: i32,
        primary_bounds: Option<WindowBounds>,
    ) -> OpenWindowObservation {
        let Ok(candidates) = self.window_list.observer_snapshot() else {
            return OpenWindowObservation::Unknown;
        };
        if let Some(pinned) = self.pinned {
            if pinned.pid != pid {
                return OpenWindowObservation::Unknown;
            }
            let Ok(rows) = self.window_server.query_window(pinned.window_id) else {
                return OpenWindowObservation::Unknown;
            };
            return classify_pinned_window(pinned, &candidates, Ok(&rows));
        }
        let Some(candidate) = unique_initial_candidate(pid, primary_bounds, &candidates) else {
            return OpenWindowObservation::Unknown;
        };
        let Ok(rows) = self.window_server.query_window(candidate.window_id) else {
            return OpenWindowObservation::Unknown;
        };
        let Some(pinned) = pin_initial_window(candidate, Ok(&rows)) else {
            return OpenWindowObservation::Unknown;
        };
        self.pinned = Some(pinned);
        OpenWindowObservation::Present
    }
}
