# Feature: right-to-left task-list items

**Status:** ✅ Complete
**Branch:** `feature/rtl-task-items`
**Date:** 2026-11-27
**Lines Changed:** renderer `parsers/pulldown.rs` (`block_is_rtl`, `RtlBlock`,
`render_rtl_item`, `finish_rtl_item`, the `TaskListMarker` arm),
`tests/rtl_alignment.rs`

## Summary

One of the gaps #108 left (devlogs 091, 092): a list item whose text is right
to left but starts with a task checkbox, `- [ ] מטלה`, kept the left-to-right
path: the item sat at the left edge, and in md-viewer (whose fonts shape the
text right to left) its text was drawn over its own checkbox. It now mirrors
a left-to-right task item: marker at the right edge, checkbox left of it, text
left of the checkbox.

## Features

- [x] `block_is_rtl` accepts a `TaskListMarker` as an item's first content
  event (or the first after a loose item's `Start(Paragraph)`). Anywhere else it
  still sends the block down the left-to-right path. The direction still comes
  from the first strong character of the text.
- [x] `RtlBlock::task` keeps the checkbox state and the marker's source span; the
  `TaskListMarker` arm stores it there for a right-to-left item instead of adding
  the checkbox to the row.
- [x] `render_rtl_item` draws the checkbox in a child ui laid out right to left
  from the marker's gap, bottom-aligned on the item's first line, then lays out
  the text up to its left. In a mutable view a click returns the new state and
  `finish_rtl_item` records the same `CheckboxClickEvent` the left-to-right arm
  does, so `show_mut` toggles `[ ]` in the source.

## Key Discoveries

### A wrapping label's galley starts at the row, not at the text

The first test measured the left-to-right text from its galley rect and found
the text "starting" at the checkbox's own left edge. A label in a wrapping row
is laid out as a full-width galley anchored at the row's left edge, with the
first row indented past the widgets before it (LESSONS, list markers). The
tests measure the first row's glyphs instead.

### The checkbox's pad is on the text side

Both checkboxes allocate at least the interact height (20 px) in width but
paint a 14 px square at the allocation's left (`Spacing::icon_rectangles`).
Left to right, the remaining 6 px sit between square and text. Laid out right to
left with no correction, the pad landed between square and marker and the text
touched the square. The allocation is shifted right by the pad, so the square
ends at the marker's gap and the text ends the pad before it: the measured gap
matches the left-to-right one.

### A child ui keeps the row intact

`render_rtl_block` starts the text at the parent's cursor. Drawing the checkbox
with `Ui::new_child` places it without advancing that cursor.

## Testing Notes

In `tests/rtl_alignment.rs`:
- `rtl_task_item_mirrors_the_ltr_task_item`: text < checkbox < marker ≤ right
  edge; checkbox-to-text gap within 2 px of the left-to-right item's; the
  checkbox at the same height on its row.
- `loose_rtl_task_items_put_their_checkboxes_right_of_the_text`.
- `clicking_an_rtl_task_checkbox_toggles_it_in_the_source`: a press and release
  on the box through `show_mut` turns `- [ ]` into `- [x]`.
- `rtl_items_follow_their_lead_text` now expects task items to be marked.

All three new tests fail on main. Mutations, each turning a named test red:
accepting no task marker (all three), no pad on the text side (the mirror
test's gap check), dropping the click event (the click test).

On Xvfb, md-viewer with a Hebrew and Arabic task list (tight, nested, loose),
dark and light: on main the item text overlaps its checkbox at the left edge;
on the branch each item reads text, checkbox, marker from the right edge.

## Future Improvements

- [ ] Paragraphs with an inline formula, image or HTML in right-to-left text
  still take the left-to-right path (devlog 129 plans formulas).
