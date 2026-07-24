# Multi-monitor Module Routing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render vEnter on every monitor's taskbar and let the config route different modules to different monitors, all from `venter.json`.

**Architecture:** Approach A from the spec. Collect the uniquely-referenced modules across all monitors into one shared registry (one worker, `texts`/`styles` indexed by slot). Each monitor with a taskbar gets its own child window (`Bar`) that paints only its subset of slots with its own layout. The primary monitor's window carries the single timer that drains updates and repaints/repositions all bars.

**Tech Stack:** Rust, `windows` crate 0.58 (Win32 GDI + WindowsAndMessaging + Graphics::Gdi monitor APIs), `serde` + `json5`.

## Global Constraints

- Windows-only; `windows` crate 0.58; no new dependencies.
- The crate is a **binary**, not a library: run tests with `cargo test` (never `cargo test --lib`).
- Config is JSON5 (`json5::from_str`); comments and trailing commas allowed.
- **Backward compatible:** a config with no `monitors` key behaves exactly as today — one bar with top-level `modules-right` on the primary monitor.
- Modules are right-aligned only (no `modules-left`/`modules-center`).
- Monitor index = `EnumDisplayMonitors` enumeration order.
- All existing tests must keep passing; every commit must compile.

---

### Task 1: Config — `monitors` schema

**Files:**
- Modify: `src/config.rs` (add `MonitorConfig`; add `monitors` field to `RawConfig`)
- Test: `src/config.rs` (`#[cfg(test)]` module)

**Interfaces:**
- Produces: `RawConfig.monitors: HashMap<String, MonitorConfig>`; `MonitorConfig { modules_right: Vec<String> }`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/config.rs`:

```rust
#[test]
fn parses_monitors_map() {
    let cfg = parse(
        r##"{
            "monitors": {
                "0": { "modules-right": ["cpu"] },
                "1": { "modules-right": ["clock", "net"] }
            },
            "cpu":   { "exec": "echo c" },
            "clock": { "exec": "echo t" },
            "net":   { "exec": "echo n" }
        }"##,
    )
    .expect("valid config");

    assert_eq!(cfg.monitors.get("0").unwrap().modules_right, vec!["cpu".to_string()]);
    assert_eq!(
        cfg.monitors.get("1").unwrap().modules_right,
        vec!["clock".to_string(), "net".to_string()]
    );
    // module definitions still flatten correctly alongside the `monitors` field
    assert!(cfg.modules.contains_key("net"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test parses_monitors_map`
Expected: FAIL to compile — `RawConfig` has no field `monitors`.

- [ ] **Step 3: Add the struct and field**

In `src/config.rs`, add the `monitors` field to `RawConfig` **before** the `#[serde(flatten)] modules` field (named fields must be declared for serde to exclude them from the flatten catch-all):

```rust
#[derive(Debug, Deserialize)]
pub struct RawConfig {
    #[serde(rename = "modules-right", default)]
    pub modules_right: Vec<String>,
    #[serde(default)]
    pub css: HashMap<String, String>,
    /// Per-monitor module routing, keyed by monitor index (as a string).
    #[serde(default)]
    pub monitors: HashMap<String, MonitorConfig>,
    /// Every remaining top-level key is a module definition, keyed by name.
    #[serde(flatten)]
    pub modules: HashMap<String, ModuleConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MonitorConfig {
    #[serde(rename = "modules-right", default)]
    pub modules_right: Vec<String>,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test parses_monitors_map`
Expected: PASS.

- [ ] **Step 5: Run the whole suite and commit**

Run: `cargo test`
Expected: all existing tests + the new one PASS.

```bash
git add src/config.rs
git commit -m "feat: parse per-monitor 'monitors' config map"
```

---

### Task 2: Config — registry builder with dedup + per-monitor slots

**Files:**
- Modify: `src/config.rs` (add `BuildResult`, `build_registry`, `slots_for_monitor`; keep old `build` intact)
- Test: `src/config.rs`

**Interfaces:**
- Consumes: `RawConfig` (Task 1), `crate::css::resolve`, `crate::plugin::PluginSpec`.
- Produces:
  - `pub struct BuildResult { pub styles: Vec<Style>, pub specs: Vec<PluginSpec>, pub monitors: HashMap<usize, Vec<usize>>, pub legacy: Vec<usize> }`
  - `pub fn build_registry(cfg: &RawConfig) -> BuildResult`
  - `pub fn slots_for_monitor(monitors: &HashMap<usize, Vec<usize>>, legacy: &[usize], index: usize, primary: bool) -> Vec<usize>`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/config.rs`:

```rust
#[test]
fn build_registry_dedups_modules_shared_across_monitors() {
    let cfg = parse(
        r##"{
            "monitors": {
                "0": { "modules-right": ["cpu", "clock"] },
                "1": { "modules-right": ["clock", "cpu"] }
            },
            "cpu":   { "exec": "echo c" },
            "clock": { "exec": "echo t" }
        }"##,
    )
    .expect("valid config");

    let b = build_registry(&cfg);
    // two unique modules -> two slots (each runs once)
    assert_eq!(b.specs.len(), 2);
    assert_eq!(b.styles.len(), 2);
    // first-seen order assigns slots: cpu=0, clock=1
    assert_eq!(b.monitors.get(&0).unwrap(), &vec![0, 1]);
    assert_eq!(b.monitors.get(&1).unwrap(), &vec![1, 0]);
    assert!(b.legacy.is_empty());
}

#[test]
fn build_registry_legacy_fallback_when_no_monitors_key() {
    let cfg = parse(
        r##"{
            "modules-right": ["cpu", "clock", "cpu"],
            "cpu":   { "exec": "echo c" },
            "clock": { "exec": "echo t" }
        }"##,
    )
    .expect("valid config");

    let b = build_registry(&cfg);
    assert!(b.monitors.is_empty());
    // duplicate "cpu" dedups to one slot but appears twice in the ordered list
    assert_eq!(b.specs.len(), 2);
    assert_eq!(b.legacy, vec![0, 1, 0]);
}

