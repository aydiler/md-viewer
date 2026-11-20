# Off-screen code blocks are highlighted only once they are visible

**Status:** 🚧 In progress
**Branch:** `perf/offscreen-code-highlight`
**Date:** 2026-11-20

## Summary

The scrollable renderer lays out the whole document in its first frame (the
bootstrap pass, which records `split_points` and `page_size`). Every code block
in it ran syntect, whether or not it could be seen. On a code-heavy document
that pass took about 13 s for 2.7 MB.

A code block that lies wholly outside the clip rect now gets a plain layout
job. It is highlighted, and the highlighted job cached, the first time it is
painted where it shows.

## Measurements

Profile first (callgrind, release build with line tables, first 10 % of each
startup fixture, on Xvfb). Share of all instructions up to the first content
frame:

| fixture | hotspot | share |
|---|---|---|
| code_heavy (300 blocks) | `CodeBlock::end` → syntect `highlight_line` | 57 % (19 % of it one-time regex compilation) |
| code_heavy | laying out the highlighted galleys | 17 % |
| tables (30 tables) | column-width optimizer (`optimize_fitted_widths` → `table_cell_height`) | 55 % |

The table cost is a separate fix.

## Key Discoveries

- **Highlighting does not change geometry.** Both the syntect and the plain job
  use `TextFormat::simple` with the monospace font and the same line height;
  only colors differ. `plain_job_has_the_highlighted_geometry` pins that for
  several languages and line heights.
- **"Off screen" is exact below the clip, estimated above it.** A block's
  height is not known before layout, so a block above the clip counts as off
  screen only if twice its line count times the line height, plus room for the
  frame, still ends above the clip. An underestimate would paint a block that
  reaches into view without colors for one frame.
- **samply needs `perf_event_paranoid` ≤ 1.** It is 2 here, a system setting,
  so callgrind on 10 % slices was used instead.

## Architecture

- `code_block_is_offscreen(ui, text, code_line_height)` in
  `egui_commonmark_backend/src/misc.rs`.
- `CodeBlock::end`: on a cache miss, an off-screen block gets
  `plain_highlighting` and nothing is cached; a visible one runs syntect and
  caches the job as before.
- `CommonMarkCache::highlighted_code_block_count` (doc-hidden) for tests.

## Testing

- `plain_job_has_the_highlighted_geometry` (backend unit test).
- `tests/offscreen_code_highlight.rs`:
  - the first frame highlights only the visible blocks;
  - scrolling to a block highlights it;
  - the document extent matches a run in which every block is visible and
    highlighted.

## Future Improvements

- The plain galley is still shaped in full. Its height is line count times
  line height plus the frame, so even shaping could be skipped off screen.
