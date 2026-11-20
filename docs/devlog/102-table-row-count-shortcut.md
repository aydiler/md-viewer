# Table cells are measured by replaying line breaking, not by laying them out

**Status:** ✅ Complete
**Branch:** `perf/table-row-count-shortcut`
**Date:** 2026-11-20

## Summary

The table column-width optimizer (#157, #182) scores candidate column widths by
measuring every cell's height at each candidate width. Each measurement laid
the cell text out in full at that width, twice (`wrapped_text_height` and
`cell_visual_lines`), only to count its rows. On a document of about 3,000
ordinary tables that was more than half of all instructions before the first
frame, and the first frame took 16 s.

Shaping does not depend on the wrap width; only line breaking does. The
renderer now takes the glyphs of the unwrapped galley, which `natural_text_width`
already lays out for the frame, and replays epaint's line breaking over them to
get the row count and the last row's width. No layout happens per candidate
width.

## Measurements

Profile first (callgrind, release build with line tables, the first 30 tables
of the startup `tables` fixture, Xvfb), share of all instructions up to the
first content frame:

| build | `wrapped_text_height` | `cell_visual_lines` | program total |
|---|---|---|---|
| main | 52.7 % | 0.1 % | 11.31 G |
| one-row shortcut (first attempt) | 44.0 % | 0.1 % | 11.08 G |
| replay in `wrapped_text_height` only | 4.4 % | **52.4 %** | 11.32 G |
| replay in both (this branch) | | | see below |

Release builds with the startup probe, alternating with main run by run on
Xvfb (5 runs each). Time from launch to the first frame with content, in ms:

| document | main: fastest / median | branch: fastest / median |
|---|---|---|
| tables (1 MB, ~3,000 tables) | 15 322 / 15 967 | **3 399 / 3 493** |
| headings | 959 / 1 006 | 959 / 974 |
| mixed scripts 1.6 MB | 2 419 / 2 493 | 2 455 / 2 544 |
| README | 326 / 366 | 324 / 355 |
| 30 restored tabs incl. 3 large | 829 / 959 | 892 / 952 |

## Key Discoveries

- **A shortcut for the easy case did nothing.** The first attempt answered only
  cells that fit on one row, from the unwrapped width. At the widths the
  optimizer tries, 70 % of measurements (35,750 of 51,408 on the 30-table
  slice) wrap, so the total moved by 2 % and the benchmark not at all.
- **Removing one layout moved the cost to a second one.** `cell_visual_lines`
  lays the same text out at the same width, and had been a galley-cache hit
  only because `wrapped_text_height` laid it out just before. With the first
  one replayed, the second missed the cache and the profile barely changed
  (11.31 G → 11.32 G). Both had to go.
- **The replay is exact only without a leading space.** Glyph x positions are
  rounded from a fractional cursor, so a label indented by earlier content is
  not the unwrapped layout shifted. `cell_visual_lines` replays a label only at
  the start of a row and lays the rest out; a mutant that replays indented
  labels too is caught by `cell_line_count_is_the_same_with_and_without_the_replay`.
- **The renderer's tests build against crates.io epaint, the app against the
  vendored one.** The two `line_break`, `rows_from_paragraphs` and
  `RowBreakCandidates` differ only in two bidi fields per row, and the tests
  were run against both (`--config 'patch.crates-io.epaint.path=…'`).

## Architecture

- `replay_wrap(ui, text, font_id, wrap_width) -> Option<WrapReplay { rows,
  last_row_width }>` in `egui_commonmark/src/parsers/pulldown.rs`, with
  `BreakCandidates` (epaint's `RowBreakCandidates`) and copies of epaint's
  private `is_cjk` and `is_cjk_break_allowed`.
- `None`, and a full layout, for text with a newline (several paragraphs) and
  for text with strong right-to-left characters (bidi reorders glyphs).
- `wrapped_text_height` and `cell_visual_lines` (via `cell_visual_lines_with`)
  use it.

## Testing

- `replay_wrap_matches_a_full_layout`: 18 texts (empty, single words,
  punctuation, dashes, URLs, non-breaking spaces, Chinese, Japanese with
  forbidden line starts, emoji, tabs, combining marks), each swept in quarter
  points from 1 pt to past its natural width, in the body and the monospace
  font: rows and last-row width equal a full layout's.
- `cell_line_count_is_the_same_with_and_without_the_replay`: 12 parsed table
  cells mixing text, inline code, emphasis, CJK and task markers, swept in half
  points from 20 to 420.
- `replay_wrap_answers_for_single_left_to_right_paragraphs`: the replay does
  answer for those texts and falls back for newlines, Hebrew and Arabic.
- `copied_cjk_break_rules_match_epaint`: the copied table is read back out of
  epaint's source and compared.
- **Mutants, each caught:** no +0.5 wrap allowance; "any character" preferred
  over a word boundary; no dash candidate; no CJK candidate (the last two only
  after adding texts for them); `forget_before` a no-op; last-row width
  without the row start; replaying indented labels.

## Future Improvements

- Labels after a leading space are still laid out; most cells have one label.
- Right-to-left cells could be replayed in logical order if bidi reordering
  were applied per row afterwards.
