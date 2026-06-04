# CLI Default Detach Design

**Status:** Approved
**Date:** 2026-06-03
**Branch:** `fix/issue-30-cli-detach`
**Issue:** aydiler/md-viewer#30

## Problem

Launching `md-viewer README.md` from a terminal currently keeps the shell attached until the GUI window closes. This is normal foreground process behavior from `eframe::run_native`, but it surprises users who expect a document viewer to release the prompt after opening.

Desktop-file launch already avoids the terminal because `data/md-viewer.desktop` uses `Terminal=false`.

## Goals

- Make normal CLI file launches return the terminal prompt quickly.
- Preserve a foreground mode for debugging, logs, and scripts that intentionally want blocking behavior.
- Avoid affecting `--help`, parse errors, or desktop-file launch behavior.
- Avoid process respawn loops.

## Non-Goals

- No multi-process IPC or single-instance handoff.
- No daemon/service mode.
- No packaging-specific launcher wrapper unless implementation proves Rust-side detach unsuitable.

## Selected Approach

Default to detaching from interactive terminal launches.

Add:

- `--foreground`: documented user-facing flag that keeps existing blocking behavior.
- hidden child marker, likely `--no-detach`: internal flag appended by the parent process so the respawned child does not detach again.

High-level flow:

1. Parse CLI args with clap.
2. If `--foreground` or hidden child marker is present, run existing `eframe::run_native` path.
3. Otherwise, respawn current executable with original args plus hidden child marker.
4. Detach child from terminal/session, silence inherited stdio if needed, and exit parent successfully.
5. Child runs current GUI logic unchanged.

## Tradeoffs

- Pros: fixes issue #30 directly; common viewer UX; keeps foreground escape hatch.
- Cons: parent may exit before GUI startup failure is visible; debugging requires `--foreground`.
- Mitigation: document `--foreground`; keep parse errors/help in parent path before detach.

## Testing Plan

- Unit or integration coverage for CLI parsing where practical.
- Smoke: `target/debug/md-viewer README.md` exits parent quickly while child continues.
- Smoke: `target/debug/md-viewer --foreground README.md` remains attached until window closes.
- Smoke: `target/debug/md-viewer --help` prints help and exits without respawning.
- Run `cargo test` and `cargo clippy`.

## Documentation Impact

- README usage should mention default detach and `--foreground`.
- Devlog entry `docs/devlog/034-cli-default-detach.md` should record scope, branch, root cause, implementation, and validation.
- Add a LESSONS entry if implementation reveals a reusable Rust/process-management gotcha.

## Open Questions Resolved

- Default behavior: detach by default.
- Branch base: create work from user's fork `origin/main`, which matches `upstream/main`.
- PR target: push fork branch and open PR against upstream `main` after validation.
