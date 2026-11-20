# Off-screen code blocks are highlighted only once they are visible

**Status:** ✅ Complete
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

After the change, release builds with the startup probe, alternating with main
run by run on Xvfb (7 runs each, load average around 100 from unrelated jobs).
Time from launch to the first frame with content, in ms:

| document | main: fastest / median | branch: fastest / median |
|---|---|---|
| code_heavy (2.7 MB, 3,000 code blocks) | 14 738 / 19 406 | **2 539 / 3 050** |
| README | 652 / 775 | 388 / 675 |
| prose 1 MB | 1 111 / 1 259 | 1 026 / 1 262 |
| mixed scripts 1.6 MB | 3 338 / 3 962 | 3 225 / 4 336 |
| 30 restored tabs incl. 3 large | 1 557 / 2 672 | 1 461 / 2 715 |

The first frame itself on code_heavy went from 18 953 to 2 776 ms (median),
and on the README from 429 to 269 ms. The other documents are unchanged within
noise.

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
- **A mutant that changes nothing in the test style is not a test of
  anything.** Making the plain job use the body font turned nothing red,
  because egui's test style sets both fonts to 13 pt and the galleys came out
  identical. The meaningful mutant is a plain job that drops the code line
  height: the unit test caught it, and the integration extent test did only
  after its viewer was given a code line height, as md-viewer always sets one.
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
