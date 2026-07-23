# Taskbar Text Spike — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove a Rust executable (`vEnter.exe`) can render hardcoded text embedded inside the native Windows taskbar, to the left of the tray/clock.

**Architecture:** A single Rust binary using the `windows` crate (windows-rs). It locates the taskbar window (`Shell_TrayWnd`), creates its own GDI-painted window, reparents that window into the taskbar via `SetParent`, converts it to a child window, and positions it inside the taskbar. A minimal Win32 message loop keeps it alive and repainting.

**Tech Stack:** Rust (edition 2021), `windows` crate 0.58, Win32 GDI for text rendering.

## Global Constraints

- **windows-rs version:** pin `windows = "0.58"`. Copy the feature list from Task 1 verbatim.
- **Binary name:** the produced executable must be `vEnter.exe` — set via `[[bin]] name = "vEnter"`. The Cargo package name is `venter` (lowercase, to satisfy Cargo naming).
- **Console subsystem:** keep the default console subsystem (do NOT add `#![windows_subsystem = "windows"]`). We want `println!`/`eprintln!` diagnostics visible while spiking.
- **Target platform:** Windows 11 Pro (build 26200). `Shell_TrayWnd` is stable across Win10/11; child positioning may need on-machine tuning.
- **windows-rs binding adaptation (IMPORTANT):** The exact wrapping of optional handle parameters is the single most version-volatile part of windows-rs. Specifically: `None` vs `HWND::default()` for null handles, `.into()` for `HMODULE`→`HINSTANCE`, and null-handle comparisons. The code below matches the official windows-rs 0.58 samples, but if a call does not compile, **run `cargo build` and follow the compiler's suggested fix for that one call** — these are mechanical, one-token changes, not design changes. Do NOT restructure the logic to work around them.
- **Testing reality:** Only the taskbar locator (Task 1) can be meaningfully unit-tested. Tasks 2 and 3 render pixels into GUI windows and are verified by a documented manual observation procedure with exact expected outcomes — this is deliberate, not a gap. Do not fabricate unit tests that assert on GUI pixels.

---

### Task 1: Project scaffold + taskbar locator

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/taskbar.rs` (contains the locator and its unit test)

**Interfaces:**
- Produces: `taskbar::find_taskbar() -> windows::core::Result<HWND>` — returns the `Shell_TrayWnd` handle, or `Err` if explorer/taskbar is not present.

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "venter"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "vEnter"
path = "src/main.rs"

[dependencies.windows]
version = "0.58"
features = [
    "Win32_Foundation",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Graphics_Gdi",
    "Win32_System_LibraryLoader",
]
```

- [ ] **Step 2: Write the failing test in `src/taskbar.rs`**

Write the module with the test, but leave `find_taskbar` unimplemented so the test fails to compile/pass first:

```rust
use windows::core::{w, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

/// Locate the native Windows taskbar window (`Shell_TrayWnd`).
pub fn find_taskbar() -> Result<HWND> {
    todo!("implemented in step 4")
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::HWND;

    #[test]
    fn find_taskbar_returns_a_handle() {
        let hwnd = find_taskbar().expect("Shell_TrayWnd should exist while explorer is running");
        assert_ne!(hwnd, HWND::default(), "taskbar handle should not be the null handle");
    }
}
```

Also create a minimal `src/main.rs` so the crate builds:

```rust
mod taskbar;

fn main() {
    match taskbar::find_taskbar() {
        Ok(hwnd) => println!("Found taskbar (Shell_TrayWnd): {hwnd:?}"),
        Err(e) => eprintln!("Failed to find taskbar: {e}"),
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test find_taskbar_returns_a_handle`
Expected: FAIL — panics at the `todo!("implemented in step 4")` in `find_taskbar`.

- [ ] **Step 4: Implement `find_taskbar`**

Replace the `todo!` body in `src/taskbar.rs`:

```rust
pub fn find_taskbar() -> Result<HWND> {
    // Safety: FindWindowW only queries the window manager; there are no
    // invariants for the caller to uphold.
    unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test find_taskbar_returns_a_handle`
