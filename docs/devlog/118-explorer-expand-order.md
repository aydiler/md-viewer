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
- The explorer render pre-loads children by walking the tree once per expanded
  folder per frame (issue #287, option 4). At 256 folders it measures 0 % idle;
  raising the budget would need that fixed first.
