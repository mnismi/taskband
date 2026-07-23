# Time-Tinted Module Color Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new optional `time-colors` module property that changes the module's text color automatically by hour of day, chosen at paint time from the system clock.

**Architecture:** `Style` (in `src/css.rs`) gains a sorted `time_colors: Vec<(u8, Color)>` list plus a pure `pick_time_color` lookup with cyclic midnight wrap. `src/config.rs` parses and validates the JSON property into that list during registry build. `src/window.rs` calls `GetLocalTime()` once per `WM_PAINT` and tints each module that has rules; everything else renders unchanged.

**Tech Stack:** Rust, `windows` crate 0.58 (Win32 GDI + `GetLocalTime`), serde/json5.

**Spec:** `docs/superpowers/specs/2026-07-23-time-tinted-modules-design.md`

## Global Constraints

- Purely additive: modules without `time-colors` must render byte-for-byte as today.
- `time-colors` overrides the text color only — background, font, padding, alignment untouched.
- Invalid rule entries (hour > 23, unparseable color) warn via `eprintln!("Winbar: ...")` and are skipped; duplicate `from` hours keep the last (warn); all-invalid → fall back to static `color`.
- Hour source is `GetLocalTime()` at paint time, never the module's output text.
- No new crates; only the `Win32_System_SystemInformation` feature is added to the existing `windows` dependency.
- Match existing code style: doc comments on public items, tests in `#[cfg(test)] mod tests` at the bottom of each file.

---

### Task 1: `pick_time_color` and `Style.time_colors` in `src/css.rs`

**Files:**
- Modify: `src/css.rs` (Style struct ~line 33-58, new function after `parse_edges` ~line 120, tests at bottom)

**Interfaces:**
- Consumes: existing `Color` struct (`src/css.rs:4`).
- Produces:
  - `Style.time_colors: Vec<(u8, Color)>` — public field, default empty, expected sorted ascending by hour.
  - `pub fn pick_time_color(hour: u8, rules: &[(u8, Color)]) -> Option<Color>` — last rule with `from <= hour`; if `hour` precedes all rules, the final (largest-`from`) rule wraps past midnight; `None` on empty rules.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module at the bottom of `src/css.rs`:

```rust
    #[test]
    fn pick_time_color_selects_bucket_and_wraps_midnight() {
        let cyan = Color { r: 0x8b, g: 0xe9, b: 0xfd };
        let white = Color { r: 0xff, g: 0xff, b: 0xff };
        let amber = Color { r: 0xff, g: 0xb8, b: 0x6c };
        let purple = Color { r: 0xbd, g: 0x93, b: 0xf9 };
        let rules = [(6, cyan), (12, white), (18, amber), (22, purple)];

        assert_eq!(pick_time_color(6, &rules), Some(cyan)); // boundary hour is inside its bucket
        assert_eq!(pick_time_color(11, &rules), Some(cyan));
        assert_eq!(pick_time_color(12, &rules), Some(white));
        assert_eq!(pick_time_color(21, &rules), Some(amber));
        assert_eq!(pick_time_color(23, &rules), Some(purple));
        assert_eq!(pick_time_color(0, &rules), Some(purple)); // wraps past midnight
        assert_eq!(pick_time_color(5, &rules), Some(purple));
    }

    #[test]
    fn pick_time_color_single_and_empty_rules() {
        let red = Color { r: 0xff, g: 0x00, b: 0x00 };
        // a single entry colors all 24 hours
        assert_eq!(pick_time_color(0, &[(9, red)]), Some(red));
        assert_eq!(pick_time_color(9, &[(9, red)]), Some(red));
        assert_eq!(pick_time_color(23, &[(9, red)]), Some(red));
        assert_eq!(pick_time_color(12, &[]), None);
    }

    #[test]
    fn style_default_has_no_time_colors() {
        assert!(Style::default().time_colors.is_empty());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib css`
Expected: compile error — `pick_time_color` not found and `Style` has no field `time_colors`.

- [ ] **Step 3: Implement**

In `src/css.rs`, add the field to `Style` (after `text_align: TextAlign,`):

```rust
    /// Hour→color tint rules, sorted ascending by hour. Empty = no tinting;
    /// see [`pick_time_color`].
    pub time_colors: Vec<(u8, Color)>,
```

Add to `Default for Style` (after `text_align: TextAlign::Center,`):

```rust
            time_colors: Vec::new(),
```

Add the function after `parse_edges` (before `resolve`):

