/// Compute the child-relative x for our window so its right edge sits `gap`
/// pixels to the left of the nearest obstacle (the tray / embedded apps).
///
/// `taskbar_left` is the taskbar's screen left edge; `obstacle_lefts` are the
/// obstacles' screen left edges (same origin). `right_reserve` reserves that
/// many pixels at the taskbar's right edge, used for the Windows 11 secondary
/// taskbar clock, which is painted inside the full-width XAML host and so has no
/// obstacle window to detect. The result is in taskbar-client coordinates
/// (relative to `taskbar_left`) and clamped to `>= 0`. With no obstacles the
/// boundary is the taskbar's right edge minus the reserve (park far right).
pub fn compute_x(
    taskbar_left: i32,
    taskbar_width: i32,
    obstacle_lefts: &[i32],
    width: i32,
    gap: i32,
    right_reserve: i32,
) -> i32 {
    let cap = taskbar_left + taskbar_width - right_reserve;
    let boundary_screen = obstacle_lefts.iter().copied().min().unwrap_or(cap).min(cap);
    let boundary_client = boundary_screen - taskbar_left;
    (boundary_client - gap - width).max(0)
}

/// Whether a bar should be repositioned this tick. While the taskbar is hidden
/// (fullscreen game) nothing moves. A primary taskbar always has a detectable
/// tray, so an empty obstacle scan there is a transient state: repositioning
/// would take compute_x's park-far-right fallback and overlap the tray/clock.
/// A secondary has no obstacle window for its clock; the reserve-based
/// fallback is its normal path, so it repositions regardless of obstacles.
pub fn should_reposition(taskbar_visible: bool, primary: bool, has_obstacles: bool) -> bool {
    taskbar_visible && (!primary || has_obstacles)
}

/// Pack module widths left-to-right with no extra gaps (each width already
/// includes its own padding + margin). Returns each module's left offset within
/// the bar and the total bar width.
pub fn place_modules(widths: &[i32]) -> (Vec<i32>, i32) {
    let mut offsets = Vec::with_capacity(widths.len());
    let mut x = 0;
    for &w in widths {
        offsets.push(x);
        x += w;
    }
    (offsets, x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parks_at_far_right_when_no_obstacles() {
        // boundary = 0 + 1920; x = 1920 - 8 - 260
        assert_eq!(compute_x(0, 1920, &[], 260, 8, 0), 1652);
    }

    #[test]
    fn sits_left_of_single_obstacle() {
        // boundary = 1336; x = 1336 - 8 - 260
        assert_eq!(compute_x(0, 1920, &[1336], 260, 8, 0), 1068);
    }

    #[test]
    fn uses_leftmost_of_multiple_obstacles() {
        // min(1602, 1336) = 1336
        assert_eq!(compute_x(0, 1920, &[1602, 1336], 260, 8, 0), 1068);
    }

    #[test]
    fn clamps_to_zero_when_obstacle_too_far_left() {
        // 100 - 8 - 260 = -168 -> 0
        assert_eq!(compute_x(0, 1920, &[100], 260, 8, 0), 0);
    }

    #[test]
    fn handles_nonzero_taskbar_left() {
        // secondary monitor: taskbar_left = 1920, obstacle at 3256
        // boundary_client = 3256 - 1920 = 1336 -> 1068
        assert_eq!(compute_x(1920, 1920, &[3256], 260, 8, 0), 1068);
    }

    #[test]
    fn reserves_space_for_secondary_clock_when_no_obstacles() {
        // secondary taskbar left=1920 width=3440 (right edge 5360), no obstacles.
        // reserve 100 => cap = 5360 - 100 = 5260; client = 5260 - 1920 = 3340
        // x = 3340 - 8 - 185 = 3147
        assert_eq!(compute_x(1920, 3440, &[], 185, 8, 100), 3147);
    }

    #[test]
    fn obstacle_left_of_reserve_still_wins() {
        // an obstacle further left than the reserve cap takes precedence.
        // cap = 5360 - 100 = 5260; min(5000, 5260) = 5000; client = 5000 - 1920 = 3080
        // x = 3080 - 8 - 185 = 2887
        assert_eq!(compute_x(1920, 3440, &[5000], 185, 8, 100), 2887);
    }

    #[test]
    fn skips_reposition_while_taskbar_hidden() {
        // fullscreen game: explorer hides the taskbar, obstacle scan is empty
        assert!(!should_reposition(false, true, false));
        assert!(!should_reposition(false, false, false));
        // hidden taskbar wins even if obstacles are somehow still reported
        assert!(!should_reposition(false, true, true));
    }

    #[test]
    fn primary_with_no_obstacles_keeps_position() {
        // the primary always has a detectable tray; an empty scan means a
        // transient state, so parking far right would overlap the tray/clock
        assert!(!should_reposition(true, true, false));
    }

    #[test]
    fn primary_with_obstacles_repositions() {
        assert!(should_reposition(true, true, true));
    }

    #[test]
    fn secondary_repositions_with_or_without_obstacles() {
        // the secondary clock has no obstacle window; the reserve-based
        // fallback in compute_x is its normal path
        assert!(should_reposition(true, false, false));
        assert!(should_reposition(true, false, true));
    }

    #[test]
    fn places_modules_left_to_right() {
        let (offsets, total) = place_modules(&[100, 60, 80]);
        assert_eq!(offsets, vec![0, 100, 160]);
        assert_eq!(total, 240);
    }

    #[test]
    fn empty_bar_is_zero_width() {
        let (offsets, total) = place_modules(&[]);
        assert!(offsets.is_empty());
        assert_eq!(total, 0);
    }
}
