# Open-Source Scaffolding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the essential files that make Taskband a professional open-source GitHub project: README, LICENSE, CONTRIBUTING, repo hygiene fixes, and GitHub Actions for CI checks and tagged releases.

**Architecture:** Pure scaffolding — no Rust source changes except whatever `cargo fmt`/`clippy` fixes are needed to make CI green. Each deliverable is one file (or one hygiene fix) committed independently. Verification is running the exact commands CI will run.

**Tech Stack:** Markdown, GitHub Actions YAML, Rust stable toolchain (`cargo fmt`, `clippy`, `test`).

## Global Constraints

- Public name is **Taskband**; GitHub location is **`mnismi/taskband`** (use verbatim in badges/URLs).
- License is **MIT**, copyright line exactly: `Copyright (c) 2026 Mohamed Nismi`
- CI runs on `windows-latest` only (this is a Windows-only app).
- CI triggers on branches `main` and `master`; release triggers on tags `v*`.
- Binary artifact name is `Taskband.exe` (capital W — set by `[[bin]]` in Cargo.toml).
- Commit messages follow conventional-commit style (`feat:`, `fix:`, `docs:`, `style:`, `ci:`, `chore:`).
- This repo currently sits on branch `master` with no remote; nothing is pushed by this plan.

---

### Task 1: Repo hygiene — commit Cargo.lock, rename branch to main

**Files:**
- Modify: `.gitignore`
- Add to git: `Cargo.lock` (already exists on disk, currently ignored)

