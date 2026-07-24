# Dynamic Positioning Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the embedded `vEnter` window continuously reposition itself just left of TrafficMonitor (and the tray), following taskbar changes, instead of sitting at a fixed x.

**Architecture:** Extract the position math into a pure, unit-tested `layout::compute_x`. Add a 250 ms `WM_TIMER` to the window whose handler gathers the tray/embedded-app rects, calls `compute_x`, and repositions only when the target changed.

**Tech Stack:** Rust, `windows` crate 0.58, Win32 (`SetTimer`, child enumeration, `SetWindowPos`).

## Global Constraints

- **windows-rs version:** `windows = "0.58"` (already pinned). If any call fails to compile, run `cargo build` and apply the compiler's suggested mechanical fix (handle/Option wrapping) — do not restructure logic.
- **Binary name:** `vEnter.exe` (already configured).
- **Console subsystem:** keep default (diagnostic `println!` visible).
- **Existing code:** builds on `src/window.rs` from the spike. `create_window` already produces a layered window (`WS_EX_LAYERED` + `SetLayeredWindowAttributes`); do not change that.
- **Testing reality:** `compute_x` is pure and fully unit-tested. The Win32 glue (timer, enumeration, `SetWindowPos`) is verified visually + by screen capture, as in the spike.

---

### Task 1: Pure `compute_x` positioning function

**Files:**
- Create: `src/layout.rs`
- Modify: `src/main.rs` (add `mod layout;`)

**Interfaces:**
- Produces: `layout::compute_x(taskbar_left: i32, taskbar_width: i32, obstacle_lefts: &[i32], width: i32, gap: i32) -> i32` — returns the taskbar-client x so our window's right edge sits `gap` px left of the leftmost obstacle; clamped `>= 0`; parks at the far right when there are no obstacles.

- [ ] **Step 1: Write `src/layout.rs` with failing tests**

```rust
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
    todo!("implemented in step 3")
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
```

- [ ] **Step 2: Add the module declaration and verify the tests fail**

In `src/main.rs`, add `mod layout;` next to the other `mod` lines:

```rust
mod layout;
mod taskbar;
mod window;
```

Run: `cargo test --lib layout 2>&1 || cargo test layout`
(Use `cargo test compute` if the name filter differs.) Simpler: `cargo test parks_at_far_right_when_no_obstacles`
Expected: FAIL — panics at `todo!("implemented in step 3")`.

- [ ] **Step 3: Implement `compute_x`**

Replace the `todo!` body in `src/layout.rs`:

```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test layout`
Expected: PASS — 5 tests in `layout::tests` pass.

- [ ] **Step 5: Commit**

```bash
git add src/layout.rs src/main.rs
git commit -m "feat: add pure compute_x positioning function"
```

---

### Task 2: Timer-driven repositioning

**Files:**
- Modify: `src/window.rs`

**Interfaces:**
- Consumes: `layout::compute_x` (Task 1).
- Produces: a private `reposition(hwnd: HWND)` and a `WM_TIMER` handler; `embed_in_taskbar` now does initial placement via `reposition` and starts a repeating timer instead of using a fixed x.

- [ ] **Step 1: Update the `WindowsAndMessaging` import block in `src/window.rs`**

Replace the existing `use windows::Win32::UI::WindowsAndMessaging::{...}` block with:

```rust
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW, GetParent,
    GetWindow, GetWindowLongPtrW, GetWindowRect, IsWindowVisible, KillTimer, PostQuitMessage,
    RegisterClassW, SetLayeredWindowAttributes, SetParent, SetTimer, SetWindowLongPtrW,
    SetWindowPos, TranslateMessage, GWL_STYLE, GW_CHILD, GW_HWNDNEXT, HWND_TOP, LWA_ALPHA, MSG,
    SWP_SHOWWINDOW, WINDOW_STYLE, WM_DESTROY, WM_PAINT, WM_TIMER, WNDCLASSW, WS_CHILD,
    WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_POPUP, WS_VISIBLE,
};
```

(This drops the now-unused `SWP_FRAMECHANGED` and `SWP_NOZORDER`, and adds `GetParent`, `GetWindow`, `GW_CHILD`, `GW_HWNDNEXT`, `HWND_TOP`, `IsWindowVisible`, `KillTimer`, `SetTimer`, `WM_TIMER`.)

- [ ] **Step 2: Add layout constants near the top of `src/window.rs`**

Add just below the `use` lines:

```rust
const TIMER_ID: usize = 1;
const TIMER_MS: u32 = 250;
const WIDTH: i32 = 260;
const GAP: i32 = 8;
```

- [ ] **Step 3: Replace the body of `embed_in_taskbar`**

Replace the whole `embed_in_taskbar` function with:

