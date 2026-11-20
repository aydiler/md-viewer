# Table cells that fit on one row are no longer laid out to count rows

**Status:** 🚧 In progress
**Branch:** `perf/table-row-count-shortcut`
**Date:** 2026-11-20

## Summary

The table column-width optimizer (#157, #182) scores candidate column widths
by measuring every cell's wrapped height at each candidate width. Each
measurement laid the cell text out in full at that width, only to count its
rows. On a document of 300 ordinary tables, that was more than half of all
instructions before the first frame.

`wrapped_text_height` now asks `fits_on_one_row` first. When the cell's
unwrapped width (already laid out for the frame by `natural_text_width`) is
narrower than the wrap width, the answer is one row and nothing is laid out.

## Measurements

Profile (callgrind, release build with line tables, the first 30 tables of the
startup `tables` fixture, on Xvfb). Share of all instructions up to the first
content frame on main:

| function (inclusive) | share |
|---|---|
| `optimize_fitted_widths_with_cache` | 55.3 % |
| `table_cell_height` | 53.9 % |
| `wrapped_text_height` | 52.7 % |

## Key Discoveries

- **The shortcut is exact, not an estimate.** epaint keeps a paragraph on one
  row when its width is at most `effective_wrap_width()`, which is `max_width`
  (or `max_width + 0.5` with `round_output_to_gui`). The galley's reported
  width is within 0.5 of the paragraph width. "Unwrapped width + 0.5 < wrap
  width" therefore implies one row. Text near the boundary, and text with a
  newline (several paragraphs), takes the full layout as before.

## Architecture

- `fits_on_one_row(ui, text, font_id, wrap_width)` in
  `egui_commonmark/src/parsers/pulldown.rs`.
- `wrapped_text_height` returns `line_height` when it is true. Its other
  callers (HTML tables) get the same shortcut.

## Testing

- `one_row_shortcut_matches_a_full_layout`: thirteen texts (empty, single
  words, punctuation, CJK, Hebrew, emoji, tabs, leading and trailing spaces),
  each swept in quarter-point steps from 6 pt below to 6 pt above its natural
  width; the shortcut must never claim one row where a full layout gives more,
  and `wrapped_text_height` must equal the full layout's height.
- `one_row_shortcut_answers_for_text_that_clearly_fits`: the shortcut does fire
  where it should, so the guard above cannot pass by never firing.
