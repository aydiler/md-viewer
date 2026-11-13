# Macro compile-fail tests run on stable only

**Status:** Complete
**Branch:** `fix/trybuild-nightly`
**Date:** 2026-11-13

## Summary

`egui_commonmark_macros_extended`'s `tests()` uses trybuild, which compares the
compiler's error output for `tests/fail/*.rs` against checked-in `.stderr`
files. On nightly, `incorrect_immutable.rs` produced differently worded output,
so `cargo test` in the renderer workspace failed on this machine's default
toolchain while CI (stable) stayed green.

## Change

`compile_fail` now runs only when `rustc -V` reports a stable release (no
`-nightly`, `-beta` or `-dev`). The `pass` cases still run on every toolchain.
`rustc_is_stable` reads `RUSTC` if set, otherwise runs `rustc`, which follows
the toolchain `cargo +<toolchain>` selected through `RUSTUP_TOOLCHAIN`. No new
dependency.

## Verification

| toolchain | result |
|---|---|
| nightly | 4 passed; prints "skipping compile_fail cases" |
| stable (1.99.1) | 4 passed; all three `should fail to compile` cases ran |
| stable, `incorrect_type.stderr` deliberately broken | rc 101, `mismatch` reported |

The last row is the check that the gate did not turn the stable run into a
no-op.