#[test]
fn build_registry_skips_undefined_modules() {
    let cfg = parse(
        r##"{
            "monitors": { "0": { "modules-right": ["cpu", "ghost"] } },
            "cpu": { "exec": "echo c" }
        }"##,
    )
    .expect("valid config");

    let b = build_registry(&cfg);
    assert_eq!(b.specs.len(), 1);
    assert_eq!(b.monitors.get(&0).unwrap(), &vec![0]); // "ghost" skipped
}

#[test]
fn slots_for_monitor_prefers_map_then_legacy() {
    let mut monitors = HashMap::new();
    monitors.insert(0usize, vec![0, 1]);
    let legacy = vec![2];

    // map present: listed monitor uses its entry; unlisted monitor -> empty
    assert_eq!(slots_for_monitor(&monitors, &legacy, 0, true), vec![0, 1]);
    assert_eq!(slots_for_monitor(&monitors, &legacy, 5, false), Vec::<usize>::new());

    // map empty: primary uses legacy, non-primary -> empty
    let empty: HashMap<usize, Vec<usize>> = HashMap::new();
    assert_eq!(slots_for_monitor(&empty, &legacy, 3, true), vec![2]);
    assert_eq!(slots_for_monitor(&empty, &legacy, 3, false), Vec::<usize>::new());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test build_registry`
Expected: FAIL to compile — `build_registry` / `slots_for_monitor` / `BuildResult` not defined.

- [ ] **Step 3: Implement the registry builder**

In `src/config.rs`, add (leave the existing `build` function untouched — it is removed in Task 4):

```rust
/// A resolved registry: per-slot styles/specs (each referenced module once),
/// plus each monitor's ordered slot list and the legacy (primary-only) list.
pub struct BuildResult {
    pub styles: Vec<crate::css::Style>,
    pub specs: Vec<crate::plugin::PluginSpec>,
    pub monitors: HashMap<usize, Vec<usize>>,
    pub legacy: Vec<usize>,
}

/// Resolve a list of module names into slot indices, registering each newly-seen
/// module (its style + spec) into the shared registry. Undefined names warn and
/// are skipped; a repeated name reuses its existing slot.
fn resolve_list(
    names: &[String],
    cfg: &RawConfig,
    styles: &mut Vec<crate::css::Style>,
    specs: &mut Vec<crate::plugin::PluginSpec>,
    slot_of: &mut HashMap<String, usize>,
) -> Vec<usize> {
    let mut slots = Vec::new();
    for name in names {
        if let Some(&slot) = slot_of.get(name) {
            slots.push(slot);
            continue;
        }
        match cfg.modules.get(name) {
            Some(m) => {
                let slot = styles.len();
                styles.push(crate::css::resolve(&cfg.css, &m.css));
                specs.push(crate::plugin::PluginSpec {
                    name: name.clone(),
                    exec: m.exec.clone(),
                    interval: std::time::Duration::from_secs(m.interval.max(1)),
                });
                slot_of.insert(name.clone(), slot);
                slots.push(slot);
            }
            None => eprintln!("vEnter: module '{name}' is not defined (skipped)"),
        }
    }
    slots
}

/// Build the shared registry plus per-monitor slot lists. When `monitors` is
/// present it wins (and top-level `modules-right` is ignored with a warning);
/// otherwise `legacy` holds the top-level `modules-right` slots for the primary.
pub fn build_registry(cfg: &RawConfig) -> BuildResult {
    let mut styles = Vec::new();
    let mut specs = Vec::new();
    let mut slot_of: HashMap<String, usize> = HashMap::new();

    let mut monitors = HashMap::new();
    for (key, mc) in &cfg.monitors {
        let Ok(index) = key.parse::<usize>() else {
            eprintln!("vEnter: monitor key '{key}' is not a valid index (skipped)");
            continue;
        };
        let slots = resolve_list(&mc.modules_right, cfg, &mut styles, &mut specs, &mut slot_of);
        monitors.insert(index, slots);
    }

    let legacy = if cfg.monitors.is_empty() {
        resolve_list(&cfg.modules_right, cfg, &mut styles, &mut specs, &mut slot_of)
    } else {
        if !cfg.modules_right.is_empty() {
            eprintln!("vEnter: 'monitors' is set; top-level 'modules-right' is ignored");
        }
        Vec::new()
    };

    BuildResult { styles, specs, monitors, legacy }
}

/// The slot list a monitor should display: its `monitors` entry when the map is
/// non-empty (empty for unlisted monitors), else the legacy list on the primary.
pub fn slots_for_monitor(
    monitors: &HashMap<usize, Vec<usize>>,
    legacy: &[usize],
    index: usize,
    primary: bool,
) -> Vec<usize> {
    if !monitors.is_empty() {
        monitors.get(&index).cloned().unwrap_or_default()
    } else if primary {
        legacy.to_vec()
    } else {
        Vec::new()
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test build_registry` then `cargo test slots_for_monitor`
Expected: PASS.

- [ ] **Step 5: Run the whole suite and commit**

Run: `cargo test`
Expected: all tests PASS (old `build` and its test still present and green).

```bash
git add src/config.rs
git commit -m "feat: registry builder with per-monitor slot lists and dedup"
```

---

### Task 3: Taskbar — monitor enumeration and taskbar mapping

**Files:**
- Modify: `src/taskbar.rs` (add `MonitorInfo`, `enumerate_monitors`, `find_secondary_taskbars`, `detect`, `monitor_log_line`)
- Test: `src/taskbar.rs`

**Interfaces:**
- Consumes: existing `find_taskbar() -> Result<HWND>`.
- Produces:
  - `pub struct MonitorInfo { pub index: usize, pub rect: RECT, pub primary: bool, pub hmonitor: HMONITOR, pub taskbar: Option<HWND> }`
  - `pub fn detect() -> Vec<MonitorInfo>`
  - `pub fn monitor_log_line(m: &MonitorInfo) -> String`

- [ ] **Step 1: Write the failing tests**

Replace the `tests` module in `src/taskbar.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::HMONITOR;

    #[test]
    fn find_taskbar_returns_a_handle() {
        let hwnd = find_taskbar().expect("Shell_TrayWnd should exist while explorer is running");
        assert_ne!(hwnd, HWND::default(), "taskbar handle should not be the null handle");
    }

    #[test]
    fn detects_exactly_one_primary_with_a_taskbar() {
        let monitors = detect();
        assert!(!monitors.is_empty(), "at least one monitor should be detected");
        assert_eq!(
            monitors.iter().filter(|m| m.primary).count(),
            1,
            "exactly one primary monitor"
        );
        let primary = monitors.iter().find(|m| m.primary).unwrap();
        assert!(primary.taskbar.is_some(), "the primary monitor has the Shell_TrayWnd taskbar");
    }

    #[test]
    fn monitor_log_line_formats_index_size_and_taskbar() {
        let m = MonitorInfo {
            index: 1,
            rect: RECT { left: 1920, top: 0, right: 3840, bottom: 1080 },
            primary: false,
            hmonitor: HMONITOR::default(),
            taskbar: None,
        };
        let line = monitor_log_line(&m);
        assert!(line.contains("[1]"), "line was: {line}");
        assert!(line.contains("1920x1080"), "line was: {line}");
        assert!(line.contains("@ (1920,0)"), "line was: {line}");
        assert!(line.contains("taskbar: no"), "line was: {line}");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test-threads=1 detects_exactly_one_primary_with_a_taskbar` (or `cargo test detects_exactly_one_primary`)
Expected: FAIL to compile — `MonitorInfo` / `detect` / `monitor_log_line` not defined.

- [ ] **Step 3: Implement enumeration + mapping**

Replace the top of `src/taskbar.rs` (imports + functions, keeping the `find_taskbar` fn) with:

```rust
use std::mem::size_of;

use windows::core::{w, Result};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFO,
    MONITORINFOF_PRIMARY, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, FindWindowW};

/// One display and the taskbar (if any) sitting on it. `index` is the monitor's
/// position in `EnumDisplayMonitors` order.
pub struct MonitorInfo {
    pub index: usize,
    pub rect: RECT,
    pub primary: bool,
    pub hmonitor: HMONITOR,
    pub taskbar: Option<HWND>,
}

/// Locate the native primary taskbar window (`Shell_TrayWnd`).
pub fn find_taskbar() -> Result<HWND> {
    // Safety: FindWindowW only queries the window manager.
    unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }
}

/// `EnumDisplayMonitors` callback: push one `MonitorInfo` per monitor.
unsafe extern "system" fn enum_proc(hmon: HMONITOR, _hdc: HDC, _rc: *mut RECT, data: LPARAM) -> BOOL {
    let monitors = &mut *(data.0 as *mut Vec<MonitorInfo>);
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    if GetMonitorInfoW(hmon, &mut mi).as_bool() {
        let index = monitors.len();
        monitors.push(MonitorInfo {
            index,
            rect: mi.rcMonitor,
            primary: mi.dwFlags & MONITORINFOF_PRIMARY != 0,
            hmonitor: hmon,
            taskbar: None,
        });
    }
    TRUE
}

/// All monitors in enumeration order (taskbars not yet attached).
fn enumerate_monitors() -> Vec<MonitorInfo> {
    let mut monitors: Vec<MonitorInfo> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_proc),
            LPARAM(&mut monitors as *mut Vec<MonitorInfo> as isize),
        );
    }
    monitors
}

