# Dynamic positioning — track the tray / TrafficMonitor

**Date:** 2026-07-23
**Status:** Design approved, pending implementation plan
**Type:** Increment on the taskbar-text spike (see `2026-07-23-taskbar-text-spike-design.md`)

## Goal

Make the embedded `vEnter` window **follow taskbar changes** instead of sitting at
a fixed x. It should park immediately to the **left of TrafficMonitor** and slide
whenever TrafficMonitor moves (e.g. when a tray icon is added/removed) or the
taskbar is resized/moved — the same "live" behavior TrafficMonitor exhibits by
anchoring to the tray.

The user chose **coexist**: vEnter sits just left of TrafficMonitor, both visible.

## Approach: timer-driven recompute (polling)

A repeating timer (`SetTimer`, ~250 ms → `WM_TIMER`) recomputes the target position
each tick and repositions only when it changes. This is simple, robust, and matches
how TrafficMonitor feels live. It naturally covers tray icons added/removed,
TrafficMonitor sliding, and taskbar resize/move because every tick reads current
window rects.

Rejected alternative: `SetWinEventHook` (event-driven). More precise but
significantly more Win32 plumbing and edge cases; unnecessary here.

## Anchor algorithm

Each tick:

1. Get the taskbar rect (`GetWindowRect(Shell_TrayWnd)`), giving `taskbar_left` and
   `taskbar_width`.
2. Enumerate the taskbar's direct children. Collect the screen left-edge of each
   **obstacle**: a child that is
   - visible,
   - not our own window,
   - positioned in the right half (`left > taskbar_left + taskbar_width/2`), and
   - not full-width (`width < taskbar_width`, excludes the full-width XAML
     `DesktopWindowContentBridge`/`CoreWindow`).

   On the target machine this yields `{TrafficMonitor #32770, TrayNotifyWnd}`.
3. `right_boundary = min(obstacle lefts)` (or the taskbar's right edge if there are
   no obstacles).
4. Our **right edge = right_boundary − gap**; so
   `x = (right_boundary_client − gap − width).max(0)`, where `*_client` means
   converted to taskbar-client coordinates (`screen_x − taskbar_left`).
5. If `x` (or the taskbar height) differs from our current placement, `SetWindowPos`
   to the new rect with `HWND_TOP` (reassert composited z-order); otherwise do
   nothing (avoids flicker).

## Components

- **`layout::compute_x` (pure, unit-tested):**
  `compute_x(taskbar_left, taskbar_width, obstacle_lefts, width, gap) -> i32`.
  Contains all the arithmetic (boundary selection, client conversion, clamp). No
  Win32 — fully testable.
- **`window` glue (visually verified):** the `WM_TIMER` handler that gathers
  obstacle rects via Win32 enumeration, calls `compute_x`, and repositions. Plus
  `SetTimer` started after embedding.

## Success criteria

- Adding/removing a tray icon (which moves TrafficMonitor) makes vEnter move with
  it, staying just to its left — no overlap.
- Unit tests for `compute_x` pass (no obstacles, single obstacle, multiple
  obstacles, clamp-to-zero, non-zero `taskbar_left`).

## Out of scope (deferred)

- Re-embedding after `explorer.exe` restart (`TaskbarCreated`).
- DPI scaling and secondary-monitor taskbars.
- Auto-fitting the window width to the text (fixed width for now).
- Per-tick z-order reassertion when position is unchanged (only reassert on move).

## Risks

- The obstacle filter is geometry-based; an unusual third-party taskbar child in the
  right half could shift our anchor. Acceptable — the min-left rule degrades
  gracefully (we just sit further left).
- 250 ms polling is a tiny, constant cost; fine for a status bar.