Expected: PASS (explorer.exe is running on the dev machine, so `Shell_TrayWnd` exists).

- [ ] **Step 6: Run the binary to confirm end-to-end**

Run: `cargo run`
Expected: prints `Found taskbar (Shell_TrayWnd): HWND(...)` with a non-zero handle.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml src/main.rs src/taskbar.rs
git commit -m "feat: scaffold vEnter crate and locate the taskbar window"
```

---

### Task 2: Standalone GDI window that paints the text

Builds and verifies the *paint* path in isolation, before adding the *embed* path. This de-risks Task 3: if text does not appear here, the problem is painting; if it appears here but not in Task 3, the problem is embedding.

**Files:**
- Create: `src/window.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: nothing from Task 1 (independent).
- Produces:
  - `window::create_window() -> windows::core::Result<HWND>` — registers the window class, creates a visible `WS_POPUP` window at a fixed screen position, and returns its `HWND`. Its window procedure paints `"vEnter ▲ hello"` in white on a black background on `WM_PAINT`.
  - `window::run_message_loop()` — blocking Win32 message loop; returns when the window is destroyed.

- [ ] **Step 1: Create `src/window.rs`**

```rust
use windows::core::{w, Result};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DrawTextW, EndPaint, GetClientRect, SetBkMode, SetTextColor,
    DT_LEFT, DT_SINGLELINE, DT_VCENTER, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostQuitMessage,
    RegisterClassW, TranslateMessage, MSG, WINDOW_EX_STYLE, WM_DESTROY, WM_PAINT, WNDCLASSW,
    WS_POPUP, WS_VISIBLE,
};

/// Create a standalone, visible window that paints the spike text.
pub fn create_window() -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("vEnterTaskbarWindow"),
            hbrBackground: CreateSolidBrush(COLORREF(0x0000_0000)), // black
            ..Default::default()
        };
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("vEnterTaskbarWindow"),
            w!("vEnter ▲ hello"),
            WS_POPUP | WS_VISIBLE,
            100, 100,   // x, y on screen
            260, 40,    // width, height
            None,       // no parent (top-level for now)
            None,       // no menu
            instance,   // module handle
            None,       // no create param
        )?;

        Ok(hwnd)
    }
}

/// Blocking Win32 message loop. Returns when the window is destroyed.
pub fn run_message_loop() {
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);

                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);

                SetBkMode(hdc, TRANSPARENT);
                SetTextColor(hdc, COLORREF(0x00FF_FFFF)); // white

                let mut text: Vec<u16> = "vEnter ▲ hello".encode_utf16().collect();
                DrawTextW(hdc, &mut text, &mut rect, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
```

- [ ] **Step 2: Wire `src/main.rs` to create the window and run the loop**

Replace the whole file:

