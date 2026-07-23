# vEnter plugin system — waybar-style modules

**Date:** 2026-07-23
**Status:** Design approved, pending implementation plan
**Type:** Feature on top of the taskbar-text spike + dynamic positioning
(see `2026-07-23-taskbar-text-spike-design.md`,
`2026-07-23-dynamic-positioning-design.md`)
**Branch:** `development`

## Goal

Turn the single hardcoded taskbar label into a **waybar-style plugin system**.
The user defines modules in a config file; each module runs a script/command on
an interval and its output is rendered as a label. Modules are reorderable, the
bar is always right-aligned (parked left of the tray / TrafficMonitor), and each
module is styleable with CSS properties.

This replaces the hardcoded `"vEnter ▲ hello"` text with a data-driven,
user-configurable bar.

## Decisions (from brainstorming)

- **Config format:** JSONC (JSON with comments, waybar-style). One config file.
- **Order:** a `modules-right` array; module definitions keyed by name.
- **Plugin output:** plain text — the script's trimmed stdout is the label. (JSON
  output / tooltips / state classes are deferred.)
- **Refresh:** interval-based polling; each module has an `interval` (seconds).
- **Styling:** genuine CSS **property** names/values under a `css` block. A
  top-level `css` provides defaults; each module's `css` overrides matching
  properties. No selector engine / cascade.
- **Layout:** always right-aligned; reuses the existing positioning logic.
- **Rendering:** keep the proven layered **color-key** transparency approach.

## Config schema

```jsonc
{
  // left-to-right as displayed; the last entry sits nearest the tray
  "modules-right": ["cpu", "clock"],

  "css": {                       // defaults applied to every module
    "font-family": "Segoe UI",
    "font-size": "12px",
    "color": "#d0d0d0",
    "padding": "0 8px"
  },

  "cpu": {
    "exec": "powershell -NoProfile -File scripts/cpu.ps1",
    "interval": 2,               // seconds
    "css": { "color": "#7fdbb0", "font-weight": "bold" }
  },
  "clock": {
    "exec": "powershell -NoProfile -c \"(Get-Date).ToString('HH:mm')\"",
    "interval": 1,
    "css": { "background-color": "#303040", "color": "#ffffff" }
  }
}
```

- A name listed in `modules-right` but absent from the object is skipped with a
  warning. An object key not listed in `modules-right` is simply not rendered.
- Config file location for v1: `venter.json` resolved next to the executable,
  falling back to the current working directory. (A proper config-dir search is
  deferred.)
- Comments and trailing commas are allowed (parsed via `json5`).

## Plugin execution model

Plugins must never run on the UI thread — a slow script would freeze the whole
taskbar window. Instead:

- A single **background worker thread** owns its own copy of the module list
  (`name`, `exec`, `interval`).
- It loops on a short tick (~100 ms). For each module whose `interval` has
  elapsed since its last run, it runs `exec` via `std::process::Command`,
  captures stdout, trims trailing whitespace/newlines, and sends
  `(module_index, text)` over an `mpsc::Sender`.
- The **UI timer** (the existing `WM_TIMER`, 250 ms) drains the `Receiver`,
  updates each module's cached text, and — only if any text changed — remeasures
  widths, recomputes the window width/position, and calls `InvalidateRect` to
  repaint.

This keeps rendering responsive and confines all cross-thread traffic to a
one-way channel (no shared mutex).

