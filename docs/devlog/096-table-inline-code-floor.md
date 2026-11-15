# 096 — Short inline code is not cut in narrow table columns

**Branch:** `fix/table-inline-code-floor`
**Date:** 2026-11-15
**Status:** Fixed

## Problem

In a fitted Markdown table, a short identifier in a narrow column was cut
mid-word: `budget_` / `mb`, `lod_` / `bias`. Found while retaking the README
screenshots (devlog 094).

## Cause

A column's minimum width came only from its header's widest word
(`unbreakable_text_width`, #157's header floors). Body cells set no minimum.
When `fit_column_widths` squeezed a column below a body cell's code token,
egui wrapped the token at character level. Short code is never chunked by
`inline_code_wrap_segments` (only past 56 characters), so nothing else kept it
whole.

## Fix

`inline_code_floor` gives each cell's inline code of at most
`INLINE_CODE_FLOOR_CHARS` (20) characters a floor:
- the code's width in the monospace family at body size, which is how cells
  paint code (`cell_visual_lines` measures it the same way);
- plus the 16 px padding `natural_text_width` adds.

A column's minimum is now the larger of its header floor and the code floors
of all its cells. Everything downstream already takes the minimums:
`framed_table_widths`, `table_layout_key` and the dense-overflow optimizer.
When floors overflow, the table scrolls horizontally, as it already does for
header floors.

Longer code still wraps. A floor sized for a path would force horizontal
scrolling on ordinary tables, and paths are chunked on purpose (LESSONS,
"Inline-code wrap segmentation").

HTML tables carry no `Code` events, so they are unchanged.

## Tests

- `tests/wrapping.rs::fitted_columns_do_not_split_short_inline_code`: red on
  main (`budget_mb` painted on 2 rows) and green with the fix. It goes red
  again when the floor is forced to 0.
- `inline_code_floor_covers_short_code_only`: no floor for prose, a floor at
  least the code's width for short code, and no floor for a 30-character token.
