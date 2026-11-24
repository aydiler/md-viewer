# Feature: Explorer expansion budget, breadth first and current root first

**Status:** ✅ Complete
**Branch:** `fix/explorer-expand-order`
**Date:** 2026-11-24
**Lines Changed:** `src/main.rs` (`FileExplorer::expand_children`, `graft_children`, `restore_expansion`, two tests)

## Summary

#288 (RichardCao, fixing #287) bounded Explorer's Expand All and the restored
expansion to 256 folders and moved the bulk scan off the UI thread. Reviewing it
on Xvfb confirmed the fix and found two ways the 256 slots were spent badly:

1. **Expand All went depth first.** With `a/` holding 330 subfolders next to
   `b/` … `z/`, all 256 slots went into `a/`; `b/` … `z/` stayed collapsed.
   0.3.12 had expanded all 381.
2. **Saved folders from an earlier root used up the restore budget.**
   `set_root` never clears `expanded_dirs`, so the saved list carries folders
   of roots opened before. They sorted by depth and name only, so with 300 of
   them and 1,024 under the current root, the restore kept 256 old-root
   folders and none under the current root: the notice appeared and the tree
   came back fully collapsed.

Both are fixed here; #288 itself was merged first, as is.

## Features

- [x] `expand_children` walks breadth first: a queue of paths, each scanned
      with `scan_directory_shallow`, then `graft_children` attaches the scanned
      lists to the tree. The depth limit (10) and the budget (256) are the same
      as #288's; only the order changed.
- [x] Expand All also stops before the Explorer would list more than
      `BULK_ROW_LIMIT` (2,048) rows. Breadth first made the per-frame cost
      visible (see below); the notice now reports the number of folders that
      were actually expanded, since either budget can end the walk.
- [x] `restore_expansion(paths, root)` sorts folders under `root` first when it
      has to cut, then parents before descendants, then by path. Under the
      budget nothing is dropped, so going back to an earlier root still finds
      its folders open.
- [x] Tests, both observed red on #288's code first:
      `expand_all_opens_shallow_directories_before_deep_ones` and
      `saved_expansion_keeps_the_current_root_when_it_is_cut`.

## Measurements (#288 vs 0.3.12, Xvfb, debug builds)

Fixture: 1,024 top-level folders × 20 subfolders each, Watch Files on.

| | 0.3.12 | #288 |
|---|---|---|
| Expand All: a key pressed 0.3 s after the click is handled after | 115 s, all of it on the UI thread | 0.9 s |
| inotify watches afterwards | 21,506 | 258 |
| idle CPU afterwards | 100 % of a core | 0 % |
| edit to the open document reloads | not within 3 s | yes |
| Ctrl+Q afterwards | no exit within 60 s | exits |
| restart with 1,024 saved folders: idle CPU | 99 % | 0 %, notice once, 256 kept |

## Key Discoveries

### A folder budget does not bound the per-frame cost; rows do

Breadth first on the wide fixture opened 256 top-level folders with 21
entries each: about 6,400 listed rows instead of the ~1,300 #288's depth-first
walk left (most of its 256 were empty leaf folders). The Explorer lays out
every row each frame (`ScrollArea::show`, no row virtualization), so the cost
moved from idle to interaction. Main-thread CPU while the pointer moved over
the Explorer for 5 s (~42 moves/s), optimized build without LTO, two runs each:

| | CPU |
|---|---|
| no expansion (1,024 rows) | 22–23 % |
| #288, depth first | 30 % |
| breadth first, folder budget only | 72–73 % |
| breadth first + 2,048-row budget | 34–35 % |

The debug build saturated in both of the first two cases (82 % and 95 %), so
it could not tell them apart; the optimized build and the unexpanded control
were needed to see the difference at all. The proper fix is to virtualize the
Explorer's rows (#287's option 4); the row budget keeps Expand All inside what
the current renderer handles.

### Breadth first needs the paths, not the nodes

A queue of `&mut FileTreeNode` would borrow the whole tree for the length of
the walk. The walk only needs paths: scan each directory into a map, then graft
the map onto the tree in one depth-first pass. Each scanned list is grafted
before its own children are looked up, so deeper levels attach in the same
pass.

### Two measurement traps (recorded in LESSONS)

- **The first "PR binary" was main's.** Both worktrees built into one
  `CARGO_TARGET_DIR`, and cargo keeps dep-info paths relative to the package,
  so the second worktree's older sources looked fresh and nothing was rebuilt.
  `strings` on the binary for a string only the PR has caught it.
- **`xdotool key ctrl+d` never toggled the theme.** egui's shortcut reads the
  modifier state at the end of the frame's events, so a chord that starts and
  ends inside one frame loses Ctrl. On a frozen window every queued event lands
  in one frame, so the probe must hold Ctrl down (`keydown ctrl`, `key d`,
  `keyup ctrl` after the response) or it measures nothing on the very build it
  exists for.

## Future Improvements

- Refresh while an Expand All scan is still pending replaces the pending scan
  and drops the expansion; ↻ could be disabled like ⊞ while it runs.
- `watch_directories` logs its warning on every reconcile once manual
  expansion passes 256, and truncates before the `is_dir` filter.
- A restore is bounded by folders only: 256 restored folders with 20 entries
  each still list about 6,400 rows (the saved state an old Expand All left).
  Virtualizing the rows fixes that too.
- Virtualize the Explorer: render only the visible rows from a flattened list
  cached on change (`show_rows`), and drop the per-frame pre-load walk that
  looks up every expanded folder in the tree (issue #287, option 4). Then the
  row budget, and probably the folder budget, can grow.
