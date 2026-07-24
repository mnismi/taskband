# Open-Source Scaffolding for Taskband — Design

**Date:** 2026-07-23
**Status:** Approved

## Goal

Make the repository presentable as a professional open-source project on GitHub.
Scope is "essentials only": README, LICENSE, CONTRIBUTING, repo hygiene fixes,
and GitHub Actions for checks and releases. The public name is **Taskband**.

## Decisions

- **License:** MIT, `Copyright (c) 2026 Mohamed Nismi`
- **Host:** GitHub (`.github/` conventions)
- **CI:** checks on push/PR, plus release artifacts on version tags
- **Name:** "Taskband" everywhere; tagline mentions Waybar-inspired

## Deliverables

### 1. README.md

- Logo (`assets/taskband-logo-1024.png`, displayed at reduced width), title
  **Taskband**, tagline: "A Waybar-inspired status bar for the Windows taskbar."
- Badges: CI status, MIT license.
- **Features:** config-driven modules rendered on the real taskbar; per-monitor
  module routing; JSON5 config (comments + trailing commas) with live hot
  reload; per-module CSS-like styling; system tray icon with "Edit config";
  single self-contained `.exe` with a baked-in default config.
- **Installation:** download `Taskband.exe` from GitHub Releases, or build from
  source with `cargo build --release`.
- **Configuration:** config lives beside the exe (`config.json`); document the
  module format (`exec`, `interval`, `css`), top-level `modules` order, the
  `monitors` per-monitor routing map, and `secondary-clock-reserve`. Use the
  repo's current `config.json` as the worked example.
- **Building from source:** Rust stable on Windows; `cargo run` / `cargo build
  --release`; debug builds keep a console for diagnostics.
- **License** section pointing at LICENSE.

### 2. LICENSE

Standard MIT text, `Copyright (c) 2026 Mohamed Nismi`.

### 3. CONTRIBUTING.md

Short guide: prerequisites (Rust stable, Windows 11), build/run commands,
required checks before a PR (`cargo fmt`, `cargo clippy -- -D warnings`,
`cargo test`), and conventional-commit message style (`feat:`, `fix:`,
`docs:`, …) matching the existing history.

### 4. Repo hygiene

- Remove `Cargo.lock` from `.gitignore` and commit the lockfile (Rust
  convention for binary crates — reproducible builds).
- Rename local branch `master` → `main` to match GitHub's default. CI triggers
  on both `main` and `master` so this is non-fatal either way.

### 5. .github/workflows/ci.yml

On push and pull_request (branches `main`, `master`), one job on
`windows-latest`:

1. `cargo fmt --all -- --check`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test`
4. `cargo build --release`

Run fmt/clippy locally as part of this work so CI is green from the first push.

### 6. .github/workflows/release.yml

On tag push matching `v*`: build release on `windows-latest`, zip `Taskband.exe`
together with `config.json` as `taskband-<tag>-x86_64-windows.zip`, and create a
GitHub Release with the zip attached (`softprops/action-gh-release` or `gh
release create`). `permissions: contents: write`.

## Out of scope

CODE_OF_CONDUCT, SECURITY.md, issue/PR templates, CHANGELOG, Dependabot.
These can be added later without conflicting with anything here.