**Interfaces:**
- Consumes: nothing.
- Produces: branch `main` (later tasks commit to it); tracked `Cargo.lock` (Task 6's CI builds against it).

- [ ] **Step 1: Remove the Cargo.lock ignore rule**

Current `.gitignore` content is:

```
/target
Cargo.lock
```

Replace the entire file with:

```
/target
```

- [ ] **Step 2: Verify git now sees Cargo.lock**

Run: `git status --short`
Expected output includes both lines:

```
 M .gitignore
?? Cargo.lock
```

- [ ] **Step 3: Commit**

```bash
git add .gitignore Cargo.lock
git commit -m "chore: track Cargo.lock for reproducible binary builds"
```

- [ ] **Step 4: Rename the branch to main**

```bash
git branch -m master main
```

- [ ] **Step 5: Verify the rename**

Run: `git branch --show-current`
Expected output: `main`

### Task 2: LICENSE (MIT)

**Files:**
- Create: `LICENSE`

**Interfaces:**
- Consumes: nothing.
- Produces: `LICENSE` file at repo root (README's License section and badge link to it).

- [ ] **Step 1: Create `LICENSE` with exactly this content**

```
MIT License

Copyright (c) 2026 Mohamed Nismi

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [ ] **Step 2: Commit**

```bash
git add LICENSE
git commit -m "docs: add MIT license"
```

### Task 3: README.md

**Files:**
- Create: `README.md`

**Interfaces:**
- Consumes: `LICENSE` from Task 2 (linked), logo at `assets/taskband-logo-1024.png` (already in repo).
- Produces: `README.md` referencing `.github/workflows/ci.yml` (created in Task 6 — the badge 404s until then, which is fine locally since nothing is pushed).

- [ ] **Step 1: Create `README.md` with exactly this content**

````markdown
<p align="center">
  <img src="assets/taskband-logo-1024.png" width="128" alt="Taskband logo">
</p>

<h1 align="center">Taskband</h1>

<p align="center">A Waybar-inspired status bar for the Windows taskbar.</p>

<p align="center">
  <a href="https://github.com/mnismi/taskband/actions/workflows/ci.yml"><img src="https://github.com/mnismi/taskband/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT"></a>
</p>

Taskband renders config-driven status modules directly on the real Windows
taskbar — CPU load, a clock, or anything a shell command can print. If you
miss [Waybar](https://github.com/Alexays/Waybar) on Windows, this is for you.

## Features

- **Modules on the real taskbar** — no floating overlay window; bars embed
  into the taskbar itself, on every monitor that has one
- **Anything is a module** — a module is just a command (`exec`) run on an
  `interval`; its output is rendered on the bar, multi-line output included
- **Per-monitor routing** — send different modules to different monitors
- **CSS-like styling** — global defaults plus per-module overrides for color,
  background, font, padding, margin, and text alignment
- **JSON5 config with live reload** — comments and trailing commas allowed;
  edits apply instantly, no restart
- **System tray** — reload config, edit config, toggle start-at-login, quit
- **Single self-contained `.exe`** — a default config is baked in, so the
  binary runs on its own

## Installation

Download `Taskband.exe` from the
[latest release](https://github.com/mnismi/taskband/releases/latest) and run it.
An icon appears in the system tray; right-click it to manage Taskband.

Or build from source:

```
git clone https://github.com/mnismi/taskband.git
cd taskband
cargo build --release
```

The binary lands at `target/release/Taskband.exe`.

## Configuration

Taskband looks for `config.json` next to `Taskband.exe` first, then in the
current working directory. If neither exists it uses the built-in default;
the tray's **Edit config** writes that default out beside the exe so you can
customize it. The file is watched — saving it reloads the bar live.

The format is [JSON5](https://json5.org/), so comments and trailing commas
are fine:

```json5
{
    // Module order, left to right (rendered at the right end of the taskbar).
    "modules": ["cpu", "clock"],

    // Global style defaults, inherited by every module.
    "css": {
        "font-family": "Segoe UI",
        "font-size": "12px",
        "color": "#d0d0d0",
        "padding": "0 8px"
    },

    // Each remaining top-level key defines a module.
    "cpu": {
        "exec": "powershell -NoProfile -Command \"'CPU ' + (Get-CimInstance Win32_Processor).LoadPercentage + '%'\"",
        "interval": 2, // seconds between runs (default: 5)
        "css": { "color": "#7fdbb0", "font-weight": "bold" }
    },
    "clock": {
        // Each output line becomes a line on the bar.
        "exec": "powershell -NoProfile -Command \"(Get-Date).ToString('ddd dd MMM'); (Get-Date).ToString('HH:mm:ss')\"",
        "interval": 1,
        "css": { "color": "#ffffff", "font-size": "14px" }
    }
}
```

### Modules

| Key        | Type   | Default | Description                                    |
| ---------- | ------ | ------- | ---------------------------------------------- |
| `exec`     | string | —       | Command to run; stdout becomes the module text |
| `interval` | number | `5`     | Seconds between runs                           |
| `css`      | object | `{}`    | Style overrides for this module                |

### Styling

Supported CSS properties, in the global `css` block or per module:

`color`, `background-color`, `font-family`, `font-size` (px),
`font-weight` (`normal`, `bold`, or a number), `padding`, `margin`
(1–4 edge values, px), `text-align` (`left`, `center`, `right`).

### Multiple monitors

By default all modules appear on the primary taskbar. To route modules per
monitor, add a `monitors` map keyed by monitor index (shown in the console
output of a debug build):

```json5
{
    "modules": ["cpu", "clock"], // fallback for monitors not listed below
    "monitors": {
        "0": { "modules": ["cpu", "clock"] },
        "1": { "modules": ["clock"] }
    }
}
```

Secondary taskbars need **Settings → Personalization → Taskbar → Show my
taskbar on all displays** enabled. Windows 11 paints its own clock on
secondary taskbars; `"secondary-clock-reserve"` (default `100`) reserves that
many pixels at the right edge so modules don't overlap it.

## Building from source

Requires [Rust](https://rustup.rs/) stable on Windows.

```
cargo run             # debug build; keeps a console for diagnostics
cargo build --release # console-less background app
```

## License

[MIT](LICENSE)
````

- [ ] **Step 2: Verify the logo path resolves**

Run: `ls assets/taskband-logo-1024.png`
Expected: the file is listed (no error).

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: add README"
```

### Task 4: CONTRIBUTING.md

**Files:**
- Create: `CONTRIBUTING.md`

**Interfaces:**
- Consumes: nothing.
- Produces: `CONTRIBUTING.md`; the check commands listed in it must match Task 6's CI steps exactly.

- [ ] **Step 1: Create `CONTRIBUTING.md` with exactly this content**

````markdown
# Contributing to Taskband

Thanks for your interest! Bug reports, feature ideas, and pull requests are
all welcome.

## Prerequisites

- Windows 10/11
- [Rust](https://rustup.rs/) stable (with `rustfmt` and `clippy` components,
  included by default)

## Building and running

```
cargo run             # debug build with a console for diagnostics
cargo build --release # release build (no console window)
```

## Before opening a pull request

CI runs these on every PR; run them locally first:

```
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Commit messages

This project uses [Conventional Commits](https://www.conventionalcommits.org/):
`feat:`, `fix:`, `docs:`, `style:`, `refactor:`, `ci:`, `chore:`. Keep the
subject line imperative and under ~72 characters.

## Questions

Open an issue — there's no discussion forum yet.
````

- [ ] **Step 2: Commit**

```bash
git add CONTRIBUTING.md
git commit -m "docs: add contributing guide"
```

### Task 5: Make the codebase pass the CI checks locally

**Files:**
- Modify: any `src/*.rs` file that `cargo fmt` reformats or `clippy` flags (discovery step — likely small or empty diffs)

**Interfaces:**
- Consumes: nothing.
- Produces: a working tree where all three CI check commands exit 0, so Task 6's workflow is green on first push.

- [ ] **Step 1: Check formatting**

Run: `cargo fmt --all -- --check`
Expected: exit code 0 with no output. If it prints diffs, continue to Step 2; otherwise skip to Step 3.

- [ ] **Step 2 (only if Step 1 failed): Apply formatting and commit**

```bash
cargo fmt --all
git add -u
git commit -m "style: apply rustfmt"
```

- [ ] **Step 3: Check clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: `Finished` with no errors. If clippy reports errors, fix each one with the **minimal** change that satisfies the lint (prefer the fix clippy suggests; use a targeted `#[allow(...)]` with a one-line justification comment only when the suggestion would hurt clarity), then re-run until clean and commit:

```bash
git add -u
git commit -m "fix: resolve clippy lints"
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: exit code 0 (this crate has few or no tests; an empty test run passing is fine). If a test fails, stop and report — do not change test expectations to force a pass.

### Task 6: CI workflow

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: green checks from Task 5; `Cargo.lock` from Task 1.
- Produces: workflow named `CI` at `.github/workflows/ci.yml` (README's badge URL from Task 3 points at this exact path).

- [ ] **Step 1: Create `.github/workflows/ci.yml` with exactly this content**

```yaml
name: CI

on:
  push:
    branches: [main, master]
  pull_request:
    branches: [main, master]

env:
  CARGO_TERM_COLOR: always

jobs:
  check:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - uses: Swatinem/rust-cache@v2

      - name: Check formatting
        run: cargo fmt --all -- --check

      - name: Clippy
        run: cargo clippy --all-targets -- -D warnings

      - name: Test
        run: cargo test

      - name: Build release
        run: cargo build --release
```

- [ ] **Step 2: Verify the YAML is well-formed**

Run: `python -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml')); print('OK')"`
Expected: `OK`. If Python/PyYAML is unavailable, run `cargo` is not a substitute — instead visually confirm indentation is 2 spaces throughout and every `- name:` aligns; the real check happens on first push.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add fmt/clippy/test/build workflow"
```

### Task 7: Release workflow

**Files:**
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: `Taskband.exe` produced by `cargo build --release`; repo `config.json`.
- Produces: on tag `v*`, a GitHub Release with `taskband-<tag>-x86_64-windows.zip` attached.

- [ ] **Step 1: Create `.github/workflows/release.yml` with exactly this content**

```yaml
name: Release

on:
  push:
    tags: ["v*"]

permissions:
  contents: write

jobs:
  release:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - uses: Swatinem/rust-cache@v2

      - name: Build release
        run: cargo build --release

      - name: Package
        shell: pwsh
        run: |
          New-Item -ItemType Directory dist | Out-Null
          Copy-Item target/release/Taskband.exe dist/
          Copy-Item config.json dist/
          Compress-Archive -Path dist/* -DestinationPath "taskband-${{ github.ref_name }}-x86_64-windows.zip"

      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          files: taskband-*-x86_64-windows.zip
          generate_release_notes: true
```

- [ ] **Step 2: Verify the YAML is well-formed**

Run: `python -c "import yaml,sys; yaml.safe_load(open('.github/workflows/release.yml')); print('OK')"`
Expected: `OK` (same fallback as Task 6 Step 2 if Python is unavailable).

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: add tagged release workflow with zipped exe"
```

---

## After the plan

Manual steps for the maintainer (not part of this plan): create the
`mnismi/taskband` repo on GitHub, `git remote add origin
https://github.com/mnismi/taskband.git`, `git push -u origin main`, and later
`git tag v0.1.0 && git push origin v0.1.0` to cut the first release.
