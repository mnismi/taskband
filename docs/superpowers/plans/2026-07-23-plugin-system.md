# vEnter Plugin System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the hardcoded taskbar label with a waybar-style plugin system — config-driven modules that run scripts on an interval, render right-aligned, and are styled with CSS properties.

**Architecture:** A JSONC config defines ordered modules (`modules-right`), each with an `exec` command, an `interval`, and a `css` block. A background worker thread runs due plugins and pushes `(index, text)` updates over an `mpsc` channel. The Win32 UI thread drains the channel on its existing timer, remeasures/repositions, and repaints each module with its resolved style. Rendering keeps the layered color-key transparency already in place.

**Tech Stack:** Rust, `windows` crate 0.58 (Win32 GDI), `serde` + `json5` for config, `std::process::Command` + `std::sync::mpsc` + `std::thread` for plugin execution.

## Global Constraints

- Binary name is `vEnter` (produces `vEnter.exe`); crate/package name is `venter`. Do not change these.
- Windows-only; uses the `windows` crate 0.58 and `std::os::windows` extensions.
- Keep the layered color-key transparency: `SetLayeredWindowAttributes(hwnd, COLORREF(0x000000), 0, LWA_COLORKEY)`. Pure black `#000000` is reserved as the transparency key and must not be used as a `background-color`.
- Plugin commands run via `cmd /C <exec>` verbatim (`raw_arg`) with `CREATE_NO_WINDOW`. Never run a plugin command on the UI thread.
- All arithmetic/parsing/scheduling logic goes in **pure functions with unit tests**; Win32 glue is verified visually and via the System.Drawing screen-capture method used elsewhere in this project.
- TDD: write the failing test first, watch it fail, implement minimally, watch it pass, commit. Frequent commits.
- Design reference: `docs/superpowers/specs/2026-07-23-plugin-system-design.md`.

## File Structure

- `Cargo.toml` — add `serde` (derive) and `json5` deps.
- `src/config.rs` — **new.** serde types (`RawConfig`, `ModuleConfig`), `parse(&str)`, `load(&Path)`, `config_path()`.
- `src/css.rs` — **new.** `Color`, `Edges`, `Style` (+ `Default`), `Color::colorref`. CSS value parsers and `resolve` merge added in Increment 2.
- `src/plugin.rs` — **new.** `PluginSpec`, `Update`, `is_due` (pure), `spawn_worker`, `run_exec`.
- `src/layout.rs` — **modify.** keep `compute_x`; add `place_modules`.
- `src/window.rs` — **modify.** `State` + `State::new`, per-module paint, timer channel drain, dynamic-width reposition; `create_window` takes the state.
- `src/main.rs` — **modify.** load config → build styles/specs → spawn worker → create window with state → embed → run.
- `venter.json` — **new.** committed example config (cpu + clock via inline PowerShell).

---

## Increment 1 — Config-driven plain-text plugins (no CSS yet)

Deliverable: vEnter reads `venter.json`, runs the listed plugins on a worker thread, and renders their plain-text output right-aligned with a single default font. CSS blocks are parsed but not yet applied.

### Task 1: Dependencies + config parsing

**Files:**
- Modify: `Cargo.toml`
- Create: `src/config.rs`
- Modify: `src/main.rs` (add `mod config;`)

**Interfaces:**
- Produces:
  - `config::RawConfig { pub modules_right: Vec<String>, pub css: HashMap<String, String>, pub modules: HashMap<String, ModuleConfig> }`
  - `config::ModuleConfig { pub exec: String, pub interval: u64, pub css: HashMap<String, String> }`
  - `config::parse(text: &str) -> Result<RawConfig, String>`
  - `config::load(path: &std::path::Path) -> Result<RawConfig, String>`
  - `config::config_path() -> std::path::PathBuf`

- [ ] **Step 1: Add dependencies**

Edit `Cargo.toml`, adding below the `[dependencies.windows]` block:

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
json5 = "1"
```

(Leave the existing `[dependencies.windows]` table as-is; a `[dependencies]` table and a `[dependencies.windows]` table can coexist.)

- [ ] **Step 2: Write the failing test**

Create `src/config.rs` with only the tests first (module body added next step won't exist yet, so this fails to compile — that is the "fail"):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modules_order_and_css() {
        let cfg = parse(
            r#"{
                // order, left-to-right
                "modules-right": ["cpu", "clock"],
                "css": { "color": "#ffffff" },
                "cpu": { "exec": "echo hi", "interval": 2, "css": { "font-weight": "bold" } },
                "clock": { "exec": "echo now" }
            }"#,
        )
        .expect("valid config");

        assert_eq!(cfg.modules_right, vec!["cpu".to_string(), "clock".to_string()]);
        assert_eq!(cfg.css.get("color").map(String::as_str), Some("#ffffff"));

        let cpu = cfg.modules.get("cpu").expect("cpu module");
        assert_eq!(cpu.exec, "echo hi");
        assert_eq!(cpu.interval, 2);
        assert_eq!(cpu.css.get("font-weight").map(String::as_str), Some("bold"));

        let clock = cfg.modules.get("clock").expect("clock module");
        assert_eq!(clock.interval, 5); // default when omitted
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(parse("{ not valid").is_err());
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib config 2>&1 | head -30`
Expected: compile error — `cannot find function 'parse'` / `RawConfig` not defined.

