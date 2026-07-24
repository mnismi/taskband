# Time-Tinted Module Color — Design

**Date:** 2026-07-23
**Branch:** master
**Builds on:** the plugin system (`docs/superpowers/specs/2026-07-23-plugin-system-design.md`)

## Goal

Let a module's text color change automatically with the time of day. A new
optional `time-colors` property maps hour-of-day ranges to colors; at paint
time the current local hour picks the color. A clock module can render cyan in
the morning, white at midday, amber in the evening, and purple at night —
without any config edits or reloads.

Modules without `time-colors` render exactly as they do today — this is a
purely additive change.

## Motivation

Static per-module colors are frozen when the config loads. A value-reactive
color makes the bar glanceable: the clock's hue alone signals the time of day.
This is the first value-driven style mechanism; numeric-threshold coloring
(e.g. CPU% → green/orange/red) is a separate future model, out of scope here.

## Config surface

A new optional module-level property, `time-colors`, a sibling of
`exec`/`interval`/`css`:

```jsonc
"clock-tint": {
  "exec": "powershell -NoProfile -Command \"(Get-Date).ToString('HH:mm')\"",
  "interval": 1,
  "css": { "font-size": "14px", "font-weight": "bold" },
  "time-colors": [
    { "from": 6,  "color": "#8be9fd" },   // 06:00–11:59  cyan (morning)
    { "from": 12, "color": "#ffffff" },   // 12:00–17:59  white (day)
    { "from": 18, "color": "#ffb86c" },   // 18:00–21:59  amber (evening)
    { "from": 22, "color": "#bd93f9" }    // 22:00–05:59  purple (night)
  ]
}
```

## Behaviour

- `from` is an hour `0–23`. Ranges are **cyclic**: each entry owns hours from
  its `from` up to the next entry's `from`; the entry with the largest `from`
  wraps past midnight until the smallest. With the example above, `03:00` →
  purple (the `22` bucket wraps through midnight).
- Entries may appear in any order in the config; they are sorted by `from`
  during resolution. Granularity is per-hour.
- When `time-colors` is present and non-empty it overrides the module's text
  `color` only — background, font, padding, and alignment come from `css` as
  usual. When absent or empty, the static `color` applies unchanged.
- Invalid entries (bad hex color, `from` outside `0–23`) warn and are dropped,
  consistent with how invalid CSS values are handled. If *all* entries are
  invalid, the module falls back to its static color. Duplicate `from` values:
  the last one wins (warn).
- The hour comes from the **system clock** (`GetLocalTime`) at paint time —
  not from parsing the module's output text. This keeps the mechanism robust
  for any module (the existing clock prints the day-of-month first, which
  would defeat text parsing) and free of timezone math.
- The tint is evaluated fresh on every repaint. A bar repaints whenever one of
  its modules' text changes, so any module updating a few times per hour (a
  clock updates every second) picks up an hour-boundary color change within
  one interval. No extra forced repaints are added.

## Components / files

### `Cargo.toml`
- Add the `Win32_System_SystemInformation` feature (for `GetLocalTime`).

### `src/css.rs`
- `Style` gains `time_colors: Vec<(u8, Color)>`, default empty, **sorted by
  hour** after resolution.
- New pure function `pick_time_color(hour: u8, rules: &[(u8, Color)]) ->
  Option<Color>`: returns the color of the last rule whose `from <= hour`, or
  — when `hour` precedes every rule — the color of the **last** rule (cyclic
  wrap). Empty rules → `None`.
- Unit tests: each bucket, exact boundaries (`from` itself is inside its
  bucket), midnight wrap, single-entry list (colors all 24 hours), empty list.

### `src/config.rs`
- `ModuleConfig` gains `#[serde(rename = "time-colors", default)]
  time_colors: Vec<TimeColorRule>` with
  `struct TimeColorRule { from: u8, color: String }`.
- `resolve_list` (registry build) validates each rule — `parse_color` on the
  hex, `from <= 23` — warning and skipping invalid ones, sorts by `from`, and
  stores the result in the slot's `Style.time_colors`.
- Unit tests: parsing the property, invalid-entry skipping, absent property →
  empty vec.

### `src/window.rs`
- In `WM_PAINT`, per module: if `style.time_colors` is non-empty, call
  `GetLocalTime()` and `pick_time_color(hour)`; use that color for
  `SetTextColor`, else `style.color`. (One `GetLocalTime` call per paint is
  fine; hoist it outside the module loop.)

### `config.json`
- Add a `clock-tint` demo module (as above) to the `modules` list so the
  feature is visible in the default config.

## Testing

- Pure unit tests in `css.rs` (`pick_time_color`) and `config.rs` (parsing /
  validation), matching the existing test style.
- The `GetLocalTime` + GDI glue is Win32 code verified live with a screenshot,
  as with every prior increment. To see multiple tints without waiting for
  real hour boundaries, temporarily edit the config's `from` values around the
  current hour.

## Non-goals (YAGNI)

- Per-minute granularity or gradient blending between hour buckets.
- Tinting the background or other style properties.
- Numeric-threshold coloring keyed off the module's printed value (future,
  separate mechanism).
