# Feature: Non-recursive explorer-root file watching

**Status:** ✅ Complete
**Branch:** `fix/explorer-watch-nonrecursive`
**Date:** 2026-06-19
**Lines Changed:** +79 / -16 in `src/main.rs`

## Summary

Launching md-viewer hung for ~6 s before the first frame whenever the file
explorer root was a large tree (e.g. `/home/ahmet`). The watcher set up in
`start_watching()` registered the explorer root with
`notify::RecursiveMode::Recursive`; `notify`'s inotify backend implements
recursive watching by **walking the entire directory tree on the calling
thread and issuing one `inotify_add_watch` per directory**. Since
`start_watching()` runs synchronously inside `MarkdownApp::new()` (before the
eframe event loop), the whole walk blocked the first paint, and KDE flagged the
window as "not responding" (DrKonqi ANR marker timestamped at the exact launch).

`/home/ahmet` has **454,709 directories** — a cache-warm walk alone measured
**6.11 s**, and the watch consumed ~87 % of the inotify watch budget (524,288).

The fix makes watching mirror the explorer's already-lazy tree: watch the root
**plus each currently-expanded directory, non-recursively**. A non-recursive
inotify watch on a directory still reports create/delete/modify of its *direct*
entries, which is exactly the set of changes the user can see. Changes inside
collapsed/unwatched subtrees aren't visible anyway, so not refreshing for them
is correct (and removes the old spurious whole-tree refreshes for invisible
deep changes).

## Features

- [x] Startup no longer walks the whole root subtree (sub-100 ms vs ~6 s)
- [x] Live tree updates preserved for visible (root + expanded) directories
- [x] Open-tab live reload preserved (tab files were already watched individually)
- [x] Expand/collapse keeps watches in sync incrementally (no watcher rebuild)

## Key Discoveries

### `notify` recursive watch walks the tree synchronously on the caller thread

The cost isn't the inotify kernel side per se — it's that `RecursiveMode::Recursive`
makes notify `walkdir` the whole subtree and call `inotify_add_watch` for every
directory, inline in the `watch()` call. On a huge home directory that's
hundreds of thousands of syscalls before `watch()` returns. Moving it off-thread
would only hide the cost; the right fix is to not watch what isn't visible.

### Open-tab reload never depended on the recursive root watch

Tab files are always watched individually (`get_open_tab_paths()` →
`RecursiveMode::NonRecursive`). The recursive root watch was redundant for tab
reloads, so removing it doesn't affect live reload of open documents.

### The tree-refresh trigger needs no change

`reload_changed_tabs` decides to refresh the explorer via
`path.starts_with(root)`. Events still originate only under `root` (from the
root + expanded-dir watches), so the check and the parent-flashing walk keep
working unchanged.

## Architecture

### Modified Structs

```rust
// MarkdownApp: was a single Option<PathBuf> for the recursively-watched root.
// Now the set of non-recursively-watched explorer dirs (root + expanded).
watched_explorer_dirs: HashSet<PathBuf>,
```

### New Functions

| Function | Purpose |
|----------|---------|
| `reconcile_explorer_watches()` | Incrementally add/remove the non-recursive explorer-dir watches (root + expanded dirs) against the live watcher when the expanded set changes — mirrors `update_watched_paths`'s diff so expand/collapse doesn't tear down the debouncer + bridge thread. |

### Changed flow

- `start_watching()` builds `explorer_dirs = root + expanded_dirs` (local,
  existing) and watches each `NonRecursive`.
- `reconcile_explorer_watches()` is called after `toggle_expanded`, `expand_all`,
  and `collapse_all`. `open_folder_dialog`, F5, and toggle-explorer already call
  `start_watching()` (full rebuild), so they need no change.

## Testing Notes

Verified on Xvfb `:99` with the debug build and an **isolated** `XDG_DATA_HOME`
(the real session state was not touched):

- **Startup (the bug):** with `explorer_root = /home/ahmet`, time-to-window
  **0.11 s** (was ~6 s); process held **10** inotify watches (root + tab + the
  existing expanded dirs) vs ~455,000 for the recursive watch.
- **Live tree update:** explorer root `/tmp/mdv-fixture` with `sub/` expanded;
  externally creating `sub/new.md` made it appear in the tree (the non-recursive
  `sub/` watch fired the refresh).
- **Tab reload:** editing the open `root.md` externally updated the rendered
  content live ("EDITED LIVE").
- `cargo build` + `cargo clippy -p md-viewer`: clean (only pre-existing
  vendored-crate warnings).

## Future Improvements

- [ ] `expand_all()` on a huge root still calls `load_all_children` (depth-10
  recursive shallow scans) and is inherently heavy — pre-existing, user-initiated.
  Could be made lazy/bounded if it becomes a problem.
- [ ] Optionally call `reconcile_explorer_watches()` after a watch-driven
  `refresh()` to prune watches for externally-deleted expanded dirs sooner
  (inotify already auto-drops watches on deleted dirs, so this is cosmetic).