- [ ] **Step 4: Write the implementation**

Prepend to `src/config.rs` (above the `#[cfg(test)]` block):

```rust
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct RawConfig {
    #[serde(rename = "modules-right", default)]
    pub modules_right: Vec<String>,
    #[serde(default)]
    pub css: HashMap<String, String>,
    /// Every remaining top-level key is a module definition, keyed by name.
    #[serde(flatten)]
    pub modules: HashMap<String, ModuleConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ModuleConfig {
    pub exec: String,
    #[serde(default = "default_interval")]
    pub interval: u64,
    #[serde(default)]
    pub css: HashMap<String, String>,
}

fn default_interval() -> u64 {
    5
}

/// Parse a JSONC (JSON5) config string. Comments and trailing commas allowed.
pub fn parse(text: &str) -> Result<RawConfig, String> {
    json5::from_str(text).map_err(|e| e.to_string())
}

/// Read and parse the config file at `path`.
pub fn load(path: &Path) -> Result<RawConfig, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("reading {}: {e}", path.display()))?;
    parse(&text).map_err(|e| format!("parsing {}: {e}", path.display()))
}

/// Resolve the config path: `venter.json` next to the executable, else `venter.json`
/// in the current working directory.
pub fn config_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("venter.json");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from("venter.json")
}
```

Add the module declaration at the top of `src/main.rs` (with the other `mod` lines):

```rust
mod config;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib config 2>&1 | tail -20`
Expected: both tests pass.

