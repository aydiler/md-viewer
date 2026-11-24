# Feature: Explorer leftovers from #288 / #290

**Status:** ✅ Complete
**Branch:** `fix/explorer-leftovers`
**Date:** 2026-11-24
**Lines Changed:** `src/main.rs` (`FileExplorer::refresh`, `watch_directories`, `MarkdownApp::note_explorer_watch_cut`, two tests)

## Summary

Two small defects left by the bounded Expand All (#288, devlog 118), both in
code that has not been released yet, so neither needs a CHANGELOG entry of its
own:

1. **Refresh during Expand All dropped the expansion.** `refresh` started a
   plain root scan, which replaced `pending_scan`; the Expand All result was
   thrown away and nothing told the user. The ⊞ button is disabled while a scan
   runs, but ↻ was not.
2. **The watch budget could be spent on folders that no longer exist, and
   its warning repeated.** `watch_directories` cut the expanded set to 256
   *before* filtering out missing folders, so deleted folders that sorted
   first took slots and were then dropped, leaving existing folders unwatched.
   It also logged its warning on every reconcile, i.e. on every expand or
   collapse once more than 256 folders were open.

## Features

- [x] `refresh` returns early while an Expand All scan is pending and not
      discarded. That scan rescans the same root, so the refresh is not lost.
- [x] `watch_directories` filters `is_dir` before cutting, and returns whether
      it cut: `(Vec<PathBuf>, bool)`.
- [x] `MarkdownApp::note_explorer_watch_cut` logs the warning only when the
      set starts being cut (`explorer_watches_cut` flag), called from
      `start_watching_attempt` and `reconcile_explorer_watches`.
- [x] Tests, both observed red on main first:
      `refresh_keeps_a_pending_expand_all` (0 expanded instead of 20) and
      `watch_budget_skips_missing_folders_before_cutting` (only the root
      watched instead of root plus 256).

## Key Discoveries

### A bulk scan and a plain scan share one slot

`pending_scan` holds one receiver. Anything that starts a scan while another is
pending discards the first silently: the worker thread finishes and its send
fails. The bulk scan already rescans the root, so letting it win loses
nothing; the opposite order (Expand All while a refresh is pending) is
already prevented because ⊞ is disabled while any scan runs.

## Future Improvements

- The Explorer still lays out every listed row each frame; devlog 120 makes it
  draw only the visible rows.