/// Every secondary taskbar window (`Shell_SecondaryTrayWnd`), one per secondary
/// monitor when "show taskbar on all displays" is enabled.
fn find_secondary_taskbars() -> Vec<HWND> {
    let mut taskbars = Vec::new();
    unsafe {
        let mut prev: Option<HWND> = None;
        while let Ok(h) = FindWindowExW(None, prev, w!("Shell_SecondaryTrayWnd"), None) {
            if h == HWND::default() {
                break;
            }
            taskbars.push(h);
            prev = Some(h);
        }
    }
    taskbars
}

/// Detect all monitors and attach each taskbar (primary + secondaries) to the
/// monitor it sits on, via `MonitorFromWindow`.
pub fn detect() -> Vec<MonitorInfo> {
    let mut monitors = enumerate_monitors();

    let mut taskbars = Vec::new();
    if let Ok(primary) = find_taskbar() {
        if primary != HWND::default() {
            taskbars.push(primary);
        }
    }
    taskbars.extend(find_secondary_taskbars());

    for tb in taskbars {
        let hmon = unsafe { MonitorFromWindow(tb, MONITOR_DEFAULTTONEAREST) };
        if let Some(m) = monitors.iter_mut().find(|m| m.hmonitor == hmon) {
            m.taskbar = Some(tb);
        }
    }
    monitors
}

