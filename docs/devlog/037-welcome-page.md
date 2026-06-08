# Feature: Welcome / idle page + recent files (issue #28, part 2)

**Status:** Implemented
**Branch:** `feature/welcome-page`
**Date:** 2026-06-08
**Issue:** aydiler/md-viewer#28

## Summary

When no document is open, the app now shows a **welcome / idle page** instead of
forcing a sample tab and refusing to close the last tab. Part 2 of issue #28
(part 1 was File → Open Folder, PR #33).

## User-Facing Behavior

- **Closing the last tab is now allowed** — it lands on the welcome page (the
  previous behavior made Ctrl+W / × a no-op on the final tab).
- A fresh launch with no file and no saved session shows the welcome page (the
  built-in sample document was removed).
- The welcome page has: a document icon, "Open a file or folder" heading,
  centered **Open File** / **Open Folder** buttons, and a **Recent** list.
- Each recent row shows the filename (click to open), its directory, and a
  relative time ("3m ago"); missing files are greyed and non-clickable. A
  "Show more…" / "Show less" toggle expands the list past the first few.
- The tab bar shows a dim "No file open" hint while empty.

## Architecture

- **State:** `MarkdownApp.recent_files: Vec<RecentEntry>` and
  `welcome_show_all: bool`. `RecentEntry { path, last_opened (epoch secs) }`.
  Persisted via a new `PersistedState.recent_files` field (no new dependency —
  serde + `u64` timestamps).
- **Logic (pure, unit-tested):** `push_recent(list, path, now)` (dedupe → front,
  cap at `RECENT_FILES_CAP = 20`) and `format_relative_time(epoch, now)`.
  `record_recent` wraps `push_recent` with `now_epoch_secs()`; it is called from
  `open_in_new_tab` and once for a CLI-provided file in the constructor.
- **Empty tabs:** `close_tab` no longer guards the last tab and no longer
  underflows when `tabs` becomes empty; the constructor starts with an empty
  `Vec` instead of `Tab::from_sample()`. All tab access already used
  `.get()/.get_mut()`, so an empty `tabs` is safe (no panics).
- **UI:** `render_welcome(ui)` is invoked from `render_tab_content` when
  `self.tabs.get(active_tab)` is `None`. Uses the deferred-action pattern
  (snapshot the recent list, collect clicks, act after the closure) to avoid
  borrowing `self` while iterating.
- **Removed:** `Tab::from_sample()` and the `SAMPLE_MARKDOWN` const (the welcome
  page replaces the first-run sample; dead code after this change).

## Key Discoveries

- **egui centering:** a `ui.horizontal` inside `vertical_centered` expands to full
  width, so the button row was left-aligned. Fixed by centering the row manually
  with leading `add_space((avail - group_w) / 2.0)`.
- **Recent recording site:** the constructor builds initial tabs via `Tab::new`
  directly (not `open_in_new_tab`), so CLI/session files weren't recorded.
  A CLI-provided file is now recorded explicitly; session-restored tabs are not
  (restoration isn't an explicit "open").

## Testing Notes

- `cargo test` — 29 passed (added `push_recent_*` and `relative_time_buckets`).
- `cargo build`, `cargo fmt --check`, `cargo clippy --bin md-viewer` — clean (only
  pre-existing vendored warnings).
- Live (Xvfb `:99`): launched with README → closed the last tab (Ctrl+W) → welcome
  page rendered with centered buttons, the "No file open" tab hint, and a Recent
  entry (`README.md  /home/adiler/md-viewer · 1m ago`); clicking the recent entry
  reopened the file (window title returned to "README.md - Markdown Viewer").
- The Open File / Open Folder buttons invoke the same `rfd` dialogs as Part 1;
  the native picker can't be driven on `:99` (portal renders on the host
  session) — see devlog 036.

## Future Improvements

- Optional: drag-and-drop hint on the welcome page; a "clear recent" action.
