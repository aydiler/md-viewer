# 095 — Table rows with text and inline math

**Branch:** `fix/table-math-height`
**Date:** 2026-11-15
**Status:** Fixed

## Problem

A table cell holding text and an inline formula, such as a header
`$\chi^2$ (60 pts)`, got a row about three lines tall for one line of content.
Found while retaking the README screenshots (devlog 094).

## Cause

`table_cell_height` reserves a formula's height *on top of* the text height
whenever a cell has anything besides a single formula. The comment says why:
without formula widths the code could not tell whether the pieces share a row,
so it assumed they did not. For `$\chi^2$ (60 pts)` that is the text line,
plus the formula plus half a line, plus spacing: 50.7 px where one row needs
28.1 px (test numbers, default style).

The widths were available all along: the math cache stores the painted size of
each rendered formula, not just its height.

## Fix

- Backend: `cached_inline_math_size` returns the full painted size;
  `cached_inline_math_height` now wraps it.
- `single_row_math_cell_height`: when every formula in the cell is rendered,
  the cell has no breaks, display math or images, and text width plus formula
  widths plus item spacing fit the column (minus the same 8 px inset
  `wrapped_text_height` uses), the row is as tall as its tallest piece:
  `max(tallest formula + ½ line, line + item spacing)`. Text is measured in
  the wider of the body and monospace fonts, so the check errs toward "does
  not fit".
- Otherwise the old cumulative reservation stays, including the conservative
  two-line guess while a formula is still rendering.

## Tests

- `a_cell_mixing_text_and_one_rendered_formula_is_one_row_tall`: red without
  the fix (50.66 vs 28.07), green with it.
- `a_mixed_cell_too_wide_for_its_column_keeps_the_cumulative_reservation`
- `an_unrendered_formula_keeps_the_conservative_reservation`
- Test seam `seed_inline_math_size_for_test` marks a formula as rendered
  without running Typst.

## Not changed

Cells with several formulas that wrap still reserve cumulatively, which can
over-reserve. Simulating the wrap exactly needs per-piece widths for text
runs, which the cell measurement does not track.
