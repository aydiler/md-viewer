# Feature: Close Folder and Recent Folders

**Status:** ✅ Complete
**Branch:** `feature/close-folder-and-recents` (#297, contributed by @RichardCao)
**Date:** 2026-11-27
**Lines Changed:** `src/main.rs` (+315 / −55 for the PR, +13 / −5 for the review fix)

## Summary

The Explorer always had a root. If nothing else gave one, it fell back to the
working directory, so there was no way to close a folder and no way to get back
to one opened earlier. #297 adds:

- File → Close Folder;
- a Recent Folders list in the empty Explorer;
- removal of single entries and Clear History, for both Recent Folders and the
  welcome page's Recent Files.

The File menu now groups opening and closing files apart from opening and
closing folders.

## Features

- [x] File → Close Folder drops the Explorer's tree, its pending background scan
  and its directory watches. Open documents stay open and keep their file
  watches.
- [x] The closed state is saved, so a restart (including one with a Markdown
  file on the command line) keeps the Explorer empty until a folder is opened.
- [x] Recent Folders (at most 20) lists folders opened explicitly with Open
  Folder or from the list. Roots inferred from a document or the working
  directory are not recorded. Reopening an entry moves it to the top; a folder
  that no longer exists shows "Folder is unavailable".
- [x] ✕ on a row removes that entry; Clear History empties the list. The same
  controls are on the welcome page's Recent Files.
- [x] A row reserves the ✕ button's width before laying out the folder name, so
  the Explorer divider does not move while a long name's tooltip is shown.

## Key Discoveries

### Stopping the watcher made the next file look like a failure

The PR first called `stop_watching()` when Close Folder ran with no document
open. On main a watcher always exists, because the root falls back to the
working directory, and `update_watched_paths` only adds paths to an existing
watcher. So the next file opened was not watched until `check_file_changes`
saw no receiver, treated that as a failed watcher and recovered:

```
Started watching 0 local (inotify) + 0 GVFS (poll) files, 1 explorer dirs
Stopped watching files
Attempting to recover file watcher (attempt 1)
Started watching 1 local (inotify) + 0 GVFS (poll) files, 0 explorer dirs
```

Reload still worked, but it logged a recovery for something that had not failed
and used one of the three retries. `close_folder` now always calls
`reconcile_explorer_watches()`, which removes the directory watches and leaves
the watcher in place.

### A capped list hides a missing dedupe

`recent_folders_are_deduplicated_and_bounded` pushes 21 folders and then one
again. With `retain` removed from `push_recent_folder` it still passed: the
re-pushed folder lands at the front, and its old copy is the one `truncate`
cuts. `recent_folders_move_a_repeated_folder_to_the_front` checks the dedupe
with no cap in the way (`a`, `b`, `a` → `[a, b]`).

## Architecture

### New/Modified Fields

```rust
struct PersistedState {
    explorer_closed: Option<bool>,          // None (older state) = not closed
    recent_folders: Option<Vec<PathBuf>>,   // None (older state) = empty
    // …
}

struct MarkdownApp {
    recent_folders: Vec<PathBuf>,           // newest first, at most RECENT_FOLDERS_CAP
    // …
}
```

`FileExplorer::root` was already an `Option`, but it was `None` only when no
fallback existed. It is now `None` whenever the user has closed the folder.

### New Functions

| Function | Purpose |
|----------|---------|
| `initial_explorer_root()` | Startup root: `None` if the folder was closed, else the CLI file's folder → saved root → first tab's folder → working directory |
| `push_recent_folder()` | Moves a folder to the front of the history, without duplicates, capped at 20 |
| `FileExplorer::close_root()` | Clears root, tree, expanded set and pending scan |
| `MarkdownApp::open_folder()` | Opens a folder from the dialog or the history and records it |
| `MarkdownApp::close_folder()` | Closes the root and reconciles the directory watches |
| `MarkdownApp::render_empty_explorer()` | "No folder open", Open Folder… and the Recent Folders list |

## Testing Notes

- Unit tests: `closing_explorer_drops_the_tree_and_pending_scan`,
  `explicitly_closed_explorer_stays_closed_when_opening_a_document`,
  `recent_folders_are_deduplicated_and_bounded`,
  `recent_folders_move_a_repeated_folder_to_the_front`. Each turns red under
  the mutation it guards (pending scan kept, `closed` ignored, cap removed,
  dedupe removed).
- Xvfb, PR against main:
  - Close Folder with a document open: watches 2 → 1, and the document still
    reloads.
  - Close Folder with none open, then open a file from Recent Files and edit it:
    watches 0 → 1, it reloads, and the log has only the initial
    "Started watching" line.
  - A state file written by main restores its root as before.
  - Recent Folders reopen, removal and Clear History persist across a restart.

## Future Improvements

- [ ] Dim missing folders in Recent Folders, as the welcome page does for
  missing files.
- [ ] Register the new ✕ buttons and history rows for MCP testing once the
  `mcp` feature builds against egui 0.37 again.
