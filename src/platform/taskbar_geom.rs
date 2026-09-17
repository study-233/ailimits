// platform/taskbar_geom.rs — placement decisions for the taskbar panel.
//
// Deliberately free of Windows API calls: win.rs gathers handles and
// rectangles, this module decides what they mean. That split is what makes
// the rules testable at all — the shell windows they describe cannot be
// created in a test.

/// Width to keep clear on the right when the notification area cannot be
/// found. Secondary Win11 taskbars have no `TrayNotifyWnd` at all, yet still
/// show a clock; without a reserve the panel is drawn straight over it.
/// 88 DIP is the figure TrafficMonitor uses for the same case.
pub const RIGHT_RESERVE_DIP: f32 = 88.0;

/// Largest manual nudge accepted from the config, in either direction.
pub const MAX_OFFSET: i32 = 200;

/// The taskbar's own scale factor, derived from its height rather than from a
/// DPI query: the bar is the thing we have to match, and a 100% bottom bar is
/// 48px. Clamped so a bogus height cannot explode every derived measurement.
pub fn bar_scale(bar_height: i32) -> f32 {
    if bar_height <= 0 {
        return 1.0;
    }
    (bar_height as f32 / 48.0).clamp(1.0, 3.0)
}

/// Where to pretend the notification area starts when it does not exist.
pub fn estimated_tray_left(bar_right: i32, bar_height: i32) -> i32 {
    bar_right - (RIGHT_RESERVE_DIP * bar_scale(bar_height)).round() as i32
}

/// Keep a hand-edited offset from throwing the panel off the desktop.
pub fn clamp_offset(v: i32) -> i32 {
    v.clamp(-MAX_OFFSET, MAX_OFFSET)
}

/// Manual positions may overlap shell buttons. Automatic placement retains
/// the last position when geometry is missing or every gap is occupied.
#[allow(clippy::too_many_arguments)]
pub fn panel_x(
    left: i32,
    right: i32,
    width: i32,
    scale: f32,
    manual: Option<i32>,
    last: Option<i32>,
    safe: Option<i32>,
    preferred: i32,
) -> i32 {
    let proposed = manual
        .map(|dip| left.saturating_add((dip as f32 * scale) as i32))
        .or(safe)
        .or_else(|| last.map(|dip| left.saturating_add((dip as f32 * scale) as i32)))
        .unwrap_or(preferred);
    proposed.clamp(left, right.saturating_sub(width).max(left))
}

/// Choose a verified empty horizontal interval. Occupied intervals include
/// shell buttons and the notification area. Prefer the nearest gap to the
/// tray; a large left gap is the second choice on centered taskbars.
pub fn free_panel_x(
    left: i32,
    right: i32,
    width: i32,
    preferred: i32,
    gap: i32,
    occupied: &[(i32, i32)],
) -> Option<i32> {
    if width <= 0 || right - left < width || occupied.is_empty() {
        return None;
    }
    let mut blocked: Vec<_> = occupied
        .iter()
        .map(|&(a, b)| ((a - gap).max(left), (b + gap).min(right)))
        .filter(|&(a, b)| b > a)
        .collect();
    blocked.sort_unstable();
    let mut cursor = left + gap;
    let mut candidates = Vec::new();
    for (a, b) in blocked
        .into_iter()
        .chain(std::iter::once((right - gap, right)))
    {
        if a - cursor >= width {
            candidates.push(preferred.clamp(cursor, a - width));
        }
        cursor = cursor.max(b);
    }
    candidates
        .into_iter()
        .min_by_key(|x| (*x as i64 - preferred as i64).abs())
}

#[cfg(test)]
mod placement_tests {
    use super::*;

    #[test]
    fn manual_position_ignores_obstacles_and_survives_dpi_changes() {
        for scale in [1.0, 1.5, 2.0] {
            assert_eq!(
                panel_x(
                    0,
                    (1600.0 * scale) as i32,
                    (151.0 * scale) as i32,
                    scale,
                    Some(500),
                    Some(50),
                    Some(30),
                    20
                ),
                (500.0 * scale) as i32
            );
        }
        assert_eq!(
            panel_x(-1600, 0, 151, 1.0, Some(500), None, None, -200),
            -1100
        );
        assert_eq!(
            panel_x(0, 800, 151, 1.0, Some(i32::MAX), None, None, 0),
            649
        );
    }

