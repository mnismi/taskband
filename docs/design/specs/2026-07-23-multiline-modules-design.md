# Multi-line Module Rendering — Design

**Date:** 2026-07-23
**Branch:** development
**Builds on:** the plugin system (`docs/superpowers/specs/2026-07-23-plugin-system-design.md`)

## Goal

Let a module render more than one line of text. A module's script (`exec`) can
print several `\n`-separated lines; each line is drawn stacked and vertically
centered within the module's slot on the taskbar strip. A new `text-align` CSS
property controls how the lines are aligned horizontally.

Single-line modules render exactly as they do today — this is a purely additive
change.

## Motivation

The taskbar strip is short (~40px), but two small stacked lines fit comfortably
(e.g. a label over a value, or a date over a time). This mirrors how many status
bars present compact two-line widgets.

## Behaviour

- The worker already delivers a module's trimmed stdout as a single `String`.
  Interior newlines survive `.trim()`, so no worker change is needed.
- At render time the text is split with Rust's `str::lines()`, which splits on
  `\n` and strips a trailing `\r` — so the `\r\n` that PowerShell emits is handled.
- The module's **width** is the width of its **widest** line (plus padding and
  margin, as now).
- The lines are stacked using the font's line height and the whole block is
  **vertically centered** within the strip. If the block is taller than the strip
  (too many lines / too large a font) it top-aligns and clips — no auto-shrink.
- Each line is aligned horizontally per the module's `text-align`.

## CSS: `text-align`

New property, values `left | center | right`, default `center`.

Default `center` is safe for existing configs: a single-line module's box is
exactly as wide as its one line, so left/center/right all render identically.
Alignment only becomes visible once a module has lines of differing lengths.

```jsonc
"weather": {
  "exec": "powershell -NoProfile -File weather.ps1",   // prints two lines
  "css": { "text-align": "center", "font-size": "10px" }
}
```

## Components / files

Three files, one of which is untouched:

### `src/css.rs`
- New `TextAlign { Left, Center, Right }` (Debug, Clone, Copy, PartialEq, Eq).
- `Style` gains `text_align: TextAlign`, defaulting to `Center`.
- `parse_text_align(&str) -> Option<TextAlign>` (`left`/`center`/`right`).
- `apply` handles the `"text-align"` key via the existing `set` helper (invalid
  values warn and are ignored, like every other property).
- Unit tests: parser accepts/rejects, and `text-align` overrides through `resolve`.

### `src/window.rs`
- `measure()`: iterate `text.lines()`, `DT_CALCRECT` each non-empty line, take the
  **max** text width; add padding/margin as now. Empty text → 0.
- `WM_PAINT`: obtain the font's line height via `GetTextMetricsW`
  (`tmHeight + tmExternalLeading`); compute `block_height = line_height ×
  line_count`; vertical top = `max(0, (strip_height − block_height) / 2)`; draw
  each non-empty line into a one-line rect at `top + j × line_height` with
  `DT_SINGLELINE | DT_VCENTER | <align flag>`. The alignment flag
  (`DT_LEFT`/`DT_CENTER`/`DT_RIGHT`) comes from `style.text_align`, so `DrawTextW`
  positions each line horizontally — no per-line width math.

### `src/plugin.rs`
- **No change.** `.trim()` preserves interior newlines; `.lines()` handles `\r\n`.

## Testing

- Pure unit tests in `css.rs` for the parser and the `resolve` override, matching
  the existing test style.
- The GDI measure/paint is Win32 glue, verified live with a screenshot (as with
  every prior increment) using a module whose `exec` prints two lines.

## Non-goals (YAGNI)

- Auto-shrinking the font to fit the strip.
- A second full row of modules (would need a separate floating window).
- Per-line styling (all lines in a module share the module's style).