```rust
mod taskbar;
mod window;

fn main() -> windows::core::Result<()> {
    let _hwnd = window::create_window()?;
    println!("Standalone vEnter window created near the top-left of the screen.");
    window::run_message_loop();
    Ok(())
}
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles. If any windows-rs call errors, apply the mechanical fix per the Global Constraints "binding adaptation" note.

- [ ] **Step 4: Manual verification (paint path)**

Run: `cargo run`
Expected observations:
- A small (~260×40) borderless window appears near the top-left of the screen (position 100,100).
- It shows the white text `vEnter ▲ hello` on a black background.
Stop the program with `Ctrl+C` in the console (the window has no close button).

Record the result: does the text render? (Yes = paint path confirmed.)

- [ ] **Step 5: Commit**

```bash
git add src/window.rs src/main.rs
git commit -m "feat: render spike text in a standalone GDI window"
```

---

### Task 3: Embed the window into the taskbar

Reparents the painting window into `Shell_TrayWnd` and positions it inside the taskbar. This is the spike's success criterion.

**Files:**
- Modify: `src/window.rs` (add `embed_in_taskbar`)
- Modify: `src/main.rs` (find taskbar → create window → embed)

**Interfaces:**
- Consumes: `taskbar::find_taskbar()` (Task 1), `window::create_window()` and `window::run_message_loop()` (Task 2).
- Produces: `window::embed_in_taskbar(child: HWND, taskbar: HWND) -> windows::core::Result<()>` — reparents `child` into `taskbar`, converts it to a child window, and positions it inside the taskbar to the left of the tray area.

- [ ] **Step 1: Add imports to `src/window.rs`**

Extend the existing `use windows::Win32::UI::WindowsAndMessaging::{...}` import to also include these items (keep the ones already there):

```rust
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetWindowLongPtrW,
    GetWindowRect, PostQuitMessage, RegisterClassW, SetParent, SetWindowLongPtrW, SetWindowPos,
    TranslateMessage, GWL_STYLE, MSG, SWP_FRAMECHANGED, SWP_NOZORDER, SWP_SHOWWINDOW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_DESTROY, WM_PAINT, WNDCLASSW, WS_CHILD, WS_POPUP, WS_VISIBLE,
};
```

- [ ] **Step 2: Add `embed_in_taskbar` to `src/window.rs`**

```rust
/// Reparent `child` into the taskbar (`taskbar`), turn it into a child
/// window, and position it inside the taskbar just left of the tray area.
pub fn embed_in_taskbar(child: HWND, taskbar: HWND) -> Result<()> {
    unsafe {
        // 1. Reparent our window into the taskbar.
        SetParent(child, taskbar)?;

        // 2. Convert it to a child window so it clips to and moves with the taskbar.
        let current = WINDOW_STYLE(GetWindowLongPtrW(child, GWL_STYLE) as u32);
        let child_style = (current & !WS_POPUP) | WS_CHILD | WS_VISIBLE;
        SetWindowLongPtrW(child, GWL_STYLE, child_style.0 as isize);

        // 3. Position inside the taskbar, leaving room on the right for the tray/clock.
        let mut tb = RECT::default();
        GetWindowRect(taskbar, &mut tb)?;
        let tb_width = tb.right - tb.left;
        let tb_height = tb.bottom - tb.top;

        let width = 260;
        let x = (tb_width - width - 220).max(0); // ~220px of clearance for the tray
        SetWindowPos(
            child,
            None,
            x, 0, width, tb_height,
            SWP_NOZORDER | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
        )?;

        Ok(())
    }
}
```

- [ ] **Step 3: Update `src/main.rs` to embed**

Replace the whole file:

```rust
mod taskbar;
mod window;

fn main() -> windows::core::Result<()> {
    let taskbar = taskbar::find_taskbar()?;
    let child = window::create_window()?;
    window::embed_in_taskbar(child, taskbar)?;
    println!("vEnter embedded into the taskbar — look to the left of the tray/clock.");
    window::run_message_loop();
    Ok(())
}
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: compiles. Apply mechanical binding fixes if needed (see Global Constraints).

- [ ] **Step 5: Manual verification (spike success criterion)**

Run: `cargo run`
Expected observations:
- The white text `vEnter ▲ hello` appears **inside the real taskbar**, to the left of the system tray/clock.
- It is part of the taskbar (not a floating window) — it sits on the taskbar strip and does not appear elsewhere on screen.
- Optional check: it moves with the taskbar (e.g. if the taskbar is repositioned) and stays put over other windows without a separate window frame.

If the text appears but is mispositioned (off-screen or overlapping the tray), adjust the `220` clearance / `width` constants in `embed_in_taskbar` and re-run. Positioning tuning is expected per the spec's risk note.

Stop with `Ctrl+C` in the console.

Record the result: **is the text visible inside the taskbar?** Yes = spike succeeds; the concept is proven.

- [ ] **Step 6: Commit**

```bash
git add src/window.rs src/main.rs
git commit -m "feat: embed the vEnter window into the native taskbar"
```

---

## Notes for the next iteration (out of scope now)

Once the spike is confirmed, the natural next steps (deferred per the spec) are: swap the hardcoded string for real metrics behind the painter, re-embed after `explorer.exe` restarts (listen for the `TaskbarCreated` message), handle DPI/positioning robustly, and clean teardown on exit. Do not build these until the spike is verified.