    #[test]
    fn no_space_or_failed_detection_keeps_last_position_and_size() {
        assert_eq!(panel_x(0, 1600, 151, 1.0, None, Some(450), None, 1200), 450);
        assert_eq!(panel_x(0, 1600, 151, 1.0, None, None, None, 1200), 1200);
        assert_eq!(
            panel_x(0, 1600, 151, 1.0, None, Some(450), Some(1000), 1200),
            1000
        );
    }
    #[test]
    fn crowded_right_side_moves_left_and_never_overlaps() {
        for scale in [1.0, 1.5, 2.0] {
            let s = |x: f32| (x * scale) as i32;
            let occupied = [(s(446.0), s(1107.0)), (s(1164.5), s(1600.0))];
            let x = free_panel_x(0, s(1600.0), s(151.0), s(1003.5), s(6.0), &occupied).unwrap();
            assert!(x + s(151.0) <= s(440.0));
            assert!(occupied.iter().all(|&(a, b)| x + s(151.0) <= a || x >= b));
        }
    }
    #[test]
    fn roomy_right_gap_is_preferred_and_unknown_or_full_bar_falls_back() {
        assert_eq!(
            free_panel_x(0, 1600, 151, 1250, 6, &[(400, 900), (1420, 1600)]),
            Some(1250)
        );
        assert_eq!(
            free_panel_x(0, 1600, 151, 1250, 6, &[(0, 1400), (1420, 1600)]),
            None
        );
        assert_eq!(free_panel_x(0, 1600, 151, 1250, 6, &[]), None);
        // Negative monitor coordinates and a manually nudged preferred point.
        assert_eq!(
            free_panel_x(-1600, 0, 151, 300, 6, &[(-1000, -500), (-200, 0)]),
            Some(-357)
        );
    }
}

/// Put taskbars in a stable, human-meaningful order: left to right by the
/// monitor they sit on. The shell hands them over in whatever order it
/// happens to enumerate, which would make a saved display index point
/// somewhere else after a reboot.
pub fn order_bars(bars: &mut [(isize, i32)]) {
    bars.sort_by_key(|&(hwnd, left)| (left, hwnd));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reserve has to grow with the bar, or it under-reserves on a
    /// high-DPI taskbar and the panel lands on the clock anyway.
    #[test]
    fn the_estimated_tray_edge_scales_with_the_bar() {
        let at_100 = estimated_tray_left(3440, 48);
        let at_200 = estimated_tray_left(3440, 96);
        assert_eq!(at_100, 3440 - 88);
        assert!(
            3440 - at_200 > 3440 - at_100,
            "a taller bar must reserve more: 100% left {at_100}, 200% left {at_200}"
        );
    }

    /// A stray bar height must not produce a reserve wider than the screen.
    #[test]
    fn the_scale_is_bounded_at_both_ends() {
        assert_eq!(bar_scale(0), 1.0, "a zero-height bar falls back to 1x");
        assert_eq!(bar_scale(480), 3.0, "an absurd bar height is capped");
    }

    #[test]
    fn offsets_are_clamped_to_a_sane_window() {
        assert_eq!(clamp_offset(0), 0);
        assert_eq!(clamp_offset(50), 50);
        assert_eq!(clamp_offset(9999), MAX_OFFSET);
        assert_eq!(clamp_offset(-9999), -MAX_OFFSET);
    }

    /// Index-addressed displays are only meaningful if the order is stable.
    /// Enumeration order from the shell is arbitrary, so sort by geometry.
    #[test]
    fn bars_are_ordered_left_to_right_and_ties_are_broken() {
        let mut bars = vec![(0xBB, 3440), (0xAA, 0), (0xCC, 3440)];
        order_bars(&mut bars);
        assert_eq!(
            bars,
            vec![(0xAA, 0), (0xBB, 3440), (0xCC, 3440)],
            "left edge first, handle as the tie-break"
        );
    }
}
