# Feature: Right-to-left list items and table cells (#108)

**Status:** ✅ Complete for list items and table cells; task items and widget-holding items stay left-aligned
**Branch:** `feature/rtl-lists`
**Date:** 2026-11-15

## Summary

Devlog 091 right-aligned RTL paragraphs and headings but left list items and
table cells on the left. Now a list item whose lead text is right to left is
drawn mirrored: text flush right, its marker to the right of the text, nested
items indented from the right. A table cell whose text is right to left is
right-aligned inside its column. Column order stays as written.

## Features

- [x] RTL bullet, hollow-bullet and numbered items: marker on the right,
  numbers drawn `.1` (the visual order of `1.` on a right-to-left line).
- [x] Nested RTL items indent from the right edge.
- [x] Tight and loose lists.
- [x] RTL table cells right-aligned, each cell judged on its own.
- [ ] Task-list items (`- [ ] …`): stay left-aligned with the checkbox on the left.
- [ ] Items and cells containing an image, inline math, inline HTML or a
  footnote reference: stay on the per-label path, like paragraphs (091).
- [ ] Paragraphs after an RTL item's first one (loose lists), footnote and
  definition-list bodies: stay left-aligned.

## Key Discoveries

### An item is judged by its lead text
`rtl_block_starts` records `Start(Item)` when the text before the item's
first nested block (or its first paragraph, in a loose list) starts with a
right-to-left letter. `block_is_rtl` now stops at any block start other than
that opening paragraph, so an English item with a Hebrew sub-list is LTR and
its sub-item RTL.

### The item's lead is collected like a paragraph
`List::start_item(.., rtl = true)` reserves no indent and no marker slot and
returns the marker kind instead. The item's text goes into `rtl_paragraph`;
the lead ends at the first non-inline `Start` tag, at `End(Paragraph)` of a
loose item, or at `End(Item)`, where `render_rtl_item` draws the galley
ending at `column_right - indent - marker - gap` and paints the marker in the
slot to its right. The marker's baseline comes from the laid-out galley's
first row, not predicted from the cursor as `paint_list_marker` must.

### A row's first cell carries the row's start event
`parse_row` keeps every event but `End(TableCell)`, so the first cell of a
row begins with `Start(TableRow)` (or `Start(TableHead)`), not
`Start(TableCell)`. The cell lookup takes the first `Start(TableCell)` in the
cell's events.

### A nested item's indent includes its parent's marker
A left-to-right nested item is laid out inside its parent's content area,
which already starts past the parent's marker slot and gap. Its step from the
parent is therefore marker + gap + indentation spaces: 33.2 px at the test
defaults, 29 px in the app (2 spaces). The first mirrored version subtracted
only the spaces (14.6 px, 8 px in the app), visible in an Xvfb screenshot as
nested bullets barely moving. The nested test now compares the RTL step with
the LTR step directly.

### Tables stretch to the pane
Since #211 a table fills the pane, so a header's text width says nothing
about where its column ends. The first table test assumed it did, and its
failure (the cell ending at 789 against a header ending at 532) briefly read
as the cell's row being wider than the cell; a mutation using
`max_rect().right()` instead of `cursor().left() + col_w` showed both give
the same edge. The table test measures a column's right edge as
the start of the next column's header.

## Architecture

- `egui_commonmark_backend/src/elements.rs`: `paint_rtl_list_marker`,
  `list_marker_width`, `body_spaces_width`.
- `egui_commonmark/src/lib.rs`: `List::start_item` takes `rtl` and returns
  the deferred marker kind; `List::depth`.
- `egui_commonmark/src/parsers/pulldown.rs`: `rtl_block_starts` /
  `block_is_rtl` cover items and cells; `render_rtl_block` returns its galley
  and rect; `render_rtl_item`, `is_inline_tag`, `finish_rtl_item`; fields
  `rtl_item`, `next_item_rtl`; the table cell closure collects RTL cells.

## Testing

- `tests/rtl_alignment.rs`: bullet item text and marker at the right with an
  LTR item in the same list unchanged; nested RTL items step in from the
  right exactly as far as LTR items step in from the left;
  ordered item's number right of its text; loose items; RTL cell ends at its
  column's right edge, LTR cell unchanged.
- Unit tests: item lead (tight, loose, nested, task item, image), cells
  judged one by one.
- Mutations, each on `--test rtl_alignment`: items not detected (4 red),
  cells not detected (table red), marker not painted (bullet and number
  tests red), no nesting step (nested red), cell edge 100 px left (table
  red). A cell edge 100 px *right* stays green: `render_rtl_block` clamps to
  `max_rect().right()`, so it cannot overshoot.
- Xvfb, against current `main`: Arabic bullets with a nested list (bold and
  a link in the nested item; on `main` that item's pieces overlapped), Arabic
  and Hebrew numbered items (`.1`, `.2`), an English item and an English
  numbered item unchanged, a task item unchanged, a table with Arabic,
  Hebrew and English cells. Spacing identical to `main`. Clicking the link in
  the nested RTL item opened `other.md`.

## Future Improvements

- Mirror the whole table (first column on the right) when its header row is
  right to left.
- Task-list items with the checkbox on the right.
