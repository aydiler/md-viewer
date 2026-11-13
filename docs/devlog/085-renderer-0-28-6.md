# Vendored renderer 0.28.6

**Status:** Complete
**Branch:** `chore/renderer-0.28.6`
**Date:** 2026-11-13

## Why

The release dry run on ebb2868 (run 43405680630) stopped at `validate`:
`check-fork-publishable.sh` reported `egui_commonmark_extended@0.28.5` and
`egui_commonmark_macros_extended@0.28.5` as STALE. #227, #228 and #229 changed
renderer sources after 0.28.5 was published, so a release would have shipped
the old renderer to `cargo install` users. The check working as designed.

## Change

0.28.5 → 0.28.6 in `crates/egui_commonmark/Cargo.toml` (workspace version and
the two internal pins), the root `Cargo.toml` (dependency and the two
`[patch.crates-io]` pins), and the version line of each fork crate in both
lockfiles: five lockfile lines in total, edited by exact match. md-viewer stays
0.3.2.

## Verification

- `check-fork-publishable.sh`: rc 0, all three crates "not on crates.io yet".
- `check-workspace-sync.sh`, `cargo build --locked`, `cargo test --locked` (80).
- Renderer with the CI feature set: 108 + 34 + the other targets green.
- Local `cargo publish --dry-run -p md-viewer` after
  `prepare-crates-manifest.py`, with the fork patched in via `--config`:
  compiled `egui_commonmark_extended v0.28.6` and md-viewer.

## Key Discovery

Restoring the root `Cargo.toml` after the package check with `git checkout`
also reverted the uncommitted bump in it. A copy taken before running
`prepare-crates-manifest.py` saved it. Commit (or copy) before any step that
rewrites a tracked file you will want to restore.
