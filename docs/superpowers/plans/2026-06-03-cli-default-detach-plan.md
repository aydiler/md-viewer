# CLI Default Detach Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `md-viewer README.md` return the terminal prompt quickly by default while preserving foreground mode for debugging.

**Architecture:** Add a small CLI process-management layer before `eframe::run_native`. The parent process parses args, detects terminal launch, respawns the same executable with a hidden `--no-detach` marker and null stdio, then exits; the child runs the existing GUI path unchanged. Tests cover CLI parse/decision behavior without launching egui.

**Tech Stack:** Rust 1.80, clap derive, std `Command`/`Stdio`, eframe/egui, existing in-file unit tests.

---

## File Structure

- Modify `src/main.rs`: add CLI flags, helper functions, unit tests, and parent respawn path before `eframe::run_native`.
- Modify `README.md`: document default detach and `--foreground`.
- Create `docs/devlog/034-cli-default-detach.md`: record issue scope, root cause, implementation, and validation.
- Modify `docs/LESSONS.md`: add reusable process-management lesson about foreground escape hatch for default-detached GUI CLIs.
- Keep existing spec `docs/superpowers/specs/2026-06-03-cli-default-detach-design.md` and this plan as planning artifacts.

---

### Task 1: Add CLI detach decision tests

**Files:**
- Modify: `src/main.rs:1174-1184`
- Modify: `src/main.rs:4132-4260`

- [ ] **Step 1: Add test imports and failing tests**

Add these tests near the top of the existing `#[cfg(test)] mod tests` block in `src/main.rs`, immediately after `use super::*;`:

```rust
    #[test]
    fn default_terminal_launch_detaches() {
        let args = Args::try_parse_from(["md-viewer", "README.md"]).unwrap();
        assert!(should_detach(&args, true));
    }

    #[test]
    fn non_terminal_launch_does_not_detach() {
        let args = Args::try_parse_from(["md-viewer", "README.md"]).unwrap();
        assert!(!should_detach(&args, false));
    }

    #[test]
    fn foreground_flag_disables_detach() {
        let args = Args::try_parse_from(["md-viewer", "--foreground", "README.md"]).unwrap();
        assert!(!should_detach(&args, true));
    }

    #[test]
    fn hidden_no_detach_marker_disables_detach() {
        let args = Args::try_parse_from(["md-viewer", "--no-detach", "README.md"]).unwrap();
        assert!(!should_detach(&args, true));
    }

    #[test]
    fn child_args_preserve_user_args_and_append_marker() {
        let child_args = child_args_with_no_detach([
            OsString::from("md-viewer"),
            OsString::from("README.md"),
            OsString::from("--no-watch"),
        ]);

        assert_eq!(
            child_args,
            vec![
                OsString::from("README.md"),
                OsString::from("--no-watch"),
                OsString::from("--no-detach"),
            ]
        );
    }

    #[test]
    fn hidden_no_detach_marker_is_not_in_help() {
        use clap::CommandFactory;

        let help = Args::command().render_long_help().to_string();
        assert!(help.contains("--foreground"));
        assert!(!help.contains("--no-detach"));
    }
```

- [ ] **Step 2: Run tests and confirm failure**

Run:

```bash
cargo test default_terminal_launch_detaches non_terminal_launch_does_not_detach foreground_flag_disables_detach hidden_no_detach_marker_disables_detach child_args_preserve_user_args_and_append_marker hidden_no_detach_marker_is_not_in_help
```

Expected: FAIL because `should_detach`, `child_args_with_no_detach`, `OsString`, `--foreground`, and `--no-detach` do not exist yet.

---

### Task 2: Implement detach flags and helper functions

**Files:**
- Modify: `src/main.rs:3-10`
- Modify: `src/main.rs:1174-1209`

- [ ] **Step 1: Add imports**

Replace the top import block:

```rust
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::LazyLock;
use std::time::{Duration, Instant};
```

with:

```rust
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::LazyLock;
use std::time::{Duration, Instant};
```

- [ ] **Step 2: Add CLI fields and helpers**

Replace `Args` and `main` in `src/main.rs:1174-1209` with:

```rust
#[derive(Parser, Debug)]
#[command(name = "md-viewer")]
#[command(about = "A lightweight markdown viewer", long_about = None)]
struct Args {
    /// Markdown file to open
    file: Option<PathBuf>,

    /// Disable live reload (watching is enabled by default)
    #[arg(long)]
    no_watch: bool,

    /// Keep the GUI process attached to the terminal for debugging/logs
    #[arg(long)]
    foreground: bool,

    /// Internal marker used by the detached child process to avoid respawn loops
    #[arg(long, hide = true)]
    no_detach: bool,
}

fn should_detach(args: &Args, launched_from_terminal: bool) -> bool {
    launched_from_terminal && !args.foreground && !args.no_detach
}

fn launched_from_terminal() -> bool {
    std::io::stdin().is_terminal()
        || std::io::stdout().is_terminal()
        || std::io::stderr().is_terminal()
}

fn child_args_with_no_detach<I>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = OsString>,
{
    let mut child_args: Vec<OsString> = args.into_iter().skip(1).collect();
    child_args.push(OsString::from("--no-detach"));
    child_args
}

fn spawn_detached_child() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let child_args = child_args_with_no_detach(std::env::args_os());

    Command::new(exe)
        .args(child_args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    Ok(())
}

fn main() -> eframe::Result<()> {
    env_logger::init();

    let args = Args::parse();

    if should_detach(&args, launched_from_terminal()) {
        if let Err(err) = spawn_detached_child() {
            eprintln!("Failed to detach md-viewer process: {err}. Running in foreground.");
        } else {
            return Ok(());
        }
    }

    // Calculate optimal window width assuming both sidebars are shown (the default)
    let optimal_width =
        CONTENT_OPTIMAL_WIDTH + EXPLORER_DEFAULT_WIDTH + OUTLINE_DEFAULT_WIDTH + PANEL_SEPARATORS;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([optimal_width, OPTIMAL_WINDOW_HEIGHT])
            .with_min_inner_size([400.0, 300.0])
            .with_title("Markdown Viewer")
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "md-viewer",
        options,
        Box::new(move |cc| Ok(Box::new(MarkdownApp::new(cc, args.file, !args.no_watch)))),
    )
}
```

- [ ] **Step 3: Run targeted tests**

Run:

```bash
cargo test default_terminal_launch_detaches non_terminal_launch_does_not_detach foreground_flag_disables_detach hidden_no_detach_marker_disables_detach child_args_preserve_user_args_and_append_marker hidden_no_detach_marker_is_not_in_help
```

Expected: PASS for all six tests.

- [ ] **Step 4: Run full unit tests**

Run:

```bash
cargo test
```

Expected: PASS.

---

### Task 3: Document CLI behavior in README

**Files:**
- Modify: `README.md:208-216`

- [ ] **Step 1: Update usage examples**

Replace `README.md:208-216` with:

```markdown
## Usage

```bash
# Open a file and return the terminal prompt (live reload is enabled by default)
md-viewer README.md

# Keep the viewer attached to the terminal for debugging/logs
md-viewer --foreground README.md

# Disable live reload
md-viewer README.md --no-watch
```

When launched from a terminal, `md-viewer` detaches by default so the shell prompt is available while the window stays open. Use `--foreground` when you want terminal logs or blocking process behavior.
```

- [ ] **Step 2: Verify README snippet renders correctly**

Run:

```bash
git diff -- README.md
```

Expected: usage block has one fenced bash block and one explanatory paragraph; no nested or unclosed fence.

---

### Task 4: Add devlog and lesson

**Files:**
- Create: `docs/devlog/034-cli-default-detach.md`
- Modify: `docs/LESSONS.md`

- [ ] **Step 1: Create devlog**

Create `docs/devlog/034-cli-default-detach.md` with:

```markdown
# Feature: CLI default detach

**Status:** 🚧 In Progress
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

## Architecture

### Modified CLI args

- `foreground: bool` — user-facing `--foreground` flag.
- `no_detach: bool` — hidden `--no-detach` child marker.

### New Functions

| Function | Purpose |
|----------|---------|
| `should_detach()` | Centralizes detach decision for unit coverage. |
| `launched_from_terminal()` | Detects whether stdio is attached to a terminal. |
| `child_args_with_no_detach()` | Preserves user args and appends the child marker. |
| `spawn_detached_child()` | Respawns current executable with null stdio. |

## Testing Notes

Initial plan:

- `cargo test`
- `cargo clippy --all-targets`
- `cargo run -- --help`
- `timeout 3s cargo run -- --foreground README.md`
- terminal smoke for default detach with built binary

Final validation results will be added before PR closeout.

## Future Improvements

- [ ] Consider single-instance IPC if users later want repeated CLI launches to reuse an existing window instead of opening a new process.
```

- [ ] **Step 2: Add LESSONS entry**

Append this section under the `## Distribution / CI / Packaging` heading in `docs/LESSONS.md`, near other CLI/package launch lessons:

```markdown
### Default-detached GUI CLIs still need a foreground escape hatch
**Context:** Issue #30 — launching `md-viewer README.md` from a terminal kept the shell occupied until the GUI closed.
**Root cause:** `eframe::run_native` runs in the foreground process. Desktop launch was unaffected because the `.desktop` file uses `Terminal=false`, but direct CLI launch behaved like any foreground command.
**Fix:** Detect terminal launch, respawn the same executable with a hidden child marker (`--no-detach`) and null stdio, then let the parent exit. Keep a documented `--foreground` flag so startup errors, logs, and scripts can still use blocking behavior.
**Files:** `src/main.rs`, `README.md`
```

- [ ] **Step 3: Verify docs diff**

Run:

```bash
git diff -- docs/devlog/034-cli-default-detach.md docs/LESSONS.md
```

