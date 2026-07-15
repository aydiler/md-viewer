# Fix: Narrow tables hug their columns (#47)

**Status:** ✅ Complete
**Branch:** `fix/table-width`
**Date:** 2026-07-15
**Lines Changed:** +2 / -2 in `egui_commonmark/src/parsers/pulldown.rs` (plus comments)

## Summary

Issue #47: a table **narrower** than the content area rendered its bordered
frame stretched to full width, with the columns bunched on the left and an empty
"white column" gap after the last column. Wide tables (with a horizontal
scrollbar) were fine.

## Root cause

Both the markdown and HTML table renderers wrap `egui_extras::TableBuilder` in an
outer `ScrollArea::horizontal()` (for wide-table scroll). The `TableBuilder`
itself used `.auto_shrink([false, true])` — horizontal = `false` means "do not
shrink horizontally," so the table (and its `Frame::group` border) expanded to
fill the full `max_width` even when it only had a few narrow `Column::auto()`
columns. The leftover width became the empty gap inside the border.

## Fix

Change the **TableBuilder**'s `auto_shrink` to `[true, true]` so the table hugs
its columns' content width. The outer `ScrollArea::horizontal()` (unchanged,
still `auto_shrink([false, true])` bounded at `max_width`) continues to bound
wide tables and provide horizontal scroll.

- Narrow/medium table → hugs its columns (no empty gap).
- Wide table → content exceeds `max_width`; the ScrollArea bounds it and scrolls
  (unchanged behavior).

## Verification (Xvfb)

- Normal width: small + medium tables now hug; wide table region **pixel-identical**
  to before (ImageMagick AE = 0) → no wide-table regression.
- Narrow window (640 px): small + medium hug within the panel; wide table is
  bounded and scrollable; **no column clips without scroll**.
- `cargo test … --test wrapping` — all table/list wrapping tests pass.

## Why this is safe vs. the LESSONS warning

`docs/LESSONS.md` ("TableBuilder columns clip on narrow window without outer
ScrollArea::horizontal") required the **ScrollArea** to take `max_width` for
wide-table scroll — that ScrollArea and its `auto_shrink([false, true])` are
left untouched. Only the inner table's horizontal shrink changed, which controls
whether the table fills or hugs *inside* that ScrollArea. The `Column::auto`
width-caching concern from that lesson is a separate axis and is not touched.

**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`
(`fn table`, `fn render_html_table`)