```rust
/// Reparent `child` into the taskbar, make it a child window, place it, and
/// start a timer that keeps it parked just left of the tray / embedded apps.
pub fn embed_in_taskbar(child: HWND, taskbar: HWND) -> Result<()> {
    unsafe {
        SetParent(child, taskbar)?;

        let current = WINDOW_STYLE(GetWindowLongPtrW(child, GWL_STYLE) as u32);
        let child_style = (current & !WS_POPUP) | WS_CHILD | WS_VISIBLE;
        SetWindowLongPtrW(child, GWL_STYLE, child_style.0 as isize);

        reposition(child); // initial placement
        SetTimer(child, TIMER_ID, TIMER_MS, None); // keep tracking taskbar changes
        Ok(())
    }
}

/// Recompute where the window should sit (just left of the tray / embedded
/// apps) and move it there, but only if the target changed (avoids flicker).
fn reposition(hwnd: HWND) {
    unsafe {
        let Ok(taskbar) = GetParent(hwnd) else {
            return;
        };
        let mut tb = RECT::default();
        if GetWindowRect(taskbar, &mut tb).is_err() {
            return;
        }
        let taskbar_left = tb.left;
        let taskbar_width = tb.right - tb.left;
        let tb_height = tb.bottom - tb.top;

        // Obstacle = a visible sibling in the right half that is not full-width
        // (excludes the full-width XAML content bridge) and not our own window.
        let mut obstacles: Vec<i32> = Vec::new();
        let mut sib = GetWindow(taskbar, GW_CHILD).ok();
        while let Some(h) = sib {
            if h != hwnd && IsWindowVisible(h).as_bool() {
                let mut r = RECT::default();
                if GetWindowRect(h, &mut r).is_ok() {
                    let w = r.right - r.left;
                    if r.left > taskbar_left + taskbar_width / 2 && w < taskbar_width {
                        obstacles.push(r.left);
                    }
                }
            }
            sib = GetWindow(h, GW_HWNDNEXT).ok();
        }

        let x = crate::layout::compute_x(taskbar_left, taskbar_width, &obstacles, WIDTH, GAP);

        let mut cur = RECT::default();
        if GetWindowRect(hwnd, &mut cur).is_err() {
            return;
        }
        let cur_x = cur.left - taskbar_left;
        let cur_h = cur.bottom - cur.top;
        if cur_x != x || cur_h != tb_height {
            let _ = SetWindowPos(hwnd, Some(HWND_TOP), x, 0, WIDTH, tb_height, SWP_SHOWWINDOW);
        }
    }
}
```

- [ ] **Step 4: Handle `WM_TIMER` and clean up on `WM_DESTROY` in `wndproc`**

In the `match msg { ... }` inside `wndproc`, add a `WM_TIMER` arm and update `WM_DESTROY`:

```rust
            WM_TIMER => {
                reposition(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                let _ = KillTimer(hwnd, TIMER_ID);
                PostQuitMessage(0);
                LRESULT(0)
            }
```

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: compiles (warnings OK). Apply mechanical binding fixes if the compiler asks (e.g. `Some(HWND_TOP)` vs `HWND_TOP`, `KillTimer` argument form).

- [ ] **Step 6: Verify initial placement (assistant screen capture)**

Run `./target/debug/vEnter.exe` in the background, then capture the taskbar region and confirm `vEnter ▲ hello` sits immediately left of TrafficMonitor (not overlapping it). Command pattern (PowerShell) — capture x 800..1600 of the taskbar strip and inspect:

Run in background: `./target/debug/vEnter.exe`
Capture: use `System.Drawing` `CopyFromScreen(800, <taskbar_top>, 0, 0, <size>)` to a PNG in the scratchpad and read it.
Expected: white `vEnter ▲ hello` block appears with a small gap to the left of TrafficMonitor's stats; no overlap.

- [ ] **Step 7: Verify dynamic tracking (user action)**

Ask the user to **add or remove a tray icon** (drag one out of / into the tray-overflow chevron) while `vEnter.exe` is running.
Expected: TrafficMonitor shifts, and within ~250 ms `vEnter` slides to stay just left of it, still with no overlap. Then stop the process (`taskkill /F /IM vEnter.exe`).

- [ ] **Step 8: Commit**

```bash
git add src/window.rs
git commit -m "feat: track taskbar changes, park vEnter left of TrafficMonitor"
```

---

## Self-review notes

- **Spec coverage:** timer polling (Task 2 Step 3 `SetTimer`), anchor algorithm / obstacle filter (Task 2 Step 3 `reposition`), pure `compute_x` + tests (Task 1), reposition-only-on-change + `HWND_TOP` reassert (Task 2 Step 3). All spec success criteria covered.
- **Deferred items** (`TaskbarCreated`, DPI/multi-monitor, auto-fit width) intentionally not in any task, per spec.
- **Type consistency:** `compute_x` signature identical in Task 1 (definition) and Task 2 (call site `crate::layout::compute_x(taskbar_left, taskbar_width, &obstacles, WIDTH, GAP)`).