> **Fallback note:** if `#[serde(flatten)]` fails to deserialize under `json5` (error mentions `deserialize_any` / flatten), replace the derive with a manual pass: deserialize into `HashMap<String, json5::Value>`-style intermediate is not available, so instead keep `modules_right` and `css` as `Option` fields on a struct that also derives `Deserialize` for a `HashMap<String, ModuleConfig>` via a second `parse` of the same text with those keys removed. Prefer flatten; only fall back if it genuinely fails.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/config.rs src/main.rs
git commit -m "feat: parse JSONC plugin config (modules, order, css)"
```

---

### Task 2: Style types

**Files:**
- Create: `src/css.rs`
- Modify: `src/main.rs` (add `mod css;`)

**Interfaces:**
- Produces:
  - `css::Color { pub r: u8, pub g: u8, pub b: u8 }` with `Color::colorref(self) -> u32` (Win32 `0x00BBGGRR`)
  - `css::Edges { pub top: i32, pub right: i32, pub bottom: i32, pub left: i32 }` (derives `Default`)
  - `css::Style { pub color: Color, pub background: Option<Color>, pub font_family: String, pub font_size: i32, pub font_weight: i32, pub padding: Edges, pub margin: Edges }` with `Default`

- [ ] **Step 1: Write the failing test**

Create `src/css.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_style_is_light_gray_segoe_12() {
        let s = Style::default();
        assert_eq!(s.color, Color { r: 0xd0, g: 0xd0, b: 0xd0 });
        assert_eq!(s.background, None);
        assert_eq!(s.font_family, "Segoe UI");
        assert_eq!(s.font_size, 12);
        assert_eq!(s.font_weight, 400);
        assert_eq!(s.padding, Edges::default());
    }

    #[test]
    fn colorref_is_bgr_packed() {
        // R=0x11 G=0x22 B=0x33 -> 0x00332211
        assert_eq!(Color { r: 0x11, g: 0x22, b: 0x33 }.colorref(), 0x0033_2211);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib css 2>&1 | head -20`
Expected: compile error — `Style` / `Color` / `Edges` not defined.

- [ ] **Step 3: Write the implementation**

Prepend to `src/css.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    /// Win32 COLORREF packs bytes as 0x00BBGGRR.
    pub fn colorref(self) -> u32 {
        (self.r as u32) | ((self.g as u32) << 8) | ((self.b as u32) << 16)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Edges {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Style {
    pub color: Color,
    pub background: Option<Color>,
    pub font_family: String,
    pub font_size: i32,
    pub font_weight: i32,
    pub padding: Edges,
    pub margin: Edges,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            color: Color { r: 0xd0, g: 0xd0, b: 0xd0 },
            background: None,
            font_family: "Segoe UI".to_string(),
            font_size: 12,
            font_weight: 400,
            padding: Edges::default(),
            margin: Edges::default(),
        }
    }
}
```

Add to `src/main.rs`:

```rust
mod css;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib css 2>&1 | tail -20`
Expected: both tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/css.rs src/main.rs
git commit -m "feat: add Style/Color/Edges styling types"
```

---

### Task 3: Plugin worker

**Files:**
- Create: `src/plugin.rs`
- Modify: `src/main.rs` (add `mod plugin;`)

**Interfaces:**
- Produces:
  - `plugin::PluginSpec { pub name: String, pub exec: String, pub interval: std::time::Duration }`
  - `plugin::Update { pub index: usize, pub text: String }`
  - `plugin::is_due(elapsed_since_last: Option<Duration>, interval: Duration) -> bool` (pure)
  - `plugin::spawn_worker(specs: Vec<PluginSpec>) -> std::sync::mpsc::Receiver<Update>`

- [ ] **Step 1: Write the failing test**

Create `src/plugin.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn never_run_is_due() {
        assert!(is_due(None, Duration::from_secs(2)));
    }

    #[test]
    fn due_when_interval_elapsed() {
        assert!(is_due(Some(Duration::from_secs(2)), Duration::from_secs(2)));
        assert!(is_due(Some(Duration::from_secs(3)), Duration::from_secs(2)));
    }

    #[test]
    fn not_due_before_interval() {
        assert!(!is_due(Some(Duration::from_millis(500)), Duration::from_secs(2)));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib plugin 2>&1 | head -20`
Expected: compile error — `is_due` not defined.

- [ ] **Step 3: Write the implementation**

Prepend to `src/plugin.rs`:

```rust
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

/// Prevents a console window flashing when a plugin command spawns cmd.exe.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Worker tick; how often the worker wakes to check which modules are due.
const TICK: Duration = Duration::from_millis(100);

pub struct PluginSpec {
    pub name: String,
    pub exec: String,
    pub interval: Duration,
}

pub struct Update {
    pub index: usize,
    pub text: String,
}

/// A module is due when it has never run, or `interval` has elapsed since it did.
pub fn is_due(elapsed_since_last: Option<Duration>, interval: Duration) -> bool {
    match elapsed_since_last {
        None => true,
        Some(elapsed) => elapsed >= interval,
    }
}

/// Run one command line through `cmd /C` verbatim and return trimmed stdout.
fn run_exec(name: &str, exec: &str) -> String {
    match Command::new("cmd")
        .raw_arg(format!("/C {exec}"))
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(e) => {
            eprintln!("vEnter: module '{name}' exec failed: {e}");
            String::new()
        }
    }
}

/// Spawn a background thread that runs due plugins and streams `(index, text)`
/// updates. The thread exits when the receiver is dropped.
pub fn spawn_worker(specs: Vec<PluginSpec>) -> Receiver<Update> {
    let (tx, rx) = mpsc::channel::<Update>();
    thread::spawn(move || {
        let mut last_run: Vec<Option<Instant>> = vec![None; specs.len()];
        loop {
            let now = Instant::now();
            for (i, spec) in specs.iter().enumerate() {
                let elapsed = last_run[i].map(|t| now.duration_since(t));
                if is_due(elapsed, spec.interval) {
                    last_run[i] = Some(now);
                    let text = run_exec(&spec.name, &spec.exec);
                    if tx.send(Update { index: i, text }).is_err() {
                        return; // receiver gone; UI closed
                    }
                }
            }
            thread::sleep(TICK);
        }
    });
    rx
}
```

Add to `src/main.rs`:

```rust
mod plugin;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib plugin 2>&1 | tail -20`
Expected: three tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/plugin.rs src/main.rs
git commit -m "feat: plugin worker thread with interval scheduling"
```

---

### Task 4: Right-aligned placement math

**Files:**
- Modify: `src/layout.rs`

**Interfaces:**
- Consumes: existing `layout::compute_x` (unchanged).
- Produces: `layout::place_modules(widths: &[i32]) -> (Vec<i32>, i32)` — per-module left offset within the bar (packed left-to-right) and the total bar width.

- [ ] **Step 1: Write the failing test**

Append to the `mod tests` block in `src/layout.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib layout 2>&1 | head -20`
Expected: compile error — `place_modules` not defined.

- [ ] **Step 3: Write the implementation**

Add above the `#[cfg(test)]` block in `src/layout.rs`:

```rust
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib layout 2>&1 | tail -20`
Expected: the new tests plus the existing `compute_x` tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/layout.rs
git commit -m "feat: right-aligned module placement math"
```

---

### Task 5: Window integration + wiring (end-to-end)

**Files:**
- Modify: `src/window.rs` (replace `create_window`, `reposition`, the `WM_PAINT`/`WM_TIMER` handlers; add `State`, `make_font`, `measure`, `relayout`; delete the `WIDTH` constant)
- Modify: `src/main.rs` (full wiring)
- Create: `venter.json`

**Interfaces:**
- Consumes: `config::{load, config_path, ModuleConfig}`, `css::{Style, Color}`, `plugin::{PluginSpec, Update, spawn_worker}`, `layout::{compute_x, place_modules}`.
- Produces:
  - `window::State` (opaque fields) with `State::new(styles: Vec<css::Style>, rx: std::sync::mpsc::Receiver<plugin::Update>) -> State`
  - `window::create_window(state: Box<State>) -> windows::core::Result<HWND>`
  - `window::embed_in_taskbar` / `window::run_message_loop` (unchanged signatures)

> **Win32 binding note:** exact integer/enum types on some GDI bindings (e.g. `CreateFontW`'s italic/pitch parameters, whether `GetDC` takes `HWND` vs `Option<HWND>`) can differ slightly in `windows` 0.58. If the compiler flags a type mismatch, apply the mechanical fix it suggests (add/remove `.into()`, wrap/unwrap `Some(..)`, adjust a `BOOL`/`u32` literal). These do not change the logic.

- [ ] **Step 1: Replace the imports and constants in `src/window.rs`**

Replace the top of `src/window.rs` (the `use` block and the `const` lines) with:

```rust
use std::sync::mpsc::Receiver;

use windows::core::{w, PCWSTR, Result};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, TRUE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, GetDC,
    InvalidateRect, ReleaseDC, SelectObject, SetBkMode, SetTextColor, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_PITCH, DEFAULT_QUALITY, DT_CALCRECT, DT_LEFT, DT_SINGLELINE,
    DT_VCENTER, FF_DONTCARE, HDC, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW, GetParent,
    GetWindow, GetWindowLongPtrW, GetWindowRect, IsWindowVisible, KillTimer, PostQuitMessage,
    RegisterClassW, SetLayeredWindowAttributes, SetParent, SetTimer, SetWindowLongPtrW,
    SetWindowPos, TranslateMessage, GWLP_USERDATA, GWL_STYLE, GW_CHILD, GW_HWNDNEXT, HWND_TOP,
    LWA_COLORKEY, MSG, SWP_SHOWWINDOW, WINDOW_STYLE, WM_DESTROY, WM_PAINT, WM_TIMER, WNDCLASSW,
    WS_CHILD, WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_POPUP, WS_VISIBLE,
};

