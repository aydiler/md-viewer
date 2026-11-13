# Windows path characterization (#141 point 4)

**Status:** In progress
**Branch:** `test/windows-path-table`
**Date:** 2026-11-14

## Why

#141's last structural gap: the destination→path table
(`local_destination_resolution_is_characterized_for_every_uri_shape`) is
`#[cfg(unix)]`, and CI only ran `cargo check` on Windows, so drive-letter, UNC
and `file://host` behaviour was never executed anywhere.

## Change

- `local_destination_resolution_is_characterized_on_windows` (`#[cfg(windows)]`):
  relative paths with `\` and `/`, `C:\…` and `C:/…`, `file:///C:/…`
  (with a percent escape), `\\server\share\…`, `file://server/share/…`, and the
  scheme boundary (`//host`, `https`, `data`). Characterizes current behaviour;
  collects every mismatch before failing so one run shows Windows' full answer.
- `cross-platform.yml`: new `test-windows` job running `cargo test --locked`
  on `windows-latest`.
