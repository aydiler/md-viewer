# 131: Tables stop re-measuring every row on every frame

**Status:** 🚧 In Progress
**Branch:** `perf/table-measure-cache`
**Date:** 2026-11-28

## Summary

While a large markdown table is on screen, every frame measured every row of
it: scrolling inside a 200 KB table cost 49 ms per frame, inside a 1 MB table
265 ms (prose of the same size: 0.4 ms). Painting was already virtualized
(`egui_extras` `heterogeneous_rows`); the measurement around it was not.

## Measured before (main 8271e6f, release build, Xvfb, 3 runs)

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

## Changes

- [ ] Per-table `TableMeasurements` cache in egui temp data
- [ ] Tests and mutations
- [ ] Measured after
