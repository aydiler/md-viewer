# Fix: table columns no longer narrow a moment after a table appears

**Status:** ✅ Complete
**Branch:** `fix/table-cap-bound`
**Date:** 2026-11-25
**Lines Changed:** renderer `parsers/pulldown.rs` (both table cap sites), `tests/table_measure_state.rs`

## Summary

Found while verifying devlog 122: every column of a Markdown table that fills
its pane shrank by 2 pt on the table's third painted frame, so the table
stopped 2 pt per column short of the pane edge, and a cell whose text just
fit could wrap. Whether it showed depended on timing: a reflow (for example
when math images in the table finish loading) resets the widths, and if it
came before the shrink the table kept its fitted widths.

## Cause

The separator-drag caps (#201, #211) let a column grow only into space the
others are not using. They were computed against

```rust
usable_bound_now = table_bound - frame.total_margin().sum().x - frame.stroke.width * 2.0
```

`Frame::total_margin()` already includes the stroke (egui 0.37
`containers/frame.rs`), so the stroke was subtracted twice. The fitted widths
fill `framed_table_widths`' budget, `table_bound − total_margin − column
spacing`, which is 2 pt more. Since tables stretch to the pane (#211), the
widths summed to exactly that budget and every cap came out 2 pt under its
column. egui_extras clamps its state widths to `at_most` at the end of the
table body, so the second frame stored the clamped widths and the third
painted them.

The HTML-table site had the same bound, but its caps also never go below the
column's last width (growth-only, from #201), so HTML tables did not shrink.

## Fix

Both cap sites use `visible_column_budget`, the budget the fitted widths
fill, which `framed_table_widths` already returns. A table that fills its
budget gets caps equal to its widths; a dragged column can still grow up to
the pane edge, now exactly to it. A non-zero `item_spacing.x` (library users
other than md-viewer, which sets 0) is accounted for too: the old bound
ignored the spacing between columns.

## Tests

- `a_table_keeps_its_column_widths_across_frames`
  (`tests/table_measure_state.rs`): the stored widths of a table that fills
  its pane are the same after frames 0–5. On main it fails at frame 2:
  `[112.8, 100.7, 372.5]` → `[110.8, 98.7, 370.5]`.
- `tests/pane_clip.rs` stays green, including
  `dragging_a_table_column_rightward_never_paints_past_the_pane` and
  `table_refits_fully_after_the_sidebar_snaps_back`.

## In the app

Xvfb, optimized builds, main as control, the visual-regression table (four
columns, 80 rows) in a 1040 px window:

- Settled: main's table stops 8 px (4 columns × 2 px) short of where this
  branch's ends, at the pane edge. Both wrap the notes at this width.
- Dragging the first column separator 300 px to the right: neither build
  paints past the pane. On this branch the drag cannot widen the column,
  because the table already fills its budget (a column grows only into space
  the others leave). On main it widened by the pixels the shrink had freed.
- `scripts/visual-regression.sh`: PASS on both. This branch paints more
  content pixels (3,257 / 5,386 at the top and the end, main 3,056 / 5,207):
  its table reaches the pane edge, so the frame's right border and the row
  stripes extend 8 px further. The notes fit on one line in both at this
  window width.
- `scripts/scroll-regression.sh`: PASS on both, bottom reached at frame 68 of
  150 on each.

## Key Discoveries

- `Frame::total_margin()` is inner margin + stroke + outer margin. Code that
  subtracts it and then the stroke again is 2 × stroke short.
- A cap applied through egui_extras' `Column::at_most` shrinks a column even
  when nobody drags: the clamp runs on every frame's state widths.