```rust
/// Pick the tint for `hour` from rules sorted ascending by hour: the last rule
/// whose hour <= `hour`. When `hour` precedes every rule, the final rule wraps
/// past midnight (e.g. a 22:00 rule still applies at 03:00). Empty rules -> None.
pub fn pick_time_color(hour: u8, rules: &[(u8, Color)]) -> Option<Color> {
    let last = rules.last()?;
    let rule = rules.iter().rev().find(|(from, _)| *from <= hour).unwrap_or(last);
    Some(rule.1)
}
```

Note: `Style` derives `PartialEq, Eq` — `Vec<(u8, Color)>` satisfies both since `Color` is `Eq`, so the derives need no change.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib`
Expected: all tests pass, including the three new ones. (Running the full lib suite also proves the added field didn't break `config.rs`/`window.rs` — they construct `Style` only via `Default`/`resolve`.)

- [ ] **Step 5: Commit**

```bash
git add src/css.rs
git commit -m "feat: add time-of-day color rules to Style with cyclic lookup"
```

---

### Task 2: Parse and validate `time-colors` in `src/config.rs`

**Files:**
- Modify: `src/config.rs` (ModuleConfig ~line 30-37, new struct + helper, wire into `resolve_list` ~line 96-105, tests at bottom)

**Interfaces:**
- Consumes: `crate::css::parse_color` (`src/css.rs`), `crate::css::Color`, `Style.time_colors` from Task 1.
- Produces:
  - `ModuleConfig.time_colors: Vec<TimeColorRule>` (serde name `time-colors`, default empty).
  - `pub struct TimeColorRule { pub from: u8, pub color: String }` (Deserialize, Debug, Clone).
  - `build_registry` output: each slot's `Style.time_colors` holds the validated, sorted `(hour, Color)` pairs.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module at the bottom of `src/config.rs`:

```rust
    #[test]
    fn time_colors_parse_sorted_into_style() {
        let cfg = parse(
            r##"{
                "modules": ["clock"],
                "clock": {
                    "exec": "echo t",
                    "time-colors": [
                        { "from": 22, "color": "#bd93f9" },
                        { "from": 6,  "color": "#8be9fd" }
                    ]
                }
            }"##,
        )
        .expect("valid config");

        let b = build_registry(&cfg);
        // sorted ascending by hour regardless of config order
        assert_eq!(
            b.styles[0].time_colors,
            vec![
                (6, crate::css::Color { r: 0x8b, g: 0xe9, b: 0xfd }),
                (22, crate::css::Color { r: 0xbd, g: 0x93, b: 0xf9 }),
            ]
        );
    }

    #[test]
    fn time_colors_invalid_entries_skipped_duplicates_last_wins() {
        let cfg = parse(
            r##"{
                "modules": ["clock"],
                "clock": {
                    "exec": "echo t",
                    "time-colors": [
                        { "from": 24, "color": "#ffffff" },
                        { "from": 8,  "color": "nope" },
                        { "from": 10, "color": "#111111" },
                        { "from": 10, "color": "#222222" }
                    ]
                }
            }"##,
        )
        .expect("valid config");

        let b = build_registry(&cfg);
        // hour 24 out of range and bad color dropped; duplicate hour 10 -> last wins
        assert_eq!(
            b.styles[0].time_colors,
            vec![(10, crate::css::Color { r: 0x22, g: 0x22, b: 0x22 })]
        );
    }

    #[test]
    fn time_colors_absent_is_empty() {
        let cfg = parse(r##"{ "modules": ["cpu"], "cpu": { "exec": "echo c" } }"##)
            .expect("valid config");
        assert!(build_registry(&cfg).styles[0].time_colors.is_empty());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config`
Expected: the code compiles (`Style.time_colors` exists since Task 1; serde ignores the unknown `time-colors` JSON key until the field is added), but the first two tests FAIL with `left: [] != right: [...]` because nothing populates the field yet. The third test passes already; that's fine.

- [ ] **Step 3: Implement**

In `src/config.rs`, add after the `ModuleConfig` struct:

```rust
#[derive(Debug, Deserialize, Clone)]
pub struct TimeColorRule {
    pub from: u8,
    pub color: String,
}
```

Add the field to `ModuleConfig` (after `pub css: HashMap<String, String>,`):

```rust
    /// Optional hour-of-day tint rules; see the design spec. Validated and
    /// sorted into `Style.time_colors` during registry build.
    #[serde(rename = "time-colors", default)]
    pub time_colors: Vec<TimeColorRule>,
```

Add a helper before `resolve_list`:

```rust
/// Validate a module's `time-colors` rules into sorted (hour, Color) pairs.
/// Invalid hours/colors warn and are skipped; a duplicate hour keeps the last.
fn resolve_time_colors(name: &str, rules: &[TimeColorRule]) -> Vec<(u8, crate::css::Color)> {
    let mut out: Vec<(u8, crate::css::Color)> = Vec::new();
    for rule in rules {
        if rule.from > 23 {
            eprintln!(
                "Winbar: module '{name}' time-colors 'from' {} is not an hour 0-23 (skipped)",
                rule.from
            );
            continue;
        }
        let Some(color) = crate::css::parse_color(&rule.color) else {
            eprintln!(
                "Winbar: module '{name}' time-colors invalid color '{}' (skipped)",
                rule.color
            );
            continue;
        };
        if let Some(existing) = out.iter_mut().find(|(h, _)| *h == rule.from) {
            eprintln!(
                "Winbar: module '{name}' time-colors duplicate 'from' {} (last wins)",
                rule.from
            );
            existing.1 = color;
        } else {
            out.push((rule.from, color));
        }
    }
    out.sort_by_key(|(h, _)| *h);
    out
}
```

Wire it into `resolve_list` — replace the line `styles.push(crate::css::resolve(&cfg.css, &m.css));` with:

```rust
                let mut style = crate::css::resolve(&cfg.css, &m.css);
                style.time_colors = resolve_time_colors(name, &m.time_colors);
                styles.push(style);
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat: parse and validate per-module time-colors rules"
```

---

### Task 3: Paint-time tint in `src/window.rs` + demo config

**Files:**
- Modify: `Cargo.toml` (windows features list, ~line 19-26)
- Modify: `src/window.rs` (imports ~line 14, `WM_PAINT` arm ~line 343-412)
- Modify: `config.json` (modules list + new module block)

**Interfaces:**
- Consumes: `crate::css::pick_time_color(hour, &style.time_colors)` from Task 1; `GetLocalTime()` from the `windows` crate (returns `SYSTEMTIME`, field `wHour: u16`).
- Produces: no new API — rendering behavior only.

- [ ] **Step 1: Add the Win32 feature**

In `Cargo.toml`, add to the `[dependencies.windows]` features list (after `"Win32_System_Registry",`):

```toml
    "Win32_System_SystemInformation",
```

- [ ] **Step 2: Implement the tint in `WM_PAINT`**

In `src/window.rs`, add the import (alongside the existing `use windows::Win32::System::LibraryLoader::GetModuleHandleW;`):

```rust
use windows::Win32::System::SystemInformation::GetLocalTime;
```

In the `WM_PAINT` arm, hoist one clock read above the module loop — after the line `let height = client.bottom - client.top;`, add:

```rust
                        // One clock read per paint; tints below key off this hour.
                        let hour = GetLocalTime().wHour as u8;
```

Then make the text color rule-aware. Replace:

```rust
                            SetTextColor(hdc, COLORREF(style.color.colorref()));
```

with:

```rust
                            let color = crate::css::pick_time_color(hour, &style.time_colors)
                                .unwrap_or(style.color);
                            SetTextColor(hdc, COLORREF(color.colorref()));
```

- [ ] **Step 3: Build and run the full test suite**

Run: `cargo test`
Expected: compiles (feature gate correct, import resolves) and all tests pass.

- [ ] **Step 4: Add the demo module to `config.json`**

Change the modules line to:

```jsonc
    "modules": ["cpu", "clock", "clock-tint"],
```

Add after the `"clock"` block (inside the top-level object, comma-separated):

```jsonc
    "clock-tint": {
        "exec": "powershell -NoProfile -Command \"(Get-Date).ToString('HH:mm')\"",
        "interval": 1,
        "css": { "font-size": "14px", "font-weight": "bold" },
        "time-colors": [
            { "from": 6,  "color": "#8be9fd" },
            { "from": 12, "color": "#ffffff" },
            { "from": 18, "color": "#ffb86c" },
            { "from": 22, "color": "#bd93f9" }
        ]
    }
```

Note: `config.json` is baked into the binary via `include_str!` and must stay parseable — Step 5's `cargo test` re-checks this because `DEFAULT_CONFIG` is parsed in tests via `config::parse`. Also run `cargo build` so the binary embeds the new default.

- [ ] **Step 5: Verify live**

Run: `cargo build && cargo run`
Expected: the bar shows the new `clock-tint` module (HH:MM) whose color matches the current hour's bucket (e.g. white 12:00–17:59). To see other buckets without waiting, temporarily edit the `from` values in `config.json` around the current hour and watch the live reload change the tint; restore the values afterwards. Take a screenshot to confirm, per project convention.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml src/window.rs config.json
git commit -m "feat: tint module text color by hour of day at paint time"
```
