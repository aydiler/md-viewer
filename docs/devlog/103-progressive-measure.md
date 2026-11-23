# Large documents are measured over several frames

**Status:** ✅ Complete
**Branch:** `perf/progressive-layout`
**Date:** 2026-11-20

## Summary

`show_scrollable` measured the whole document in the first frame (the bootstrap
pass) before it could paint anything. With #259 and #260 merged, prose is what
remains: 3.7 s to the first content frame for 5 MB, 3.4 s for the 6 MB GitLab
changelog. callgrind on 1 MB of prose shows the time is real text layout, one
layout per paragraph, with nothing to cut.

With `CommonMarkViewer::progressive_measure(true)` (md-viewer turns it on), the
first frame measures only until a time budget has passed and two screens are
covered. Later frames paint the viewport slice as before and measure the rest
in budgeted chunks.

## Features

- [x] One event loop for the bootstrap and the progressive passes
- [x] Frontier state on `ScrollableCache`, reset with the split points
- [x] Frame-1 budget and estimated extent
- [x] Background continuation in an invisible child
- [x] Synchronous catch-up when the viewport passes the frontier
- [x] A pending offset past the estimate and `force_full_render` fall back to the full bootstrap
- [x] md-viewer opts in
- [x] Tests: equivalence, first frame, frontier cap, deep scroll, invalidation
- [x] Measurements

## Measurements

Release builds with the startup probe, alternating with main run by run on
Xvfb (5 runs each, one detached unit). Time from launch to the first frame with
content, in ms:

| document | main: fastest / median | branch: fastest / median |
|---|---|---|
| prose 5 MB | 3 023 / 3 133 | **292 / 293** |
| GitLab changelog 6 MB | 2 169 / 2 208 | **532 / 533** |
| tables 1 MB | 1 908 / 1 919 | 213 / 215 |
| mixed scripts 1.6 MB | 1 384 / 1 392 | 220 / 222 |
| code-heavy 2.7 MB | 1 332 / 1 350 | 166 / 167 |
| headings | 596 / 605 | 224 / 226 |
| prose 1 MB | 497 / 504 | 164 / 165 |
| 30 restored tabs incl. 3 large | 571 / 573 | 241 / 242 |
| README | 178 / 180 | 146 / 148 |

End to end on Xvfb with prose 5 MB, while measurement was still running:
- fast wheel scrolling (8 × 60 clicks) painted content in every capture and
  landed on "Section 512" with an intact layout;
- self-test offsets walked to 400 000 px and injected at 150 000 px held;
- an outline click on "Section 232" put it at the top of the pane within 0.4 s;
- resizes and a live reload that prepends a heading painted correctly;
- no panics in any run.

`scripts/visual-regression.sh` and `scripts/scroll-regression.sh` pass on the
release build (bottom reached at frame 63 of 150, as on main).

## Key Discoveries

- **A resumed pass must start in the layout state the bootstrap had there,
  not just at the same y.** The first version placed the measuring child at
  the frontier's y with the slice pass's setup, and the equivalence test put
  split points up to 15 px off, growing with every chunk. Three things differ
  between a fresh child and the bootstrap's ui at a block boundary, and all
  three had to be reproduced:
  - **The cursor row.** A block ends with a newline label, which leaves the
    cursor on an empty row. The bootstrap's next block lays its own leading
    newline on that row. The frontier records the row's top (`row_top`), and
    the child starts there.
  - **The content bottom.** egui starts a wrapped row below `min_rect`
    (`new_top = min_rect.bottom() + spacing`), and an empty child's
    `min_rect` is above where the bootstrap's was. The frontier records it
    (`min_rect_bottom`), and the child expands to it.
  - **The first-block flag.** The bootstrap clears
    `should_not_start_newline_forced` after event 0. A resumed run is never at
    the document top, so the measurer clears it before its first event. Without
    this, a heading at the frontier is laid out as if it opened the document.
- **Recorded y values are row bottoms.** `next_widget_position` is the centre
  of a zero-size frame, and with `Align::BOTTOM` that is the bottom of the
  cursor row. So `frontier.y` and `row_top` differ by the row height; mapping
  the resumed content origin to `frontier.y` shifts everything by one line
  (mutant M8).
