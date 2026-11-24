# Feature: Explorer draws only the visible rows

**Status:** ✅ Complete
**Branch:** `perf/explorer-visible-rows`
**Date:** 2026-11-24
**Lines Changed:** `src/main.rs` (`explorer_row_height`, `FileExplorer::visible_rows`, `render_file_explorer`, `render_tree_row`, one test)

## Summary

The Explorer laid out every listed row on every frame, recursively, and before
that searched the tree twice per expanded folder to pre-load children. Pointer
motion over a large tree therefore cost CPU in proportion to the whole tree,
not to what was on screen (devlog 118 measured 72 % of a core for 6,400 rows).

It now flattens the expanded tree into a list of `(depth, &node)` once per
frame and draws only the rows in view with `ScrollArea::show_rows`, the way the
outline already does.

## Features

- [x] `FileExplorer::visible_rows`: the rows in display order (same order as
      the old recursion), with depth; skips collapsed subtrees and collects
      expanded folders whose children are not loaded yet. It borrows the
      nodes, so there is no cache to invalidate.
- [x] `render_file_explorer` uses `show_rows(row_height, rows.len())`. The
      unloaded folders are loaded after the panel, which replaces the
      per-frame pre-load loop (two tree searches per expanded folder per
      frame, and it kept searching for expanded paths that no longer exist).
- [x] `render_tree_node` became `render_tree_row`: one row, no recursion, each
      row exactly `row_height` tall.
- [x] `explorer_row_height`: `show_rows` needs one height for all rows. Folder
      names use the Body text size, file names the button text size, so the
      height is the largest of `interact_size.y`, the Body row and the button
      row with its padding.
- [x] `get_children` / `find_children_in_tree` are only used by tests now
      (`#[cfg(test)]`).
- [x] Test `visible_rows_follow_the_expanded_tree_in_display_order`.

## Measurements

Optimized build (LTO off, opt-level 3), Xvfb, main-thread CPU during 5 s of
pointer motion over the Explorer, two runs each. Tree: 1,024 folders × 20
subfolders (`pr288` fixture).

| case | before | after |
|---|---|---|
| unexpanded (1,024 rows) | 24–25 % | 13–14 % |
| after Expand All (stops at 2,048 rows) | 35 % | 13–14 % |
| restart with 256 wide folders restored (~6,400 rows) | 73 % (run 2) | 18 % |

Run 1 of the restored case read 34 % with only 124 pointer moves against 204
elsewhere, so that run did not exercise the same motion; run 2 is the
comparable one.

Screenshots of the restored tree at text size Normal and the largest size, at
the top, the middle and the end, match the old drawing. The only visible
difference is that file rows are now as tall as folder rows (25 px instead of
24 px at Normal). The end of the list is reachable, and the open tab's file is
highlighted.

Behaviour checked on both builds with the same steps: clicking a file opens
it, a changed file flashes, right-click shows the Copy menu, and middle-click
closes the file's tab. `scripts/hover-stability.sh` (which uses the Explorer's
scrollbar at the panel divider) was run on both builds; results are in the PR.

## Key Discoveries

### The flash overlay was painted for rows nobody could see

The changed-file flash uses `debug_painter`, which is clipped only to the
window. With every row laid out, a flashing row scrolled out of view painted
its highlight over whatever was there. Rows outside the view are no longer
laid out, so this cannot happen.

## Future Improvements

- `BULK_ROW_LIMIT` (2,048) was set because drawing cost grew with the row
  count. Flattening is still O(rows) per frame, but it is pointer pushes; the
  limit could be raised after measuring a larger tree.
