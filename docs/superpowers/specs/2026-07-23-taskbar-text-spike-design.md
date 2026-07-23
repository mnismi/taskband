# Spike: Render text inside the native Windows taskbar (Rust)

**Date:** 2026-07-23
**Status:** Design approved, pending implementation plan
**Type:** Proof-of-concept spike

## Goal

Prove that a Rust executable (`vEnter.exe`) can render text **embedded inside the
native Windows taskbar** — sitting to the left of the system tray / clock, the way
the app *TrafficMonitor* renders its network/CPU stats.

This is a feasibility spike, not the finished product. Success is a single, narrow
outcome:

> **We can see our own hardcoded text sitting inside the real Windows taskbar.**

If this works, it becomes the foundation for a "Waybar-for-Windows" status bar with
real metrics. That later work is explicitly out of scope here.

## Approach: reparent into the taskbar (`SetParent`)

The taskbar is a real window with a well-known class name, `Shell_TrayWnd`. The
proven technique (used by TrafficMonitor and similar tools) is:

1. `FindWindowW("Shell_TrayWnd", NULL)` to get the taskbar's `HWND`.
2. Register a window class and `CreateWindowExW` a small child window of our own.
3. `SetParent(our_hwnd, taskbar_hwnd)` — our window becomes a genuine child of the
   taskbar. It now renders inside the taskbar and moves with it.
4. Position our window inside the taskbar (near the tray area, matching the
   screenshot) via `SetWindowPos` / `MoveWindow`.
5. Paint hardcoded text (e.g. `"vEnter ▲ hello"`) on `WM_PAINT` using GDI
   (`DrawTextW` / `TextOutW`), with a dark/transparent background so it blends into
   the taskbar.
6. Run a minimal message loop to keep the window alive and repainting.

### Why this over an overlay window

An always-on-top borderless overlay floated over the taskbar area is simpler but is
**not** actually embedded — it fights z-order, does not move with the taskbar, and
can be covered. It would not answer "is it even possible," so it is rejected for the
spike.

## Technology

- **Language:** Rust
- **Bindings:** the official `windows` crate (windows-rs) for Win32 APIs.
- **Binary name:** `vEnter` (produces `vEnter.exe`).
- **Rendering:** GDI (`DrawTextW`), the simplest path for text-on-taskbar.

## Components

The spike is small enough to be a single binary, but conceptually three pieces:

1. **Taskbar locator** — finds `Shell_TrayWnd`; returns its `HWND` (or a clear error
   if not found).
2. **Embedded window** — registers the class, creates the window, reparents it into
   the taskbar, and positions it.
3. **Painter** — the `WM_PAINT` handler that draws the hardcoded text.

Keeping these as distinct functions/modules (even in one file) makes the later
expansion to real metrics a matter of swapping the painter's data source.

## Explicitly out of scope (later)

These are known real concerns for a production tool, deliberately deferred so the
spike stays a spike:

- Real metrics (CPU, memory, network up/down, total traffic).
- Configuration / customization.
- Correct positioning across DPI scaling and font sizes.
- Multi-monitor / secondary taskbars.
- Surviving `explorer.exe` restarts (taskbar recreation) and re-embedding.
- Windows 11 vs Windows 10 taskbar internal differences beyond `Shell_TrayWnd`.
- Clean teardown / unembedding on exit.
- Transparency/theming polish to perfectly match the taskbar.

## Success criteria

- `cargo build` produces `vEnter.exe`.
- Running it makes our hardcoded text visibly appear **inside** the real taskbar.
- The text stays put as a child of the taskbar (moves with it, not a floating
  window).

## Risks

- **Windows version fragility:** taskbar internals differ across Win10/Win11
  builds. `Shell_TrayWnd` itself is stable, but child positioning may need tuning on
  the target machine (Windows 11 Pro, build 26200, per this environment).
- **Rendering quirks:** the Win11 taskbar is XAML-based; a reparented GDI child
  generally still paints, but background transparency may look imperfect. Acceptable
  for a spike.

## Findings (spike outcome — 2026-07-23)

**Spike succeeded.** `vEnter.exe` renders `vEnter ▲ hello` inside the real Windows
11 taskbar (confirmed by screen capture and by eye), sitting alongside the existing
TrafficMonitor stats.

**Key discovery — the window must be layered.** On Windows 11 the taskbar is
DWM-composited, and DWM only composites **layered** windows onto it. A plain GDI
child reparented into `Shell_TrayWnd` is invisible regardless of position or
z-order — it reports `IsWindowVisible = true` and sits at the top of the child
z-order, yet never appears. Diagnostics confirmed the working reference
(TrafficMonitor's `#32770` window) carries `WS_EX_LAYERED`. The fix:

1. Create the window with `WS_EX_LAYERED`.
2. Call `SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA)` so it is opaque and
   paints normally through `WM_PAINT`.

This is the single most important thing to carry into the next iteration. It was
not obvious from the original approach, which assumed a plain GDI child would paint.

**Positioning note:** the landing x is tuned for a 1920-wide taskbar (placed left
of the tray and the existing TrafficMonitor). Robust positioning across widths,
DPI, and secondary taskbars remains deferred as originally scoped.