use crate::css::Style;
use crate::plugin::Update;

const TIMER_ID: usize = 1;
const TIMER_MS: u32 = 250;
const GAP: i32 = 8;
```

- [ ] **Step 2: Add the `State` struct and helpers**

Add after the constants in `src/window.rs`:

```rust
/// UI-thread render state, attached to the window via GWLP_USERDATA. Only the UI
/// thread touches it. The worker thread owns nothing here — it only sends Updates.
pub struct State {
    texts: Vec<String>,
    styles: Vec<Style>,
    widths: Vec<i32>,
    offsets: Vec<i32>,
    total_width: i32,
    rx: Receiver<Update>,
}

impl State {
    pub fn new(styles: Vec<Style>, rx: Receiver<Update>) -> Self {
        let n = styles.len();
        State {
            texts: vec![String::new(); n],
            widths: vec![0; n],
            offsets: vec![0; n],
            total_width: 0,
            styles,
            rx,
        }
    }
}

/// Build a GDI font from a resolved style. Caller must DeleteObject it.
unsafe fn make_font(style: &Style) -> HFONT {
    let mut face: Vec<u16> = style.font_family.encode_utf16().collect();
    face.push(0);
    CreateFontW(
        -style.font_size, // negative => character height in logical (pixel) units
        0,
        0,
        0,
        style.font_weight,
        0, // italic
        0, // underline
        0, // strikeout
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        DEFAULT_QUALITY,
        (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
        PCWSTR(face.as_ptr()),
    )
}

/// Measure a module's full width: text extent + horizontal padding + margin.
unsafe fn measure(hdc: HDC, style: &Style, text: &str) -> i32 {
    let font = make_font(style);
    let old = SelectObject(hdc, font);
    let mut utf16: Vec<u16> = text.encode_utf16().collect();
    let mut r = RECT::default();
    DrawTextW(hdc, &mut utf16, &mut r, DT_CALCRECT | DT_SINGLELINE | DT_LEFT);
    let text_w = r.right - r.left;
    SelectObject(hdc, old);
    let _ = DeleteObject(font);
    text_w
        + style.padding.left
        + style.padding.right
        + style.margin.left
        + style.margin.right
}

/// Re-measure all modules against current text and recompute offsets/total width.
unsafe fn relayout(hwnd: HWND, state: &mut State) {
    let hdc = GetDC(hwnd);
    for i in 0..state.styles.len() {
        state.widths[i] = measure(hdc, &state.styles[i], &state.texts[i]);
    }
    ReleaseDC(hwnd, hdc);
    let (offsets, total) = crate::layout::place_modules(&state.widths);
    state.offsets = offsets;
    state.total_width = total;
}
```

- [ ] **Step 3: Replace `create_window`**

Replace the whole `pub fn create_window` in `src/window.rs` with:

```rust
/// Create the layered taskbar window and attach its render state.
pub fn create_window(state: Box<State>) -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("vEnterTaskbarWindow"),
            hbrBackground: CreateSolidBrush(COLORREF(0x0000_0000)), // black = transparent key
            ..Default::default()
        };
        RegisterClassW(&wc);

        // WS_EX_LAYERED is required: the Windows 11 taskbar only composites
        // layered windows. Color-key black => transparent (see design doc).
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED,
            w!("vEnterTaskbarWindow"),
            w!("vEnter"),
            WS_POPUP | WS_VISIBLE | WS_CLIPSIBLINGS,
            100,
            100,
            260,
            40,
            None,
            None,
            instance,
            None,
        )?;

        SetLayeredWindowAttributes(hwnd, COLORREF(0x0000_0000), 0, LWA_COLORKEY)?;

        // Hand ownership of State to the window.
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);

        Ok(hwnd)
    }
}
```

- [ ] **Step 4: Replace `reposition` to use the dynamic width**

Replace the whole `fn reposition` in `src/window.rs` with:

```rust
/// Recompute where the bar should sit (just left of the tray / embedded apps)
/// using the current total width, and move it there only if something changed.
fn reposition(hwnd: HWND) {
    unsafe {
        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State;
        let width = if state_ptr.is_null() {
            0
        } else {
            (*state_ptr).total_width
        };

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

        let x = crate::layout::compute_x(taskbar_left, taskbar_width, &obstacles, width, GAP);

        let mut cur = RECT::default();
        if GetWindowRect(hwnd, &mut cur).is_err() {
            return;
        }
        let cur_x = cur.left - taskbar_left;
        let cur_w = cur.right - cur.left;
        let cur_h = cur.bottom - cur.top;
        if cur_x != x || cur_w != width || cur_h != tb_height {
            let _ = SetWindowPos(hwnd, HWND_TOP, x, 0, width, tb_height, SWP_SHOWWINDOW);
        }
    }
}
```

- [ ] **Step 5: Replace the `wndproc` body (paint, timer, destroy)**

Replace the whole `extern "system" fn wndproc` in `src/window.rs` with:

```rust
extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);

                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State;
                if !state_ptr.is_null() {
                    let state = &*state_ptr;
                    let mut client = RECT::default();
                    let _ = GetClientRect(hwnd, &mut client);
                    let height = client.bottom - client.top;

                    for i in 0..state.styles.len() {
                        let style = &state.styles[i];
                        let x0 = state.offsets[i];
                        let w = state.widths[i];

                        // Module box excludes its margins.
                        let left = x0 + style.margin.left;
                        let right = x0 + w - style.margin.right;
                        let mrect = RECT { left, top: 0, right, bottom: height };

                        // Background: real color if set, else the transparency key (black).
                        let bg = match style.background {
                            Some(c) => c.colorref(),
                            None => 0x0000_0000,
                        };
                        let brush = CreateSolidBrush(COLORREF(bg));
                        FillRect(hdc, &mrect, brush);
                        let _ = DeleteObject(brush);

                        // Text within the padded area.
                        let font = make_font(style);
                        let old = SelectObject(hdc, font);
                        SetBkMode(hdc, TRANSPARENT);
                        SetTextColor(hdc, COLORREF(style.color.colorref()));
                        let mut trect = RECT {
                            left: left + style.padding.left,
                            top: 0,
                            right: right - style.padding.right,
                            bottom: height,
                        };
                        let mut utf16: Vec<u16> = state.texts[i].encode_utf16().collect();
                        DrawTextW(hdc, &mut utf16, &mut trect, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                        SelectObject(hdc, old);
                        let _ = DeleteObject(font);
                    }
                }

                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_TIMER => {
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
                if !state_ptr.is_null() {
                    let state = &mut *state_ptr;
                    let mut changed = false;
                    while let Ok(update) = state.rx.try_recv() {
                        if update.index < state.texts.len()
                            && state.texts[update.index] != update.text
                        {
                            state.texts[update.index] = update.text;
                            changed = true;
                        }
                    }
                    if changed {
                        relayout(hwnd, state);
                        let _ = InvalidateRect(hwnd, None, TRUE);
                    }
                }
                reposition(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                let _ = KillTimer(hwnd, TIMER_ID);
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
                if !ptr.is_null() {
                    drop(Box::from_raw(ptr)); // drops State + rx (worker stops)
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
```

(`embed_in_taskbar` and `run_message_loop` are unchanged. Note the unused-parameter suppression: `lparam` is still used in the `_ =>` arm, so no warning.)

- [ ] **Step 6: Rewrite `src/main.rs` to wire everything**

Replace the entire contents of `src/main.rs` with:

```rust
mod config;
mod css;
mod layout;
mod plugin;
mod taskbar;
mod window;

use std::time::Duration;

fn main() -> windows::core::Result<()> {
    let path = config::config_path();
    let cfg = match config::load(&path) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("vEnter: {e}");
            std::process::exit(1);
        }
    };

    // Resolve display order: each name in modules-right that has a definition.
    let ordered: Vec<(String, config::ModuleConfig)> = cfg
        .modules_right
        .iter()
        .filter_map(|name| cfg.modules.get(name).map(|m| (name.clone(), m.clone())))
        .collect();

    if ordered.is_empty() {
        eprintln!("vEnter: no modules to render (check \"modules-right\" in {})", path.display());
    }

    // Increment 1: every module uses the default style. Increment 2 swaps this
    // for css::resolve(&cfg.css, &m.css).
    let styles: Vec<css::Style> = ordered.iter().map(|_| css::Style::default()).collect();

    let specs: Vec<plugin::PluginSpec> = ordered
        .iter()
        .map(|(name, m)| plugin::PluginSpec {
            name: name.clone(),
            exec: m.exec.clone(),
            interval: Duration::from_secs(m.interval.max(1)),
        })
        .collect();

    let rx = plugin::spawn_worker(specs);
    let state = Box::new(window::State::new(styles, rx));

    let taskbar = taskbar::find_taskbar()?;
    let child = window::create_window(state)?;
    window::embed_in_taskbar(child, taskbar)?;
    println!("vEnter embedded — {} module(s).", ordered.len());
    window::run_message_loop();
    Ok(())
}
```

- [ ] **Step 7: Create the example config**

Create `venter.json` at the repo root:

```jsonc
{
  // rendered left-to-right; the last entry sits nearest the tray
  "modules-right": ["cpu", "clock"],

  "css": {
    "font-family": "Segoe UI",
    "font-size": "12px",
    "color": "#d0d0d0",
    "padding": "0 8px"
  },

  "cpu": {
    "exec": "powershell -NoProfile -Command \"'CPU ' + (Get-CimInstance Win32_Processor).LoadPercentage + '%'\"",
    "interval": 2,
    "css": { "color": "#7fdbb0", "font-weight": "bold" }
  },
  "clock": {
    "exec": "powershell -NoProfile -Command \"(Get-Date).ToString('HH:mm:ss')\"",
    "interval": 1,
    "css": { "color": "#ffffff" }
  }
}
```

- [ ] **Step 8: Build and run the full test suite**

Run: `cargo build 2>&1 | tail -20`
Expected: compiles (apply any mechanical binding fixes per the Win32 note).

Run: `cargo test 2>&1 | tail -20`
Expected: all unit tests pass.

- [ ] **Step 9: Verify end-to-end visually**

Run the app, capture the taskbar, then stop it:

```bash
taskkill //F //IM vEnter.exe 2>/dev/null; ./target/debug/vEnter.exe &
```

Wait ~2s, then capture the right strip of the taskbar (PowerShell System.Drawing, as used elsewhere in this project) to a PNG and inspect it. Expected: two labels left of the tray — a clock ticking (`HH:MM:SS`) and a CPU reading (`CPU N%`), both in the default gray font, transparent background, parked left of TrafficMonitor. Then:

```bash
taskkill //F //IM vEnter.exe
```

- [ ] **Step 10: Commit**

```bash
git add src/window.rs src/main.rs venter.json
git commit -m "feat: render config-driven plugin modules on the taskbar"
```

---

## Increment 2 — CSS styling

Deliverable: each module's `css` (merged over the top-level `css` defaults) drives its color, background, font, padding, and margin.

### Task 6: CSS value parsers

**Files:**
- Modify: `src/css.rs`

**Interfaces:**
- Produces (all pure):
  - `css::parse_color(&str) -> Option<Color>` (`#rgb`, `#rrggbb`)
  - `css::parse_px(&str) -> Option<i32>` (`"12px"` or `"12"`)
  - `css::parse_weight(&str) -> Option<i32>` (`normal`=400, `bold`=700, `100..=900`)
  - `css::parse_edges(&str) -> Option<Edges>` (1–4 CSS-shorthand `px` values)

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block in `src/css.rs`:

```rust
    #[test]
    fn parses_hex_colors() {
        assert_eq!(parse_color("#ffffff"), Some(Color { r: 255, g: 255, b: 255 }));
        assert_eq!(parse_color("#000000"), Some(Color { r: 0, g: 0, b: 0 }));
        assert_eq!(parse_color("#7fdbb0"), Some(Color { r: 0x7f, g: 0xdb, b: 0xb0 }));
        // 3-digit shorthand expands each nibble
        assert_eq!(parse_color("#fff"), Some(Color { r: 255, g: 255, b: 255 }));
        assert_eq!(parse_color("#123"), Some(Color { r: 0x11, g: 0x22, b: 0x33 }));
        assert_eq!(parse_color("nope"), None);
        assert_eq!(parse_color("#12"), None);
    }

    #[test]
    fn parses_px_lengths() {
        assert_eq!(parse_px("12px"), Some(12));
        assert_eq!(parse_px("  8 "), Some(8));
        assert_eq!(parse_px("0"), Some(0));
        assert_eq!(parse_px("abc"), None);
    }

    #[test]
    fn parses_font_weight() {
        assert_eq!(parse_weight("normal"), Some(400));
        assert_eq!(parse_weight("bold"), Some(700));
        assert_eq!(parse_weight("600"), Some(600));
        assert_eq!(parse_weight("50"), None);
        assert_eq!(parse_weight("999"), None);
    }

    #[test]
    fn parses_edges_shorthand() {
        assert_eq!(parse_edges("4px"), Some(Edges { top: 4, right: 4, bottom: 4, left: 4 }));
        assert_eq!(parse_edges("0 8px"), Some(Edges { top: 0, right: 8, bottom: 0, left: 8 }));
        assert_eq!(
            parse_edges("1 2 3 4"),
            Some(Edges { top: 1, right: 2, bottom: 3, left: 4 })
        );
        assert_eq!(parse_edges("1 2 3 4 5"), None);
        assert_eq!(parse_edges("bad"), None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib css 2>&1 | head -20`
Expected: compile error — `parse_color` etc. not defined.

- [ ] **Step 3: Write the implementation**

Add above the `#[cfg(test)]` block in `src/css.rs`:

```rust
/// Parse `#rgb` or `#rrggbb`.
pub fn parse_color(s: &str) -> Option<Color> {
    let hex = s.trim().strip_prefix('#')?;
    match hex.len() {
        3 => {
            let n = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok();
            let (r, g, b) = (n(0)?, n(1)?, n(2)?);
            Some(Color { r: r * 17, g: g * 17, b: b * 17 }) // 0xF -> 0xFF
        }
        6 => {
            let n = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
            Some(Color { r: n(0)?, g: n(2)?, b: n(4)? })
        }
        _ => None,
    }
}

/// Parse an integer pixel length, with or without a `px` suffix.
pub fn parse_px(s: &str) -> Option<i32> {
    let t = s.trim();
    let num = t.strip_suffix("px").unwrap_or(t).trim();
    num.parse::<i32>().ok()
}

/// Parse a font weight: `normal`, `bold`, or a `100`–`900` number.
pub fn parse_weight(s: &str) -> Option<i32> {
    match s.trim() {
        "normal" => Some(400),
        "bold" => Some(700),
        n => {
            let w = n.parse::<i32>().ok()?;
            (100..=900).contains(&w).then_some(w)
        }
    }
}

/// Parse 1–4 CSS-shorthand `px` values into T/R/B/L edges.
pub fn parse_edges(s: &str) -> Option<Edges> {
    let parts: Vec<i32> = s
        .split_whitespace()
        .map(parse_px)
        .collect::<Option<Vec<_>>>()?;
    let e = match parts.as_slice() {
        [a] => Edges { top: *a, right: *a, bottom: *a, left: *a },
        [a, b] => Edges { top: *a, right: *b, bottom: *a, left: *b },
        [a, b, c] => Edges { top: *a, right: *b, bottom: *c, left: *b },
        [a, b, c, d] => Edges { top: *a, right: *b, bottom: *c, left: *d },
        _ => return None,
    };
    Some(e)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib css 2>&1 | tail -20`
Expected: all css tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/css.rs
git commit -m "feat: CSS value parsers (color, px, weight, edges)"
```

---

### Task 7: CSS resolve/merge + apply to modules

**Files:**
- Modify: `src/css.rs` (add `resolve` + `apply`)
- Modify: `src/main.rs` (use `css::resolve` instead of `Style::default`)

**Interfaces:**
- Consumes: `parse_color`, `parse_px`, `parse_weight`, `parse_edges`, `Style::default`.
- Produces: `css::resolve(default_css: &HashMap<String, String>, module_css: &HashMap<String, String>) -> Style`.

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block in `src/css.rs` (add `use std::collections::HashMap;` inside the test module if not present):

```rust
    fn css(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn module_css_overrides_defaults() {
        let defaults = css(&[("color", "#d0d0d0"), ("font-size", "12px"), ("padding", "0 8px")]);
        let module = css(&[("color", "#7fdbb0"), ("font-weight", "bold")]);
        let style = resolve(&defaults, &module);

        assert_eq!(style.color, Color { r: 0x7f, g: 0xdb, b: 0xb0 }); // overridden
        assert_eq!(style.font_size, 12); // from defaults
        assert_eq!(style.font_weight, 700); // from module
        assert_eq!(style.padding, Edges { top: 0, right: 8, bottom: 0, left: 8 });
        assert_eq!(style.background, None); // never set
    }

    #[test]
    fn background_color_is_applied() {
        let style = resolve(&HashMap::new(), &css(&[("background-color", "#303040")]));
        assert_eq!(style.background, Some(Color { r: 0x30, g: 0x30, b: 0x40 }));
    }

    #[test]
    fn invalid_and_unknown_values_are_ignored() {
        // bad color keeps the default; unknown property is dropped
        let style = resolve(&HashMap::new(), &css(&[("color", "notacolor"), ("wobble", "3")]));
        assert_eq!(style.color, Style::default().color);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib css 2>&1 | head -20`
Expected: compile error — `resolve` not defined.

- [ ] **Step 3: Write the implementation**

Add above the `#[cfg(test)]` block in `src/css.rs`, and add `use std::collections::HashMap;` at the top of the file:

```rust
/// Merge top-level defaults then a module's own css into a resolved Style.
/// Module properties win. Invalid values and unknown properties are ignored
/// (with a warning) so one bad line never breaks the bar.
pub fn resolve(default_css: &HashMap<String, String>, module_css: &HashMap<String, String>) -> Style {
    let mut style = Style::default();
    apply(&mut style, default_css);
    apply(&mut style, module_css);
    style
}

fn apply(style: &mut Style, css: &HashMap<String, String>) {
    for (key, value) in css {
        match key.as_str() {
            "color" => set(parse_color(value), |c| style.color = c, key, value),
            "background-color" => set(parse_color(value), |c| style.background = Some(c), key, value),
            "font-family" => style.font_family = value.trim().to_string(),
            "font-size" => set(parse_px(value), |px| style.font_size = px, key, value),
            "font-weight" => set(parse_weight(value), |w| style.font_weight = w, key, value),
            "padding" => set(parse_edges(value), |e| style.padding = e, key, value),
            "margin" => set(parse_edges(value), |e| style.margin = e, key, value),
            other => eprintln!("vEnter: unknown css property '{other}' (ignored)"),
        }
    }
}

/// Apply a parsed value, or warn and leave the current value unchanged.
fn set<T>(parsed: Option<T>, mut assign: impl FnMut(T), key: &str, value: &str) {
    match parsed {
        Some(v) => assign(v),
        None => eprintln!("vEnter: invalid value '{value}' for css '{key}' (ignored)"),
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib css 2>&1 | tail -20`
Expected: all css tests pass.

- [ ] **Step 5: Switch main to resolved styles**

In `src/main.rs`, replace the Increment-1 styles line:

```rust
    let styles: Vec<css::Style> = ordered.iter().map(|_| css::Style::default()).collect();
```

with:

```rust
    let styles: Vec<css::Style> = ordered
        .iter()
        .map(|(_, m)| css::resolve(&cfg.css, &m.css))
        .collect();
```

- [ ] **Step 6: Build and run the full test suite**

Run: `cargo build 2>&1 | tail -20`
Expected: compiles.

Run: `cargo test 2>&1 | tail -20`
Expected: all unit tests pass.

- [ ] **Step 7: Verify styling visually**

Run the app, capture the taskbar, then stop it (same method as Task 5, Step 9). Expected: with `venter.json`, the `cpu` label is bold teal (`#7fdbb0`), the `clock` label is white, both using Segoe UI 12 px with `0 8px` padding, backgrounds transparent, parked left of TrafficMonitor. Stop with `taskkill //F //IM vEnter.exe`.

- [ ] **Step 8: Commit**

```bash
git add src/css.rs src/main.rs
git commit -m "feat: apply per-module CSS styling over top-level defaults"
```

---

## Done criteria

- `cargo test` green across `config`, `css`, `plugin`, and `layout` units.
- `venter.json` drives which modules appear, in what order, refreshed on their intervals.
- Each module renders its script's plain-text output, styled by its merged CSS, right-aligned and tracking the tray/TrafficMonitor.
- The taskbar window background stays transparent (color-key), and no console windows flash on plugin ticks.

After Task 7, use **superpowers:finishing-a-development-branch** to verify tests and choose how to land the `development` branch.