/// A one-line human-readable description of a monitor for the startup log.
pub fn monitor_log_line(m: &MonitorInfo) -> String {
    let w = m.rect.right - m.rect.left;
    let h = m.rect.bottom - m.rect.top;
    format!(
        "  [{}] {}x{} @ ({},{}){}   taskbar: {}",
        m.index,
        w,
        h,
        m.rect.left,
        m.rect.top,
        if m.primary { "   primary" } else { "          " },
        if m.taskbar.is_some() { "yes" } else { "no" },
    )
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test` (the monitor tests touch the real desktop; they run in-process fine)
Expected: `detects_exactly_one_primary_with_a_taskbar`, `monitor_log_line_formats_index_size_and_taskbar`, and `find_taskbar_returns_a_handle` PASS, along with all others.

- [ ] **Step 5: Commit**

```bash
git add src/taskbar.rs
git commit -m "feat: enumerate monitors and map taskbars to them"
```

---

### Task 4: Cutover — per-monitor bars, driver timer, wiring

This is the behavior change: split `State` into `App` + `Bar`, paint/measure/reposition per bar, drive all bars from the primary window's timer, wire `main.rs` to create a bar per monitor-with-a-taskbar, and delete the now-unused old `config::build`. Verified live (Win32 glue).

**Files:**
- Rewrite: `src/window.rs`
- Rewrite: `src/main.rs`
- Modify: `src/config.rs` (delete old `build` + its `build_orders_modules_and_repeats_duplicates` test)

**Interfaces:**
- Consumes: `config::build_registry`, `config::slots_for_monitor`, `config::BuildResult` (Task 2); `taskbar::detect`, `taskbar::MonitorInfo`, `taskbar::monitor_log_line` (Task 3); `plugin::spawn_worker`, `plugin::Update`; `layout::place_modules`, `layout::compute_x`; `css::Style`, `css::TextAlign`.
- Produces (from `window.rs`):
  - `pub struct Bar` with `Bar::new(hwnd, taskbar, monitor_index, primary, modules) -> Bar` and `Bar::hwnd(&self) -> HWND`
  - `pub struct App` with `App::new(styles, rx, path, bars) -> App`
  - `pub fn register_class() -> Result<HINSTANCE>`
  - `pub fn create_bar_window(instance: HINSTANCE) -> Result<HWND>`
  - `pub fn embed_in_taskbar(child: HWND, taskbar: HWND) -> Result<()>`
  - `pub fn install(app: App, driver: HWND)`
  - `pub fn run_message_loop()`

- [ ] **Step 1: Replace `src/window.rs` entirely**

Write `src/window.rs` with exactly this content:

```rust
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::SystemTime;

use windows::core::{w, PCWSTR, Result};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, TRUE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, GetDC,
    GetTextMetricsW, InvalidateRect, ReleaseDC, SelectObject, SetBkMode, SetTextColor,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DEFAULT_QUALITY, DRAW_TEXT_FORMAT,
    DT_CALCRECT, DT_CENTER, DT_LEFT, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, HDC, HFONT,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, TEXTMETRICW, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW, GetWindow,
    GetWindowLongPtrW, GetWindowRect, IsWindowVisible, KillTimer, LoadCursorW, PostQuitMessage,
    RegisterClassW, SetLayeredWindowAttributes, SetParent, SetTimer, SetWindowLongPtrW,
    SetWindowPos, TranslateMessage, GWLP_USERDATA, GWL_STYLE, GW_CHILD, GW_HWNDNEXT, HWND_TOP,
    IDC_ARROW, LWA_COLORKEY, MSG, SWP_SHOWWINDOW, WINDOW_STYLE, WM_DESTROY, WM_PAINT, WM_TIMER,
    WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_POPUP, WS_VISIBLE,
};

