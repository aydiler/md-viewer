# Feature: Open Directory (issue #28, part 1)

**Status:** Implemented
**Branch:** `feature/open-directory`
**Date:** 2026-06-08
**Issue:** aydiler/md-viewer#28

## Summary

Adds **File → Open Folder…**, letting the user point the file explorer at any
directory at runtime. Previously the explorer root was derived only once at
startup (CLI file's parent → persisted root → first tab's parent → cwd), with no
way to change it without opening a file located elsewhere.

This is **part 1** of issue #28. The welcome/idle page (part 2) is tracked
separately.

## User-Facing Behavior

- File → Open Folder… opens a native directory picker.
- Choosing a directory repoints the explorer to it (shallow rescan; GVFS roots
  scan in a background thread, as elsewhere).
- The explorer sidebar is shown if it was hidden, so the result is visible.
- The chosen root is persisted (`explorer_root`) and restored next session.

## Architecture

- New `MarkdownApp::open_folder_dialog()` — `rfd::FileDialog::pick_folder()` →
  `FileExplorer::set_root()` → ensure explorer visible → rebuild watcher.
- Reuses existing machinery: `set_root()` already rescans (and offloads GVFS
  scans to a thread); `save()` already persists `file_explorer.root`.
- Watcher: calls `start_watching()` (full teardown + rebuild) when watching is
  enabled, so the new root is watched recursively. `update_watched_paths()` only
  reconciles tab paths and would not pick up a changed explorer root.

## Key Discoveries

- The explorer root is watched **recursively** via inotify (local only); GVFS
  roots are intentionally not recursively polled (SFTP roundtrip cost). The new
  picker inherits this behavior for free through `start_watching()`.

## Testing Notes

- `cargo build` — clean.
- `cargo clippy --bin md-viewer` — only the two pre-existing vendored warnings
  (`unused variable: max_width`, deprecated `allocate_ui_at_rect`); no new ones.
- Live (Xvfb `:99`): File → **Open Folder…** is present and correctly placed
  (right after "New Tab…") — confirmed by screenshot. Clicking it invokes the
  native picker — confirmed with the default xdg-portal backend (the synchronous
  `pick_folder` blocked the UI thread and the portal request fired). The
  folder→repoint step reuses `FileExplorer::set_root()`, which was demonstrated
  live (opening a file populates the explorer with that file's directory).

### Harness limitation (couldn't fully drive the picker on :99)

The native OS folder picker can't be driven to completion on the virtual
display: rfd's default **xdg-portal** backend renders the dialog on the *host*
session (D-Bus portal), not `:99`, and a throwaway build with rfd's **gtk3**
backend silently no-ops in this headless winit+glow context (no chooser window,
no error). So the full click-through-to-repoint screenshot was not captured on
`:99`; the repoint itself is covered by `set_root`'s already-demonstrated
behavior. This is a test-harness limitation, not a code issue.

## Future Improvements

- Optional keyboard shortcut — no obvious free combo (Ctrl+Shift+E/O are the
  explorer/outline toggles), so menu-only for now.
- Welcome page (issue #28 part 2) will add an "Open Folder" button that calls
  this same method.