`exec` is run through the system shell — `cmd /C <exec>` via
`Command::new("cmd").raw_arg("/C ...")` — so quotes, pipes, and redirection in
the command line work exactly as written (like waybar's `sh -c`). The child is
spawned with `CREATE_NO_WINDOW` so no console window flashes on each tick.

## Styling model

Each module's resolved `Style` is `top-level css` merged with `module css`
(module wins per-property). Supported v1 properties, parsed by a small
hand-written parser:

| Property           | Accepted values                                   |
|--------------------|---------------------------------------------------|
| `color`            | `#rgb`, `#rrggbb`                                  |
| `background-color` | `#rgb`, `#rrggbb` (omit ⇒ transparent)            |
| `font-family`      | a family name string (e.g. `Segoe UI`)            |
| `font-size`        | `<n>px`                                            |
| `font-weight`      | `normal`, `bold`, or `100`–`900`                  |
| `padding`          | 1–4 CSS-shorthand `px` values (T R B L)           |
| `margin`           | 1–4 CSS-shorthand `px` values (T R B L)           |

- Unknown properties are ignored with a warning (forward-compatible).
- Values that fail to parse fall back to the inherited/default value with a
  warning; the bar never fails to render because of one bad property.
- The parser and the merge are **pure and unit-tested** (no Win32).

## Rendering & transparency

Keep the layered color-key approach established by the transparency change
(`SetLayeredWindowAttributes(hwnd, COLORREF(0x000000), 0, LWA_COLORKEY)`).
`WM_PAINT` loops the modules left-to-right across the window:

- Module **without** `background-color`: fill its rect with the key color
  (black) ⇒ that region is transparent, the taskbar shows through.
- Module **with** `background-color`: fill its rect with that color ⇒ opaque.
- Text: select the module's font (`CreateFontW` from family/size/weight), set
  `SetTextColor` to the module color, `SetBkMode(TRANSPARENT)`, and `DrawTextW`
  within the padded rect.

**Documented constraint:** pure black `#000000` is reserved as the transparency
key, so it cannot be used as a `background-color`. (Semi-transparent backgrounds
would require per-pixel alpha via `UpdateLayeredWindow`; that is deferred.)

## Layout

- Measure each module's content width with a GDI DC and its selected font
  (`DrawTextW` with `DT_CALCRECT` / `GetTextExtentPoint32W`), then add its
  horizontal padding and margin.
- The window's total width is the sum of module widths. Modules are placed
  left-to-right inside the window in `modules-right` order (so the array reads
  left→right as it appears on screen; the rightmost entry sits nearest the tray).
- The existing `compute_x` still parks the window's right edge `gap` px left of
  the nearest obstacle; only the width becomes dynamic (was a fixed 260 px).
- The **placement math** (given a list of module widths, produce per-module
  x-offsets and the total width) is a **pure, unit-tested** function.

## Components / files

- `config.rs` — serde types (`Config`, `ModuleConfig`) + JSONC load. New deps:
  `serde` (derive), `json5`.
- `css.rs` — CSS property parsing and defaults/override merge into a resolved
  `Style`. **Pure, unit-tested.**
- `plugin.rs` — the worker thread, interval-due scheduling, `Command` execution,
  and channel. The "which modules are due at time `now`" logic is a **pure,
  unit-tested** function.
- `layout.rs` — extend with the right-aligned placement math (**tested**); keep
  the existing `compute_x`.
- `window.rs` — per-module paint, the timer that drains the channel and
  repositions, and state held behind the window via `GWLP_USERDATA`.
- `main.rs` — load config → create window + spawn worker → embed → run loop.

## State ownership

A `State` struct (config, resolved styles, per-module cached text + measured
widths, the channel `Receiver`) is boxed and attached to the window with
`SetWindowLongPtrW(GWLP_USERDATA)`, retrieved in `wndproc`. The worker thread
holds only its own module list and the `Sender`.

## Testing

- **Pure units:** CSS value parsing (colors, `px` lengths, font-weight,
  shorthand padding/margin), defaults/override merge, right-aligned placement
  math, plugin due-scheduling, and JSONC config parsing to structs.
- **Win32 glue:** paint, timers, and threading verified visually and via the
  System.Drawing screen-capture method used throughout this project.

## Build plan (two increments)

1. **Config-driven plugins (no CSS):** load the config, run plugins on the worker
   thread, and render right-aligned plain-text modules with a single default
   font. Replaces the hardcoded label.
2. **CSS styling:** parse and merge `css` blocks and apply `color`,
   `background-color`, fonts, and padding/margin in paint + layout.

Each increment produces a working, testable bar on its own.

## Out of scope (deferred)

- Semi-transparent backgrounds (per-pixel alpha via `UpdateLayeredWindow`).
- JSON-output plugins, tooltips, and state-based CSS classes.
- `.class` / selector cascade beyond the top-level + per-module merge.
- Config hot-reload (watching the file) and re-embed after `explorer.exe`
  restart (`TaskbarCreated`).
- Borders / border-radius, left/center module groups.
- DPI scaling and secondary-monitor taskbars.

## Risks

- **Slow scripts:** mitigated by the worker thread; a hung script delays only its
  own module's updates, not the UI.
- **Color-key limitations:** no semi-transparent backgrounds and black reserved;
  acceptable for v1 and documented.
- **Shell dependency:** commands run via `cmd /C`; this is intended (matches
  waybar's `sh -c`) and lets users write normal command lines, pipes, and quotes.
- **Text measurement vs. painting** must use the same font, or widths won't match
  the drawn glyphs — the layout and paint code share one font-construction path.