use crate::css::{Style, TextAlign};
use crate::plugin::Update;

const TIMER_ID: usize = 1;
const TIMER_MS: u32 = 250;
const GAP: i32 = 8;

/// One monitor's bar: its layered child window on that monitor's taskbar plus the
/// layout of the module slots it shows. Slots index into `App::texts`/`styles`.
pub struct Bar {
    hwnd: HWND,
    taskbar: HWND,
    monitor_index: usize,
    primary: bool,
    modules: Vec<usize>,
    widths: Vec<i32>,
    offsets: Vec<i32>,
    total_width: i32,
}

impl Bar {
    pub fn new(hwnd: HWND, taskbar: HWND, monitor_index: usize, primary: bool, modules: Vec<usize>) -> Self {
        let n = modules.len();
        Bar {
            hwnd,
            taskbar,
            monitor_index,
            primary,
            modules,
            widths: vec![0; n],
            offsets: vec![0; n],
            total_width: 0,
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

/// Shared UI-thread render state: per-slot texts/styles plus every monitor's bar.
/// Only the UI thread touches it; the worker thread only sends `Update`s.
pub struct App {
    texts: Vec<String>,
    styles: Vec<Style>,
    rx: Receiver<Update>,
    path: PathBuf,
    mtime: Option<SystemTime>,
    bars: Vec<Bar>,
}

impl App {
    pub fn new(styles: Vec<Style>, rx: Receiver<Update>, path: PathBuf, bars: Vec<Bar>) -> Self {
        let mtime = file_mtime(&path);
        App {
            texts: vec![String::new(); styles.len()],
            styles,
            rx,
            path,
            mtime,
            bars,
        }
    }
}

/// Last-modified time of a file, or None if it can't be read.
fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// If the config changed on disk, re-parse it and rebuild the registry + each
/// bar's slot list with a fresh worker. A config that fails to parse is ignored,
/// keeping the running config. Returns true if a reload was applied.
unsafe fn maybe_reload(app: &mut App) -> bool {
    let current = file_mtime(&app.path);
    if current == app.mtime {
        return false;
    }
    app.mtime = current;

    let cfg = match crate::config::load(&app.path) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("vEnter: reload skipped — {e}");
            return false;
        }
    };

    let build = crate::config::build_registry(&cfg);
    let n = build.styles.len();
    // Spawn the new worker first; assigning its receiver drops the old one,
    // which makes the old worker exit on its next send.
    let rx = crate::plugin::spawn_worker(build.specs);
    app.styles = build.styles;
    app.texts = vec![String::new(); n];
    app.rx = rx;
    for bar in &mut app.bars {
        let slots =
            crate::config::slots_for_monitor(&build.monitors, &build.legacy, bar.monitor_index, bar.primary);
        let m = slots.len();
        bar.modules = slots;
        bar.widths = vec![0; m];
        bar.offsets = vec![0; m];
        bar.total_width = 0;
    }
    println!("vEnter: reloaded config — {n} module slot(s).");
    true
}

/// Build a GDI font from a resolved style. Caller must DeleteObject it.
unsafe fn make_font(style: &Style) -> HFONT {
    let mut face: Vec<u16> = style.font_family.encode_utf16().collect();
    face.push(0);
    CreateFontW(
        -style.font_size,
        0,
        0,
        0,
        style.font_weight,
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        DEFAULT_QUALITY.0 as u32,
        (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
        PCWSTR(face.as_ptr()),
    )
}

/// The DrawTextW horizontal-alignment flag for a resolved text alignment.
fn align_flag(align: TextAlign) -> DRAW_TEXT_FORMAT {
    match align {
        TextAlign::Left => DT_LEFT,
        TextAlign::Center => DT_CENTER,
        TextAlign::Right => DT_RIGHT,
    }
}

/// Measure a module's full width: widest line's extent + horizontal padding +
/// margin. For multi-line text the module is as wide as its longest line.
unsafe fn measure(hdc: HDC, style: &Style, text: &str) -> i32 {
    let font = make_font(style);
    let old = SelectObject(hdc, font);
    let mut text_w = 0;
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let mut utf16: Vec<u16> = line.encode_utf16().collect();
        let mut r = RECT::default();
        DrawTextW(hdc, &mut utf16, &mut r, DT_CALCRECT | DT_SINGLELINE | DT_LEFT);
        text_w = text_w.max(r.right - r.left);
    }
    SelectObject(hdc, old);
    let _ = DeleteObject(font);
    text_w + style.padding.left + style.padding.right + style.margin.left + style.margin.right
}

/// Re-measure one bar's modules against current text and recompute its layout.
unsafe fn relayout_bar(bar: &mut Bar, texts: &[String], styles: &[Style]) {
    let hdc = GetDC(bar.hwnd);
    bar.widths.clear();
    for &slot in &bar.modules {
        bar.widths.push(measure(hdc, &styles[slot], &texts[slot]));
    }
    ReleaseDC(bar.hwnd, hdc);
    let (offsets, total) = crate::layout::place_modules(&bar.widths);
    bar.offsets = offsets;
    bar.total_width = total;
}

/// Register the window class once. Returns the module instance for CreateWindow.
pub fn register_class() -> Result<HINSTANCE> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("vEnterTaskbarWindow"),
            hbrBackground: CreateSolidBrush(COLORREF(0x0000_0000)), // black = transparent key
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        Ok(instance.into())
    }
}