- **The bootstrap's extent has no trailing padding.** `show()` ends with
  `add_space(2 lines)`, which is horizontal in this wrapping layout (LESSONS,
  #236). The extent a full bootstrap reports is the content bottom, and the
  progressive pass must report the same thing, not measured + padding.
- **Painted shapes cannot show how far a pass ran.** Labels outside the clip
  are laid out but not painted, so "deepest painted text" stayed at the
  viewport even with the frontier cap removed. Recorded heading positions are
  set when a heading is laid out, painted or not, and show it.

## Architecture

- `ScrollableCache::measure_frontier: Option<MeasureFrontier { event_index, y,
  src_end, row_top, min_rect_bottom }>`. `Some` means partly measured, and
  `page_size` then holds an estimate extrapolated by source length
  (`estimated_extent`). Reset with `split_points` on every invalidation.
- `CommonMarkViewerInternal::run_events`: the bootstrap's event loop,
  extracted. It takes the stream position and content origin it starts at and
  an optional `MeasureStop { min_y, budget }`, and returns the frontier when it
  stops. `show()` calls it from 0 with no stop rule. The cached events are now
  iterated by reference rather than copied with `to_vec`.
- `show_measuring`: `show()` with a stop rule. The first progressive frame uses
  it with `min_y = 2 × viewport` and an 8 ms budget.
- `measure_from_frontier`: lays out from the frontier in an invisible
  `new_child` (allocates nothing in the parent; distinct id salt) with a fresh
  renderer, at the screen position the bootstrap would give that row.
- `show_scrollable`, while the frontier is `Some`: catch up to
  `viewport.max + one screen` before choosing the slice, cap the slice at the
  frontier, then measure 4 ms more and request a repaint. A pending offset
  beyond the estimated extent, and `force_full_render`, take the full
  bootstrap.
- Options: `CommonMarkOptions::progressive_measure` (builder
  `progressive_measure`, off by default) and the doc-hidden
  `progressive_measure_blocks` test hook.

## Testing

`egui_commonmark/tests/progressive_measure.rs`, with a deterministic budget of
2 blocks per chunk:

- `progressive_measurement_ends_with_the_one_shot_geometry`: split points
  (indices equal, y within 1 px), extent and every heading position equal a
  one-shot bootstrap, at widths 600 and 380.
- `first_frame_measures_only_the_top_and_paints_it`.
- `no_frame_lays_out_past_the_measured_part` and
  `a_tall_block_at_the_frontier_does_not_lay_out_the_rest_of_the_document`: no
  heading past the frontier has a recorded position.
- `scrolling_into_the_estimated_region_paints_content`.
- `force_full_render_completes_measurement_at_once`.
- `invalidation_mid_measurement_restarts_cleanly`: a resize and a content change
  mid-way still end with the one-shot geometry.

Mutants, each caught:
- resuming at event 0;
- no first-block flag clear;
- `slice_start_y` left at 0 (heading positions);
- `min_rect` not expanded;
- the content origin mapped to `frontier.y` instead of `row_top`;
- no catch-up;
- no cap at the frontier (caught only after the observable changed, above).

Restoring the cursor row's *height* turned out to change no measurement once
the child reaches the content bottom (that mutant survived), so it was dropped
from the frontier rather than kept untested.

## Future Improvements

- The slice pass shares the fresh-renderer start that this work found wrong
  for headings: a slice that begins at a heading lays it out a few pixels off
  the bootstrap's position (the "settles a few pixels" note in LESSONS). It
  could start from the same recorded state.
- A single huge table, list or blockquote has no safe split inside it and is
  still measured in one piece.
- ~~Clicking an outline heading that is not measured yet still measures the
  whole document (`force_full_render`); it could measure up to the heading.~~
  Done in devlog 115 (`measure_to_source`).
- The changelog still takes about 0.5 s: the first frame parses the 6 MB
  source twice (`ScrollableCache::events` and `CommonMarkCache`'s own copy)
  and hashes it. One shared parse would remove that.
