# Contributing to Winbar

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

Open an issue: there's no discussion forum yet.
