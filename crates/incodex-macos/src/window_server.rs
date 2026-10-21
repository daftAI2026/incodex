//! [INPUT]: 依赖 CoreGraphics 候选快照与 WindowServer 按窗口 ID 返回的元数据。
//! [OUTPUT]: 区分固定窗口成功空查询与失败；空查询不单独证明关闭。
//! [POS]: macOS窗口身份决策层；CLI须再确认CDP无primary目标才可计Missing。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use crate::live_window::WindowBounds;
use crate::open_window::OpenWindowObservation;

// SkyLight 私有标签位；缺失这些实测位时不把未知状态猜成关闭。
pub(crate) const MINIMIZED_TAG: u64 = 1 << 60;
pub(crate) const HIDDEN_TAG: u64 = 1 << 39;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowCandidate {
    pub pid: i32,
    pub window_id: Option<u32>,
    pub layer: i32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WindowListSnapshot {
    pub complete: bool,
    pub windows: Vec<WindowCandidate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowServerRow {
    pub pid: i32,
    pub window_id: u32,
    pub tags: u64,
    pub ordered_in: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PinnedWindow {
    pub pid: i32,
    pub window_id: u32,
}

/// 只选择与 CDP 主窗口边界完全一致的唯一正常窗。
pub(crate) fn unique_initial_candidate(
    pid: i32,
    primary_bounds: Option<WindowBounds>,
    candidates: &WindowListSnapshot,
) -> Option<PinnedWindow> {
    let primary_bounds = primary_bounds?;
    if pid <= 0 || !candidates.complete {
        return None;
    }
    let mut owned = candidates.windows.iter().filter(|candidate| {
        candidate.pid == pid && candidate.layer == 0 && candidate.width > 0 && candidate.height > 0
    });
    let candidate = owned.next()?;
    if owned.next().is_some() {
        return None;
    }
    if candidate.x != primary_bounds.x
        || candidate.y != primary_bounds.y
        || candidate.width != primary_bounds.width
        || candidate.height != primary_bounds.height
    {
        return None;
    }
    Some(PinnedWindow {
        pid,
        window_id: candidate.window_id.filter(|window_id| *window_id != 0)?,
    })
}

/// 只将 WindowServer 的同身份、ordered-in 正控结果固定为主窗口。
pub(crate) fn pin_initial_window(
    candidate: PinnedWindow,
    rows: Result<&[WindowServerRow], ()>,
) -> Option<PinnedWindow> {
    if candidate.pid <= 0 || candidate.window_id == 0 {
        return None;
    }
    let [row] = rows.ok()? else {
        return None;
    };
    (row.pid == candidate.pid
        && row.window_id == candidate.window_id
        && row.tags & (MINIMIZED_TAG | HIDDEN_TAG) == 0
        && row.ordered_in == Some(true))
    .then_some(candidate)
}

/// 将固定窗口的成功空查询与明确 ordered-out 分开交给关闭策略确认。
pub(crate) fn classify_pinned_window(
    pinned: PinnedWindow,
    candidates: &WindowListSnapshot,
    rows: Result<&[WindowServerRow], ()>,
) -> OpenWindowObservation {
    if pinned.pid <= 0
        || pinned.window_id == 0
        || !pinned_candidates_are_unambiguous(pinned, candidates)
    {
        return OpenWindowObservation::Unknown;
    }
    let rows = match rows {
        Ok([]) => {
            if candidates
                .windows
                .iter()
                .any(|candidate| candidate.window_id == Some(pinned.window_id))
            {
                return OpenWindowObservation::Unknown;
            }
            return OpenWindowObservation::PinnedWindowRecordAbsent;
        }
        Ok(rows) if rows.len() == 1 => rows,
        Ok(_) | Err(()) => return OpenWindowObservation::Unknown,
    };
    let row = &rows[0];
    if row.pid != pinned.pid || row.window_id != pinned.window_id {
        return OpenWindowObservation::Unknown;
    }
    if row.tags & HIDDEN_TAG != 0 {
        return OpenWindowObservation::Unknown;
    }
    if row.tags & MINIMIZED_TAG != 0 {
        return OpenWindowObservation::Minimized;
    }
    match row.ordered_in {
        Some(true) => OpenWindowObservation::Present,
        Some(false) => OpenWindowObservation::Missing,
        None => OpenWindowObservation::Unknown,
    }
}

fn pinned_candidates_are_unambiguous(
    pinned: PinnedWindow,
    candidates: &WindowListSnapshot,
) -> bool {
    if !candidates.complete {
        return false;
    }
    let mut target_count = 0;
    for candidate in candidates.windows.iter().filter(|candidate| {
        candidate.pid == pinned.pid
            && candidate.layer == 0
            && candidate.width > 0
            && candidate.height > 0
    }) {
        if candidate.window_id != Some(pinned.window_id) {
            return false;
        }
        target_count += 1;
    }
    target_count <= 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live_window::WindowBounds;

    const PID: i32 = 42180;
    const OTHER_PID: i32 = 42181;
    const WINDOW_ID: u32 = 77;
    const OTHER_WINDOW_ID: u32 = 78;
    const DEFAULT_BOUNDS: WindowBounds = WindowBounds {
        x: 0,
        y: 0,
        width: 800,
        height: 600,
    };
    const PINNED: PinnedWindow = PinnedWindow {
        pid: PID,
        window_id: WINDOW_ID,
    };

    fn candidate(pid: i32, window_id: u32) -> WindowCandidate {
        WindowCandidate {
            pid,
            window_id: Some(window_id),
            layer: 0,
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        }
    }

    fn candidate_at(pid: i32, window_id: u32, bounds: WindowBounds) -> WindowCandidate {
        WindowCandidate {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
            ..candidate(pid, window_id)
        }
    }

    fn complete(candidates: Vec<WindowCandidate>) -> WindowListSnapshot {
        WindowListSnapshot {
            complete: true,
            windows: candidates,
        }
    }

    fn row(pid: i32, window_id: u32, tags: u64, ordered_in: bool) -> WindowServerRow {
        WindowServerRow {
            pid,
            window_id,
            tags,
            ordered_in: Some(ordered_in),
        }
    }

    #[test]
    fn initial_pin_requires_one_positive_candidate_and_server_positive_control() {
        let initial = complete(vec![candidate(PID, WINDOW_ID)]);
        let present_row = [row(PID, WINDOW_ID, 0, true)];

        assert_eq!(
            pin_initial_window(
                unique_initial_candidate(PID, Some(DEFAULT_BOUNDS), &initial).unwrap(),
                Ok(&present_row)
            ),
            Some(PINNED)
        );
        assert_eq!(
            unique_initial_candidate(PID, Some(DEFAULT_BOUNDS), &complete(Vec::new())),
            None
        );
        assert_eq!(
            unique_initial_candidate(
                PID,
                Some(DEFAULT_BOUNDS),
                &complete(vec![
                    candidate(PID, WINDOW_ID),
                    candidate(PID, OTHER_WINDOW_ID)
                ])
            ),
            None
        );
        assert_eq!(
            unique_initial_candidate(
                PID,
                Some(DEFAULT_BOUNDS),
                &WindowListSnapshot {
                    complete: false,
                    windows: vec![candidate(PID, WINDOW_ID)]
                }
            ),
            None
        );
        assert_eq!(
            unique_initial_candidate(
                PID,
                Some(DEFAULT_BOUNDS),
                &complete(vec![WindowCandidate {
                    window_id: None,
                    ..candidate(PID, WINDOW_ID)
                }])
            ),
            None
        );
        assert_eq!(
            unique_initial_candidate(
                PID,
                Some(DEFAULT_BOUNDS),
                &complete(vec![WindowCandidate {
                    window_id: Some(0),
                    ..candidate(PID, WINDOW_ID)
                }])
            ),
            None
        );
        assert_eq!(
            unique_initial_candidate(
                0,
                Some(DEFAULT_BOUNDS),
                &complete(vec![candidate(PID, WINDOW_ID)])
            ),
            None
        );
        for rows in [
            Err(()),
            Ok(&[][..]),
            Ok(&[row(PID, WINDOW_ID, 0, false)][..]),
            Ok(&[row(PID, OTHER_WINDOW_ID, 0, true)][..]),
            Ok(&[row(OTHER_PID, WINDOW_ID, 0, true)][..]),
            Ok(&[row(PID, WINDOW_ID, MINIMIZED_TAG, true)][..]),
            Ok(&[row(PID, WINDOW_ID, HIDDEN_TAG, true)][..]),
            Ok(&[WindowServerRow {
                ordered_in: None,
                ..row(PID, WINDOW_ID, 0, true)
            }][..]),
            Ok(&[row(PID, WINDOW_ID, 0, true), row(PID, WINDOW_ID, 0, true)][..]),
        ] {
            assert_eq!(pin_initial_window(PINNED, rows), None);
        }
    }

    #[test]
    fn initial_window_identity_requires_exact_primary_frame_match() {
        let primary = WindowBounds {
            x: 120,
            y: 80,
            width: 960,
            height: 720,
        };
        let initial = complete(vec![candidate_at(PID, WINDOW_ID, primary)]);

        assert_eq!(
            unique_initial_candidate(PID, Some(primary), &initial),
            Some(PINNED)
        );
        for mismatched in [
            WindowBounds {
                x: primary.x + 1,
                ..primary
            },
            WindowBounds {
                y: primary.y + 1,
                ..primary
            },
            WindowBounds {
                width: primary.width + 1,
                ..primary
            },
            WindowBounds {
                height: primary.height + 1,
                ..primary
            },
        ] {
            assert_eq!(
                unique_initial_candidate(PID, Some(mismatched), &initial),
                None,
                "every primary frame coordinate and dimension must match exactly"
            );
        }
        assert_eq!(
            unique_initial_candidate(PID, None, &initial),
            None,
            "the observer must not guess when CDP has no verified primary frame"
        );
    }

    #[test]
    fn initial_window_identity_stays_unknown_when_normal_windows_are_ambiguous() {
        let primary = WindowBounds {
            x: 120,
            y: 80,
            width: 960,
            height: 720,
        };
        let auxiliary = WindowBounds {
            x: 0,
            y: 0,
            width: 320,
            height: 240,
        };
        let candidates = complete(vec![
            candidate_at(PID, WINDOW_ID, primary),
            candidate_at(PID, OTHER_WINDOW_ID, auxiliary),
        ]);

        assert_eq!(
            unique_initial_candidate(PID, Some(primary), &candidates),
            None
        );
        assert_eq!(
            unique_initial_candidate(PID, None, &complete(Vec::new())),
            None
        );
    }

    #[test]
    fn pinned_identity_survives_frame_changes() {
        let moved_or_resized = complete(vec![candidate_at(
            PID,
            WINDOW_ID,
            WindowBounds {
                x: 300,
                y: 210,
                width: 1100,
                height: 760,
            },
        )]);

        assert_eq!(
            classify_pinned_window(
                PINNED,
                &moved_or_resized,
                Ok(&[row(PID, WINDOW_ID, 0, true)])
            ),
            OpenWindowObservation::Present
        );
    }

    #[test]
    fn small_positive_window_is_eligible_without_geometry_thresholds() {
        let small = WindowCandidate {
            width: 40,
            height: 20,
            ..candidate(PID, WINDOW_ID)
        };
        let snapshot = complete(vec![small]);
        let selected = unique_initial_candidate(
            PID,
            Some(WindowBounds {
                x: 0,
                y: 0,
                width: 40,
                height: 20,
            }),
            &snapshot,
        )
        .unwrap();
        assert_eq!(
            pin_initial_window(selected, Ok(&[row(PID, WINDOW_ID, 0, true)])),
            Some(PINNED)
        );
    }

    #[test]
    fn pinned_window_states_require_one_exact_server_row() {
        let candidates = complete(vec![candidate(PID, WINDOW_ID)]);

        assert_eq!(
            classify_pinned_window(PINNED, &candidates, Ok(&[row(PID, WINDOW_ID, 0, true)])),
            OpenWindowObservation::Present
        );
        assert_eq!(
            classify_pinned_window(
                PINNED,
                &complete(Vec::new()),
                Ok(&[row(PID, WINDOW_ID, MINIMIZED_TAG, false)])
            ),
            OpenWindowObservation::Minimized
        );
        assert_eq!(
            classify_pinned_window(
                PINNED,
                &complete(Vec::new()),
                Ok(&[row(PID, WINDOW_ID, HIDDEN_TAG, false)])
            ),
            OpenWindowObservation::Unknown
        );
        assert_eq!(
            classify_pinned_window(
                PINNED,
                &complete(Vec::new()),
                Ok(&[row(PID, WINDOW_ID, MINIMIZED_TAG | HIDDEN_TAG, false)])
            ),
            OpenWindowObservation::Unknown
        );
        assert_eq!(
            classify_pinned_window(
                PINNED,
                &complete(Vec::new()),
                Ok(&[row(PID, WINDOW_ID, 0, false)])
            ),
            OpenWindowObservation::Missing
        );
    }

    #[test]
    fn successful_empty_pinned_query_has_distinct_absence_evidence() {
        let candidates = complete(Vec::new());

        assert_eq!(
            classify_pinned_window(PINNED, &candidates, Ok(&[])),
            OpenWindowObservation::PinnedWindowRecordAbsent
        );
    }

    #[test]
    fn empty_server_query_conflicts_with_any_coregraphics_window_id_match() {
        for pid in [PID, OTHER_PID] {
            let candidates = complete(vec![candidate(pid, WINDOW_ID)]);
            assert_eq!(
                classify_pinned_window(PINNED, &candidates, Ok(&[])),
                OpenWindowObservation::Unknown,
                "WindowServer absence conflicts with CoreGraphics PID {pid}"
            );
        }
    }

    #[test]
    fn pinned_window_refuses_bad_snapshots_and_ambiguous_candidates() {
        let bad_candidates = WindowListSnapshot {
            complete: false,
            windows: vec![],
        };
        let empty_candidates = complete(Vec::new());
        let two_candidates = complete(vec![
            candidate(PID, WINDOW_ID),
            candidate(PID, OTHER_WINDOW_ID),
        ]);
        let wrong_candidate = complete(vec![candidate(PID, OTHER_WINDOW_ID)]);
        let bad = [
            (&bad_candidates, Ok(&[row(PID, WINDOW_ID, 0, true)][..])),
            (&empty_candidates, Err(())),
            (
                &empty_candidates,
                Ok(&[row(PID, WINDOW_ID, 0, true), row(PID, WINDOW_ID, 0, true)][..]),
            ),
            (
                &empty_candidates,
                Ok(&[row(OTHER_PID, WINDOW_ID, 0, true)][..]),
            ),
            (
                &empty_candidates,
                Ok(&[row(PID, OTHER_WINDOW_ID, 0, true)][..]),
            ),
            (
                &empty_candidates,
                Ok(&[WindowServerRow {
                    ordered_in: None,
                    ..row(PID, WINDOW_ID, 0, false)
                }][..]),
            ),
            (&two_candidates, Ok(&[row(PID, WINDOW_ID, 0, true)][..])),
            (&wrong_candidate, Ok(&[row(PID, WINDOW_ID, 0, true)][..])),
        ];

        for (candidates, rows) in bad {
            assert_eq!(
                classify_pinned_window(PINNED, candidates, rows),
                OpenWindowObservation::Unknown
            );
        }
    }

    #[test]
    fn other_layers_zero_size_and_foreign_windows_do_not_create_ambiguity() {
        let candidates = complete(vec![
            candidate(PID, WINDOW_ID),
            WindowCandidate {
                layer: 1,
                ..candidate(PID, OTHER_WINDOW_ID)
            },
            WindowCandidate {
                width: 0,
                ..candidate(PID, OTHER_WINDOW_ID)
            },
            candidate(OTHER_PID, OTHER_WINDOW_ID),
        ]);
        assert_eq!(
            classify_pinned_window(PINNED, &candidates, Ok(&[row(PID, WINDOW_ID, 0, true)])),
            OpenWindowObservation::Present
        );
    }
}