Expected: devlog created, one LESSONS entry added, no unrelated docs changed.

---

### Task 5: Validate CLI behavior and code quality

**Files:**
- Validate: `src/main.rs`
- Validate: `README.md`
- Validate: `docs/devlog/034-cli-default-detach.md`
- Validate: `docs/LESSONS.md`

- [ ] **Step 1: Run unit tests**

Run:

```bash
cargo test
```

Expected: PASS.

- [ ] **Step 2: Run clippy**

Run:

```bash
cargo clippy --all-targets
```

Expected: PASS with no new warnings.

- [ ] **Step 3: Build debug binary**

Run:

```bash
cargo build
```

Expected: PASS and binary at `target/debug/md-viewer`.

- [ ] **Step 4: Verify help path does not detach**

Run:

```bash
target/debug/md-viewer --help
```

Expected: help prints once, includes `--foreground`, excludes `--no-detach`, exits immediately.

- [ ] **Step 5: Verify foreground path stays attached**

Run:

```bash
timeout 3s target/debug/md-viewer --foreground README.md
```

Expected: command remains foreground until timeout kills it. Exit code from `timeout` is expected to be `124`.

- [ ] **Step 6: Verify default detach returns promptly**

Run:

```bash
timeout 3s target/debug/md-viewer README.md
```

Expected: command exits before timeout with code `0`, while a child `md-viewer --no-detach README.md` process may keep running. Close the GUI window manually after confirming behavior.

- [ ] **Step 7: Run formatting/diff hygiene**

Run:

```bash
git diff --check
```

Expected: no whitespace errors.

- [ ] **Step 8: Update devlog validation section**

Replace the `## Testing Notes` section in `docs/devlog/034-cli-default-detach.md` with exact command results from Steps 1-7. Use this structure:

```markdown
## Testing Notes

- `cargo test` — PASS.
- `cargo clippy --all-targets` — PASS.
- `cargo build` — PASS.
- `target/debug/md-viewer --help` — PASS; help includes `--foreground` and hides `--no-detach`.
- `timeout 3s target/debug/md-viewer --foreground README.md` — PASS; command stayed attached until timeout exit 124.
- `timeout 3s target/debug/md-viewer README.md` — PASS; parent exited before timeout with code 0 and GUI child stayed open.
- `git diff --check` — PASS.
```

If any command has a different result, record the exact result instead of this PASS text.

---

### Task 6: Documentation impact check and closeout prep

**Files:**
- Verify: `docs/superpowers/specs/2026-06-03-cli-default-detach-design.md`
- Verify: `docs/superpowers/plans/2026-06-03-cli-default-detach-plan.md`
- Verify: `README.md`
- Verify: `docs/devlog/034-cli-default-detach.md`
- Verify: `docs/LESSONS.md`

- [ ] **Step 1: Run project docs impact check in verify/plan mode**

Use `openclaude-addons:project-docs-updater` with:

```text
Project root: /home/akiro/Coding/md-viewer
Change range: upstream/main...HEAD plus working tree
Mode: verify
Task: issue #30 CLI default detach
Strictness: advisory
```

Expected: report identifies README, devlog, and LESSONS as updated. If it identifies required System Notes updates under workspace policy, apply those before final verification.

- [ ] **Step 2: Check branch and changed files**

Run:

```bash
git status --short --branch && git diff --name-only
```

Expected: branch `fix/issue-30-cli-detach`; changed files limited to `src/main.rs`, `README.md`, `docs/devlog/034-cli-default-detach.md`, `docs/LESSONS.md`, `docs/superpowers/specs/2026-06-03-cli-default-detach-design.md`, and `docs/superpowers/plans/2026-06-03-cli-default-detach-plan.md` unless System Notes updates are required.

- [ ] **Step 3: Review final diff**

Run:

```bash
git diff -- src/main.rs README.md docs/devlog/034-cli-default-detach.md docs/LESSONS.md docs/superpowers/specs/2026-06-03-cli-default-detach-design.md docs/superpowers/plans/2026-06-03-cli-default-detach-plan.md
```

Expected: no unrelated refactors, no raw logs, no secrets, no overclaim beyond recorded validation.

- [ ] **Step 4: Request commit/push/PR approval**

Do not commit, push, or create the PR until the user explicitly approves those actions. Suggested commit message after approval:

```text
fix: detach CLI launch by default
```

PR target after approval: `aydiler/md-viewer:main` from fork branch `aki1ro/md-viewer:fix/issue-30-cli-detach`.

---

## Self-Review

- Spec coverage: default detach, `--foreground`, hidden marker, help/parse-error safety, README, devlog, LESSONS, and validation all mapped to tasks.
- Placeholder scan: no placeholder sections; conditional validation language records exact output when commands differ.
- Type consistency: helper names match tests and implementation snippets: `should_detach`, `launched_from_terminal`, `child_args_with_no_detach`, `spawn_detached_child`.
