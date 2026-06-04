# Feature: CLI default detach

**Status:** ✅ Complete
**Branch:** `fix/issue-30-cli-detach`
**Date:** 2026-06-03

## Summary

Issue #30 reports that launching `md-viewer README.md` from a terminal keeps the shell occupied until the GUI window closes. Root cause: `src/main.rs` runs `eframe::run_native` in the foreground process. Desktop-file launch already avoids this because `data/md-viewer.desktop` uses `Terminal=false`.

This change makes terminal launches detach by default while preserving `--foreground` for debugging and log capture.

## Features

- [x] Add `--foreground` to keep blocking terminal behavior when needed.
- [x] Add hidden `--no-detach` marker to prevent child respawn loops.
- [x] Respawn the GUI child with null stdio and exit the parent quickly for terminal launches.
- [x] Preserve `--help`, parse errors, and non-terminal launches.

## Key Discoveries

### Foreground mode still matters

Default-detached GUI CLIs improve normal viewer UX, but they can hide startup errors and logs. A documented foreground flag keeps debugging and script behavior available without changing the common launch path.

### Child marker must stay before `--`

Clap treats arguments after `--` as positional values. The hidden `--no-detach` marker must be inserted before `--`; appending it after user args can make the child treat the marker as a file path and fail with stdio already nulled.

## Architecture

### Modified CLI args

- `foreground: bool` — user-facing `--foreground` flag.
- `no_detach: bool` — hidden `--no-detach` child marker.

### New Functions

| Function | Purpose |
|----------|---------|
| `should_detach()` | Centralizes detach decision for unit coverage. |
| `launched_from_terminal()` | Detects whether stdio is attached to a terminal. |
| `child_args_with_no_detach()` | Preserves user args and inserts the child marker before `--` when present. |
| `spawn_detached_child()` | Respawns current executable with null stdio. |

## Testing Notes

- `cargo fmt --check` — PASS.
- `cargo test` — PASS; 22 tests passed.
- `cargo clippy --all-targets` — PASS with existing vendored warnings for unused `max_width`, deprecated `allocate_ui_at_rect`, and an unused patch notice.
- `cargo build` — PASS with existing vendored warnings.
- `target/debug/md-viewer --help` — PASS; help includes `--foreground` and hides `--no-detach`.
- `timeout 3s target/debug/md-viewer --foreground <tmp.md>` — PASS; command stayed attached until timeout exit 124.
- `timeout 3s target/debug/md-viewer --no-detach <tmp.md>` — PASS; hidden child path stayed attached until timeout exit 124.
- `script -q -c "target/debug/md-viewer <tmp.md>" /dev/null` — PASS; parent exited with code 0 under a pseudo-terminal and child process was observable before cleanup.
- `git diff --check` — PASS.

## Future Improvements

- [ ] Consider single-instance IPC if users later want repeated CLI launches to reuse an existing window instead of opening a new process.