/// Create one layered bar window (class must already be registered).
pub fn create_bar_window(instance: HINSTANCE) -> Result<HWND> {
    unsafe {
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
        Ok(hwnd)
    }
}

/// Reparent `child` into `taskbar` and switch it to a child window.
pub fn embed_in_taskbar(child: HWND, taskbar: HWND) -> Result<()> {
    unsafe {
        SetParent(child, taskbar)?;
        let current = WINDOW_STYLE(GetWindowLongPtrW(child, GWL_STYLE) as u32);
        let child_style = (current & !WS_POPUP) | WS_CHILD | WS_VISIBLE;
        SetWindowLongPtrW(child, GWL_STYLE, child_style.0 as isize);
        Ok(())
    }
}

/// Take ownership of `app`, point every bar window at it, and start the single
/// driver timer on the `driver` window (the primary monitor's bar).
pub fn install(app: App, driver: HWND) {
    unsafe {
        let ptr = Box::into_raw(Box::new(app));
        for bar in &(*ptr).bars {
            SetWindowLongPtrW(bar.hwnd, GWLP_USERDATA, ptr as isize);
        }
        SetTimer(driver, TIMER_ID, TIMER_MS, None);
    }
}

/// Recompute where one bar should sit (just left of its taskbar's tray / embedded
/// apps) and move it there only if something changed.
fn reposition(bar: &Bar) {
    unsafe {
        let width = bar.total_width;
        let mut tb = RECT::default();
        if GetWindowRect(bar.taskbar, &mut tb).is_err() {
            return;
        }
        let taskbar_left = tb.left;
        let taskbar_width = tb.right - tb.left;
        let tb_height = tb.bottom - tb.top;

        // Obstacle = a visible sibling in the right half that is not full-width
        // (excludes the full-width XAML content bridge) and not our own window.
        let mut obstacles: Vec<i32> = Vec::new();
        let mut sib = GetWindow(bar.taskbar, GW_CHILD).ok();
        while let Some(h) = sib {
            if h != bar.hwnd && IsWindowVisible(h).as_bool() {
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
        if GetWindowRect(bar.hwnd, &mut cur).is_err() {
            return;
        }
        let cur_x = cur.left - taskbar_left;
        let cur_w = cur.right - cur.left;
        let cur_h = cur.bottom - cur.top;
        if cur_x != x || cur_w != width || cur_h != tb_height {
            let _ = SetWindowPos(bar.hwnd, HWND_TOP, x, 0, width, tb_height, SWP_SHOWWINDOW);
        }
    }
}

/// Blocking Win32 message loop. Returns when the driver window is destroyed.
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

                let app_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const App;
                if !app_ptr.is_null() {
                    let app = &*app_ptr;
                    if let Some(bar) = app.bars.iter().find(|b| b.hwnd == hwnd) {
                        let mut client = RECT::default();
                        let _ = GetClientRect(hwnd, &mut client);
                        let height = client.bottom - client.top;

                        for i in 0..bar.modules.len() {
                            let slot = bar.modules[i];
                            let style = &app.styles[slot];
                            let x0 = bar.offsets[i];
                            let w = bar.widths[i];

                            let left = x0 + style.margin.left;
                            let right = x0 + w - style.margin.right;
                            let mrect = RECT { left, top: 0, right, bottom: height };

                            let bg = match style.background {
                                Some(c) => c.colorref(),
                                None => 0x0000_0000,
                            };
                            let brush = CreateSolidBrush(COLORREF(bg));
                            FillRect(hdc, &mrect, brush);
                            let _ = DeleteObject(brush);

                            let font = make_font(style);
                            let old = SelectObject(hdc, font);
                            SetBkMode(hdc, TRANSPARENT);
                            SetTextColor(hdc, COLORREF(style.color.colorref()));

                            let mut tm = TEXTMETRICW::default();
                            let line_h = if GetTextMetricsW(hdc, &mut tm).as_bool() {
                                tm.tmHeight + tm.tmExternalLeading
                            } else {
                                style.font_size
                            };

                            let text_left = left + style.padding.left;
                            let text_right = right - style.padding.right;
                            let lines: Vec<&str> = app.texts[slot].lines().collect();
                            let block_h = line_h * lines.len() as i32;
                            let mut y = ((height - block_h) / 2).max(0);
                            let flags = align_flag(style.text_align) | DT_VCENTER | DT_SINGLELINE;
                            for line in lines {
                                if !line.is_empty() {
                                    let mut lrect = RECT {
                                        left: text_left,
                                        top: y,
                                        right: text_right,
                                        bottom: y + line_h,
                                    };
                                    let mut utf16: Vec<u16> = line.encode_utf16().collect();
                                    DrawTextW(hdc, &mut utf16, &mut lrect, flags);
                                }
                                y += line_h;
                            }
                            SelectObject(hdc, old);
                            let _ = DeleteObject(font);
                        }
                    }
                }

                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_TIMER => {
                let app_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
                if !app_ptr.is_null() {
                    let app = &mut *app_ptr;
                    // Reload first, so `changed` is sized to the current texts.
                    let reloaded = maybe_reload(app);
                    let mut changed = vec![false; app.texts.len()];
                    while let Ok(update) = app.rx.try_recv() {
                        if update.index < app.texts.len() && app.texts[update.index] != update.text {
                            app.texts[update.index] = update.text;
                            changed[update.index] = true;
                        }
                    }
                    let App { texts, styles, bars, .. } = app;
                    for bar in bars.iter_mut() {
                        let affected = reloaded
                            || bar.modules.iter().any(|&s| changed.get(s).copied().unwrap_or(false));
                        if affected {
                            relayout_bar(bar, texts.as_slice(), styles.as_slice());
                        }
                        reposition(bar);
                        if affected {
                            let _ = InvalidateRect(bar.hwnd, None, TRUE);
                        }
                    }
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                let app_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
                if !app_ptr.is_null() {
                    let app = Box::from_raw(app_ptr);
                    // Null every bar's back-pointer (incl. self) so a sibling's later
                    // WM_DESTROY is a no-op, and stop the driver timer.
                    for bar in &app.bars {
                        let _ = KillTimer(bar.hwnd, TIMER_ID);
                        SetWindowLongPtrW(bar.hwnd, GWLP_USERDATA, 0);
                    }
                    drop(app); // drops rx (worker stops)
                }
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
```

- [ ] **Step 2: Replace `src/main.rs` entirely**

Write `src/main.rs` with exactly this content:

```rust
mod config;
mod css;
mod layout;
mod plugin;
mod taskbar;
mod window;

fn main() -> windows::core::Result<()> {
    let path = config::config_path();
    let cfg = match config::load(&path) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("vEnter: {e}");
            std::process::exit(1);
        }
    };

    let monitors = taskbar::detect();
    println!("vEnter monitors:");
    for m in &monitors {
        println!("{}", taskbar::monitor_log_line(m));
    }

    let build = config::build_registry(&cfg);
    let slot_count = build.styles.len();

    // Warn about monitors named in the config that can't host a bar.
    for &idx in build.monitors.keys() {
        match monitors.iter().find(|m| m.index == idx) {
            Some(m) if m.taskbar.is_none() => eprintln!(
                "vEnter: monitor {idx} has no taskbar — enable 'Show my taskbar on all displays'."
            ),
            None => eprintln!("vEnter: monitor {idx} does not exist (skipped)."),
            _ => {}
        }
    }

    let rx = plugin::spawn_worker(build.specs);
    let instance = window::register_class()?;

    let mut bars = Vec::new();
    let mut driver = None;
    for m in &monitors {
        let Some(taskbar) = m.taskbar else {
            continue;
        };
        let slots = config::slots_for_monitor(&build.monitors, &build.legacy, m.index, m.primary);
        let hwnd = window::create_bar_window(instance)?;
        window::embed_in_taskbar(hwnd, taskbar)?;
        if m.primary {
            driver = Some(hwnd);
        }
        bars.push(window::Bar::new(hwnd, taskbar, m.index, m.primary, slots));
    }

    if bars.is_empty() {
        eprintln!("vEnter: no taskbars found; nothing to display.");
        std::process::exit(1);
    }
    let driver = driver.unwrap_or_else(|| bars[0].hwnd());
    let bar_count = bars.len();

    let app = window::App::new(build.styles, rx, path, bars);
    window::install(app, driver);
    println!(
        "vEnter embedded on {bar_count} monitor(s), {slot_count} module slot(s). Edit venter.json to reload live."
    );
    window::run_message_loop();
    Ok(())
}
```

- [ ] **Step 3: Delete the now-unused old `build` and its test**

In `src/config.rs`, delete the entire old `pub fn build(cfg: &RawConfig) -> (Vec<Style>, Vec<PluginSpec>)` function (the one that returns a tuple; keep `build_registry`). Then delete the `build_orders_modules_and_repeats_duplicates` test from the `tests` module (its behavior is now covered by `build_registry_*` tests).

- [ ] **Step 4: Build and run the whole test suite**

Run: `cargo build`
Expected: compiles with no errors (a few `unused` warnings are acceptable).

Run: `cargo test`
Expected: all tests PASS — `css` (11), `config` (parse/monitors/registry/slots), `taskbar` (3), `layout` (7), `plugin` (3). No reference to the deleted `build`.

- [ ] **Step 5: Live-verify multiple monitors**

Create a throwaway two-monitor config next to the release binary (gitignored, does not touch the user's `venter.json`):

```bash
cargo build --release
cat > target/release/venter.json <<'JSON'
{
  "monitors": {
    "0": { "modules-right": ["cpu", "clock"] },
    "1": { "modules-right": ["mem", "clock"] }
  },
  "css": { "font-family": "Segoe UI", "font-size": "12px", "color": "#d0d0d0", "padding": "0 8px" },
  "cpu":   { "exec": "powershell -NoProfile -Command \"'CPU ' + (Get-CimInstance Win32_Processor).LoadPercentage + '%'\"", "interval": 2, "css": { "color": "#7fdbb0", "font-weight": "bold" } },
  "mem":   { "exec": "powershell -NoProfile -Command \"$o=Get-CimInstance Win32_OperatingSystem; 'MEM ' + [int](100-100*$o.FreePhysicalMemory/$o.TotalVisibleMemorySize) + '%'\"", "interval": 2, "css": { "color": "#f0c674", "font-weight": "bold" } },
  "clock": { "exec": "powershell -NoProfile -Command \"(Get-Date).ToString('HH:mm:ss')\"", "interval": 1, "css": { "color": "#ffffff" } }
}
JSON
```

Start it in the background, then capture the whole virtual desktop to a PNG and inspect it:

Run (PowerShell): launch `target/release/venter.exe`, wait ~2s, then
```powershell
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
$b = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
$out = "$env:TEMP\venter_multimon.png"
$bmp.Save($out); $g.Dispose(); $bmp.Dispose()
$out
```
Then `Read` the saved PNG.

Expected: the console prints the `vEnter monitors:` list (one `[i] …` line per monitor, one marked `primary`, taskbars `yes`). The screenshot shows the CPU+clock bar on monitor 0's taskbar and the MEM+clock bar on monitor 1's taskbar, each parked just left of that taskbar's tray/clock; the `clock` value matches on both (shared slot). Stop the process when done.

- [ ] **Step 6: Commit**

```bash
git add src/window.rs src/main.rs src/config.rs
git commit -m "feat: render a per-monitor bar on each taskbar with routed modules"
```

---

## Self-Review

**Spec coverage:**
- Config schema (`monitors` keyed by index, backward-compat, precedence, skip rules) → Task 1 (parse) + Task 2 (`build_registry`, `slots_for_monitor`) + Task 4 (`main` precedence warnings).
- Data model split (`App` shared registry / `Bar` per-monitor) → Task 4 (`window.rs`).
- Config build (registry, dedup, `Update{slot}`, legacy fallback at wiring) → Task 2 + Task 4 (`main`).
- Monitor↔taskbar mapping + startup log → Task 3 (`detect`, `monitor_log_line`) + Task 4 (`main` prints them).
- Windows/driver/lifecycle (one window per monitor-with-taskbar, single driver timer, per-bar paint/reposition, ownership) → Task 4 (`window.rs` `install`/`wndproc`, `main` driver selection).
- Error handling ("taskbar on all displays" off, undefined module, parse-fail keeps config) → Task 2 (`resolve_list` skip), Task 4 (`main` warnings; `maybe_reload` keeps config).
- Testing (config unit tests + live screenshot) → Tasks 1–3 unit tests; Task 4 Step 5 live check.
- Non-goals (DPI, hotplug, left/center, per-monitor CSS) → not implemented, by design.

**Placeholder scan:** No TBD/TODO; every code step shows complete code; every command has expected output.

**Type consistency:** `build_registry`/`slots_for_monitor`/`BuildResult` signatures match between Task 2 (definition), Task 4 `window.rs` (`maybe_reload`), and Task 4 `main.rs`. `Bar::new(hwnd, taskbar, monitor_index, primary, modules)` and `Bar::hwnd()` and `App::new(styles, rx, path, bars)` and `register_class() -> HINSTANCE` / `create_bar_window(HINSTANCE)` / `install(App, HWND)` are used consistently in `main.rs`. `MonitorInfo` fields (`index`, `rect`, `primary`, `hmonitor`, `taskbar`) match between Task 3 definition and its tests and Task 4 usage.
