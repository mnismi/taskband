/// Compute the child-relative x for our window so its right edge sits `gap`
/// pixels to the left of the nearest obstacle (the tray / embedded apps).
///
/// `taskbar_left` is the taskbar's screen left edge; `obstacle_lefts` are the
/// obstacles' screen left edges (same origin). The result is in taskbar-client
/// coordinates (relative to `taskbar_left`) and clamped to `>= 0`. With no
/// obstacles the boundary is the taskbar's right edge (park far right).
pub fn compute_x(
    taskbar_left: i32,
    taskbar_width: i32,
    obstacle_lefts: &[i32],
    width: i32,
    gap: i32,
) -> i32 {
    let boundary_screen = obstacle_lefts
        .iter()
        .copied()
        .min()
        .unwrap_or(taskbar_left + taskbar_width);
    let boundary_client = boundary_screen - taskbar_left;
    (boundary_client - gap - width).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parks_at_far_right_when_no_obstacles() {
        // boundary = 0 + 1920; x = 1920 - 8 - 260
        assert_eq!(compute_x(0, 1920, &[], 260, 8), 1652);
    }

    #[test]
    fn sits_left_of_single_obstacle() {
        // boundary = 1336; x = 1336 - 8 - 260
        assert_eq!(compute_x(0, 1920, &[1336], 260, 8), 1068);
    }

    #[test]
    fn uses_leftmost_of_multiple_obstacles() {
        // min(1602, 1336) = 1336
        assert_eq!(compute_x(0, 1920, &[1602, 1336], 260, 8), 1068);
    }

    #[test]
    fn clamps_to_zero_when_obstacle_too_far_left() {
        // 100 - 8 - 260 = -168 -> 0
        assert_eq!(compute_x(0, 1920, &[100], 260, 8), 0);
    }

    #[test]
    fn handles_nonzero_taskbar_left() {
        // secondary monitor: taskbar_left = 1920, obstacle at 3256
        // boundary_client = 3256 - 1920 = 1336 -> 1068
        assert_eq!(compute_x(1920, 1920, &[3256], 260, 8), 1068);
    }
}
