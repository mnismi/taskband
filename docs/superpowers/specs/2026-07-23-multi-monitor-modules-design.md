# Multi-monitor Module Routing — Design

**Date:** 2026-07-23
**Branch:** development
**Builds on:** the plugin system and multi-line rendering
(`docs/superpowers/specs/2026-07-23-plugin-system-design.md`,
`docs/superpowers/specs/2026-07-23-multiline-modules-design.md`)

## Goal

Show vEnter on every monitor's taskbar, not just the primary, and let the config
**route different modules to different monitors**. A new `monitors` object in the
config assigns each monitor (by index) its own `modules-right` list. All of it is
driven from the config file.

A module shown on more than one monitor runs **once** and its text is mirrored
everywhere it appears; all monitors stay synchronized.

## Config schema

New optional `monitors` object, keyed by monitor index (as a string). Each entry
has its own `modules-right`. Module *definitions* and top-level `css` are
unchanged — only the routing is new.

```jsonc
{
  "monitors": {
    "0": { "modules-right": ["cpu", "clock"] },
    "1": { "modules-right": ["memory", "net"] }
  },
  "css": { "font-family": "Segoe UI", "font-size": "12px" },
  "cpu":   { "exec": "…", "interval": 2 },
  "clock": { "exec": "…", "interval": 1 },
  "memory":{ "exec": "…" },
  "net":   { "exec": "…" }
}
```

- **Backward compatible:** no `monitors` key → today's behavior: one bar with the
  top-level `modules-right` on the **primary** monitor.
- If both are present, `monitors` wins and the top-level `modules-right` is
  ignored (with a one-line warning).
- A monitor listed but with no taskbar, or an index that does not exist → warned
  and skipped. A module name with no definition → skipped (as today).

`RawConfig` gains `monitors: HashMap<String, MonitorConfig>` where
`MonitorConfig { modules_right: Vec<String> }`. It is a named field, so serde's
`#[serde(flatten)]` module map excludes it cleanly (named fields are matched
before the flatten catch-all).

## Data model — the `State` split

Today's `State` conflates *what to draw* with *where to draw*. Approach A splits
it into a shared registry and per-monitor bars.

- **`App`** (heap-allocated once) — shared pipeline + registry:
  - `rx, path, mtime` — update channel + hot-reload bookkeeping.
  - `texts: Vec<String>`, `styles: Vec<Style>` — **indexed by slot**, one slot per
    *uniquely-referenced* module.
  - `bars: Vec<Bar>`.
- **`Bar`** — one per monitor that has a taskbar:
  - `hwnd` (its child window), `taskbar` (parent hwnd).
  - `modules: Vec<usize>` — slot indices this monitor shows, in order (duplicates
    within a monitor are allowed and paint the same slot's text twice).
  - `widths, offsets, total_width` — this bar's own layout over its subset.

Rationale: the registry is *what to draw* (shared, computed once per module); the
bar is *where/which to draw* (per monitor). Each unit has one clear purpose and
can be reasoned about independently.

## Config build

`config::build` changes to return:

- a **registry**: `styles: Vec<Style>` and `specs: Vec<PluginSpec>` (one per
  uniquely-referenced module, deduped by name) plus a `name → slot` map;
- a `HashMap<usize, Vec<usize>>` mapping monitor index → ordered slot list.

Deduping: the first time a module name is referenced it is resolved (css + spec)
and assigned the next slot; later references reuse that slot. This makes a module
shared across monitors run once. `plugin.rs` is unchanged — the worker still
sends `Update { index /* slot */, text }`.

The legacy fallback (no `monitors` key) is applied at wiring time, once the
primary monitor's index is known: the top-level `modules-right` becomes the slot
list for the primary index.

## Monitor ↔ taskbar mapping (`taskbar.rs`)

- **Enumerate monitors:** `EnumDisplayMonitors` → ordered `HMONITOR`s;
  `GetMonitorInfoW` gives each monitor's rect and the `MONITORINFOF_PRIMARY`
  flag. **Index = enumeration order.**
- **Enumerate taskbars:** primary `Shell_TrayWnd`, plus a
  `FindWindowExW(None, prev, "Shell_SecondaryTrayWnd", None)` loop for the
  secondaries.
- **Map** each taskbar to a monitor index via `MonitorFromWindow(taskbar,
  MONITOR_DEFAULTTONEAREST)`, matched against the enumerated `HMONITOR`s.
- **Startup log** prints the detected monitors so the user can map an index to a
  physical screen:
  ```
  vEnter monitors:
    [0] 3440x1440 @ (0,0)      primary   taskbar: yes
    [1] 1920x1080 @ (3440,0)             taskbar: yes
  ```

## Windows, driver & lifecycle

- At startup, create **one child window per monitor that has a taskbar** and
  `embed_in_taskbar` it into that monitor's taskbar. Its `modules` list comes from
  config (empty ⇒ the bar paints nothing / is effectively hidden).
- **Single driver:** only the primary bar's window gets `SetTimer`. Its `WM_TIMER`
  runs the one global tick:
  1. hot-reload check (`maybe_reload`);
  2. drain `rx` into `App.texts`, tracking which slots changed;
  3. for each bar whose slots changed (or after a reload), relayout it;
  4. reposition **every** bar against its own taskbar;
  5. invalidate the changed bars.
- **`WM_PAINT`** per bar: find its `Bar` (match `hwnd`), paint its subset from the
  shared `texts`/`styles` and its own `offsets`/`widths`. The existing multi-line /
  text-align paint code is reused verbatim per module.
- **Reposition** already works on secondary taskbars — `layout::compute_x` has a
  `handles_nonzero_taskbar_left` test, and the obstacle scan naturally parks left
  of whatever the secondary taskbar shows (e.g. its corner clock), or at the far
  right when there is nothing.

### Ownership

`App` is heap-allocated once; each bar window's `GWLP_USERDATA` holds a shared
pointer to it. The app is single-threaded (one message loop), so `WM_PAINT` and
the driver's `WM_TIMER` never overlap and the shared raw pointer is sound — the
same pattern the current single-window code already uses. The driver window frees
the `Box<App>` and calls `PostQuitMessage` on `WM_DESTROY`; secondary windows
hold a non-owning pointer and free nothing.

## Error handling

- **"Show taskbar on all displays" is off** → no `Shell_SecondaryTrayWnd` exists →
  only the primary bar is created. If the config references other monitors, warn:
  *"monitor N has no taskbar — enable 'Show my taskbar on all displays'."*
- A parse failure on reload keeps the running config (unchanged from today).
- Hot-reload re-resolves each existing bar's module list; the **set of windows is
  fixed at startup** (monitor hotplug ⇒ restart).

## Testing

- **Unit tests (pure):** `config` parsing of the `monitors` map; `build` producing
  the shared registry + per-monitor slot lists — dedup across monitors, order
  preserved, duplicates within a monitor, undefined names skipped, legacy
  `modules-right` fallback, monitor-index string parsing.
- **Win32 glue** (monitor enumeration, taskbar mapping, second-window embedding)
  is verified live: the startup monitor log plus a screenshot showing bars on two
  monitors.

## Non-goals (YAGNI)

- Per-monitor DPI font scaling (height adapts via the taskbar rect; font size
  stays logical px).
- Dynamic monitor hotplug without a restart.
- `modules-left` / `modules-center` zones (still right-only).
- Per-monitor CSS overrides (all monitors share a module's style).
