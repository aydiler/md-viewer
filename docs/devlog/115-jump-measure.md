# 115: Jumping to a heading that is not measured yet

**Branch:** `perf/jump-measure`
**Date:** 2026-11-23
**Status:** Complete

## Summary

With progressive measurement (devlog 103), a large document is laid out over
many frames. Clicking an outline heading the measurement had not reached, or
following a `#fragment` link to one, set `force_full_render`. That cleared the
frontier and laid out the whole document in one frame, so the window froze.

Measured before the change (release build, Xvfb, 5 runs, the frame after the
click to the last heading):

| document | freeze (median) | normal frames |
|---|---|---|
| prose 1 MB | 1.2 s | 5 ms |
| prose 5 MB | 3.6 s | 6 ms |
| GitLab changelog 6 MB | 2.5 s | 23 ms |

Now the renderer measures toward the heading over several frames and keeps
painting the current view meanwhile; the app jumps once the heading's position
is recorded.

## Changes

- Renderer: `CommonMarkViewer::measure_to_source(Option<usize>)`. While the
  measured part ends before that source offset, each frame measures with
  `TARGET_MEASURE_MS` (32 ms) instead of `CONTINUE_MEASURE_MS` (4 ms)
  (`continue_budget`, `target_ahead`). It never lays out the rest in one frame.
- Renderer: `CommonMarkCache::measured_source_end(source_id)`, the source offset
  where the measured part ends, `None` once fully measured.
- App: `Tab::pending_header_jump: Option<PendingHeaderJump>` replaces
  `pending_header_click_key`. The outline click and `apply_fragment_navigation`
  arm it when the heading has no recorded position, and no longer jump to a
  line-ratio estimate first.
- App: after rendering, `resolve_header_jump` scrolls to the recorded position,
  waits while the measured part has not reached the heading, or falls back to the
  line-ratio estimate if the document is measured past the heading without a
  position. `force_full_render` now comes from the search correction only.

## Measurements after

Same probe as before: release build with a local probe that calls
`apply_fragment_navigation` at frame 30 and prints each frame's `ui()` time.
Xvfb, without the virtual-display frame sleep. Three runs each.

| target | worst frame after the click | frames until the jump | time until the jump |
|---|---|---|---|
| prose 1 MB, last heading | 33–135 ms | 8–9 | 0.26–0.38 s |
| prose 5 MB, last heading | 40–244 ms | 52–53 | 1.8–1.9 s |
| prose 5 MB, heading in the middle | 34–194 ms | 25 | 0.85–1.0 s |
| changelog 6 MB, last heading | 50–108 ms | 34–35 | 1.3–1.4 s |

Most frames take the 32 ms budget plus the paint. In about half the runs, one
of the first frames after the click takes 60–240 ms. That frame is CPU-bound on
the UI thread (its thread CPU time equals its wall time), and it is not page
faults: the faulting frames and the slow frames are different frames. Its cause
is not known; it is still at least 15 times shorter than the freeze it replaces.

Mid-document headings land at the top of the pane, at normal frame pacing too
(screenshot).

## Tests

- `progressive_measure.rs`:
  - `a_target_ahead_of_the_frontier_measures_further_per_frame_without_a_full_layout`
    compares the frontier after the same two frames with and without a target.
    The target must measure further, and must not finish the document.
  - `measuring_toward_a_target_records_it_at_the_one_shot_position` checks that
    the target heading's recorded y equals a one-shot layout's (±1 px) and that
    reaching it takes fewer frames than plain measurement.
- The block-count test hook scales by the same ratio as the time budget, which
  is what makes the target observable in a deterministic test.
- Unit tests: `a_navigation_target_raises_the_budget_only_while_it_lies_ahead`
  (renderer), `a_pending_heading_jump_waits_scrolls_or_falls_back` and
  `a_jump_to_an_unmeasured_heading_measures_toward_it_instead_of_guessing` (app).

Mutants, each red:
- `measure_to_source` ignored at the call site: the per-frame test fails;
- the fallback removed (no position, fully measured → wait): the resolver test fails;
- jumping to the line-ratio estimate first again: the app navigation test fails.

## Found on the way, not fixed here

**A jump to the last heading of a very large document does not show it.** On
prose 5 MB, after the jump the offset eases upward and settles with the
scrollbar at the bottom, but the last heading stays below the pane. Main does
the same (after its 4.8 s freeze), so it predates this change.

Diagnosed afterwards (no code changed):

- **The end is reachable.** Once measurement completes, End and the scrollbar
  show the last line, and the last heading's recorded y (2 162 965) equals where
  it is painted.
- **Only a heading jump is off, and only far down:** not measurably at 0.43 M px,
  about 15 px at 1.08 M, about 458 px at 2.16 M.
- **Cause: f32 rounding in absolute coordinates.** egui lays text out in f32
  screen coordinates, whose spacing is 0.125 px between 1 M and 2 M and 0.25 px
  above 2^21. Row heights accumulate that rounding. Passes that chunk the
  document differently (4 ms and 32 ms measuring chunks, a bootstrap at an
  offset) therefore record different positions, and the jump reads one that
  disagrees with the final layout.
- **The extreme case shows the mechanism:** a pending offset of 1e9 made the
  full bootstrap measure the 2.16 M px document as 1.63 M px (64 px spacing out
  there). The app never requests such offsets.
- **Proposed fix:** lay each measuring chunk out near y = 0 in a local frame and
  add the frontier's offset once, so no layout runs at million-pixel magnitudes.
  It touches `measure_from_frontier` and the bootstrap, so the resume
  invariants from devlog 103 need re-checking.

## Future improvements

- The search jump still uses `force_full_render` for a match the measurement has
  not reached. It records `active_search_y` while painting, so it needs the same
  treatment with a source offset and a recorded position.
- There is no visible feedback while a far heading is being measured (up to
  about 2 s on 5 MB).
