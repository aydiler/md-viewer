# Windows path characterization (#141 point 4)

**Status:** Complete
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

## First Windows run (43434525981)

- The new Windows table passed on the first run: every drive-letter, UNC and
  `file://` expectation held, including `file://server/share/…` resolving to
  `\\server\share\…` on Windows (external on Unix).
- One existing test failed: `plain_click_and_ctrl_click_resolve_the_same_link_identically`
  built its `file://` URI with `format!("file://{}", path)`. On Windows
  `canonicalize` returns a verbatim `\\?\C:\…` path and the space is not
  encoded, so the string is not a valid file URI and is correctly external.
  A test bug, not an app bug; it now uses `Url::from_file_path`, like the
  neighbouring tests.

## Key Discovery

A test suite that has only ever run on one OS carries that OS's assumptions in
its fixtures. The first Windows run found one in 78 tests, in fixture
construction rather than in the code under test.
