# 131: Tables stop re-measuring every row on every frame

**Status:** ✅ Complete
**Branch:** `perf/table-measure-cache` (#308)
**Date:** 2026-11-28

## Summary

While a large markdown table was on screen, every frame measured every row of
it: scrolling inside a 200 KB table cost 49 ms per frame, inside a 1 MB table
265 ms (prose of the same size: 0.4 ms). Painting was already virtualized
(`egui_extras` `heterogeneous_rows`); the measurement around it was not. Now a
table keeps its measurements across frames and a settled table measures no
cell while it scrolls.

## Measured before (main 8271e6f, release build, Xvfb, 3 runs, idle machine)

A probe scrolls 120 px per frame through the block once the document is
measured (`scratchpad/bigblock/`).

| document | first frame | each frame while scrolling inside |
|---|---|---|
| prose 1 MB (control) | 40 ms | 0.4 ms |
| one table, 200 KB | 0.22–0.61 s | 49 ms |
| one table, 1 MB | 0.95 s | 265 ms |

Callgrind over 30 scrolling frames of the 200 KB table:
- `table_cell_height` for every cell, twice per frame (`reserved_row_heights`
  and the body `heights`): 52 % of all instructions, most of it in
  `replay_wrap` (48 %) and the `unicode_bidi` class lookups under it (17 %).
- `markdown_table_digest`, which Debug-formats every event to key the column
  optimizer: 12 %.
- `natural_text_width` and `inline_code_floor` for every cell
  (`desired_widths`, `minimum_widths`).

## Measured after

The machine was busy with another session's jobs (load average 14–29 on 16
cores), so main and the branch ran alternately on the same fixtures, two rounds
each:

| document | main, per frame | branch, per frame | main, first frame | branch, first frame |
|---|---|---|---|---|
| prose 1 MB (control) | 0.9 ms | 0.9 ms | 57–62 ms | 61–62 ms |
| one table, 200 KB | 92 ms | 7 ms | 400–415 ms | 309 ms |
| one table, 1 MB | 469–481 ms | 37 ms | 1.29–1.82 s | 1.26–1.33 s |

About 13 times faster per frame under the same load; the first frame, which
still measures the table once, is no slower.

## Changes

- **`TableMeasurements`** (`parsers/pulldown.rs`), kept in egui temp data
  under `table_id.with("_measurements")`:
  - `content_id`: the cached parse's content hash
    (`CommonMarkCache::cached_content_hash`, new) and the table id. Any edit to
    the document measures the table again.
  - `style_key` (`table_style_key`): line height, body and monospace font, zoom,
    `layout_revision` (an image or formula that finished loading) and math
    scale. `table_layout_key` now hashes the same inputs through
    `hash_table_style`.
  - `digest`: `markdown_table_digest`, computed once per `content_id`. It keys
    the column optimizer, so an edit elsewhere in the document recomputes it to
    the same value and the optimizer's widths, and widths the user dragged, are
    kept.
  - minimum and desired column widths;
  - the row heights for the last reserved widths and the last body widths
    (`cached_row_heights`, keyed by `row_heights_key`: the widths' bits and
    the measuring ui's item spacing).
- The measuring pass and the painted pass share the entry: everything in it is
  a function of content, style and widths, unlike the stored column widths
  (devlog 122).
- Without a cached parse (events that did not come through
  `parsed_events`), a table measures every frame as before.
- Test hook: `table_cells_measured_for_test()`, a thread-local count of
  `table_cell_height` calls.

## Key Discoveries

- **A stale row height does not change the document's extent.** The cell's
  text is laid out where it really wraps and the extent follows it; the
  stale height shows as a row painted over the next one. The rendered tests
  therefore check that each painted row starts below the text of the row
  above it.
- **After an edit the first column's width differs by one ulp** (68.52919 vs
  68.529175) between the bootstrap's measuring pass and the painted pass,
  which re-measures the heights once. It also hid a mutation that ignored the
  content id from the rendered test; a unit test on the content id catches it.
- **Stored column widths survive an edit and a font change** (with or without
  the cache), so the edited table can differ from a fresh layout of the same
  text by a few pixels: the optimizer would pick other widths. The tests
  compare rows with each other there, not with a fresh layout.
- **Mutation runs need the whole test set.** A first run filtered tests with
  `-- table`, which also filtered out integration tests whose names lack the
  word, and reported two mutations as missed that the full run catches.

## Tests

- `tests/table_measure_cache.rs`:
  - `a_settled_table_measures_no_cell_while_it_scrolls`: 0 cells over ten
    scrolled frames of a 300-row table (12 040 without the cache);
  - `a_longer_cell_is_measured_again`, `a_larger_font_measures_the_rows_again`,
    `a_narrower_pane_rewraps_the_rows`: rows stay stacked, the table grows,
    and the narrower pane matches a fresh layout.
- Unit tests: `an_edit_after_a_table_keeps_its_column_layout_key`,
  `table_style_key_tracks_the_text_style`,
  `cached_row_heights_are_kept_per_column_widths`.

Mutations, each caught: content id ignored, style key without the text style,
heights not keyed by widths, cache bypassed, digest keyed by the content id.

Xvfb, main and branch side by side on a 400-row table: scrolling, dragging
column A narrower, a live edit that grows a visible row from 2 to 10 lines,
and an edit below the table that keeps the dragged width: the document pane
is pixel-identical (0 px) at every step.

## Future Improvements

- **The 1 MB table still costs 37 ms per frame under load.** Callgrind of the
  branch: `parse_table` (copying every event of the table into rows) is 30 %
  of all instructions, about three quarters of the renderer's share; the rest
  of the main thread is the per-frame copy of the slice's events
  (`events_range.to_vec()`). Caching the parsed rows per `content_id`, and
  skipping a cached table's events without copying them, would remove most of
  it.
- The first frame still measures the whole table (0.3 s for 200 KB, 1.3 s for
  1 MB here).
- HTML tables have their own path (`html_table`) and were not measured.
- Single huge lists (54 ms per frame at 1 MB) and blockquotes (20 ms) lay out
  the whole block every frame (measured with this devlog's probe).
