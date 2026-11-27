# 130: Find-bar jumps without laying out the whole document

**Status:** ✅ Complete
**Branch:** `perf/search-jump-measure`
**Date:** 2026-11-27

## Summary

Every find-bar step laid out the whole document in one frame: opening the bar
or typing a character (through `maybe_rebuild_search`), Enter, Shift+Enter and a
tab switch all call `scroll_to_active_match`, which set a line-ratio offset and
granted the next frame `force_full_render`. `show_scrollable` then dropped the
measured state and measured everything again, even when the document was
already fully measured. Devlog 115 removed the same full layout from heading
jumps and listed the search jump as its follow-up.

Now a jump measures toward its match the way a heading jump does, and a match
in the part already measured is reached by scrolling to its block first. No
step lays out the whole document.

## Measurements

A probe (local patch, not committed) opens the find bar at frame 30 with a token
that occurs at about 50 % and 95 % of the document, jumps to the next match at
frame 90, and again at frame 150, which wraps back to the first match in the
already-measured part. It prints each frame's `ui()` time, the scroll offset and
the match's recorded position. Release builds, Xvfb, without the virtual-display
frame sleep, 3 runs each.

Before (main):

| document | open the bar | jump ahead | jump back (measured part) |
|---|---|---|---|
| prose 1 MB | 355–359 ms | 376–410 ms | 354–382 ms |
| prose 5 MB | 1872–1887 ms | 1821–1880 ms | 1852–1905 ms |

Each of these is one frame. With the find bar open, every typed character cost
the same.

After (worst frame; frames until the match is in view):

| document | open the bar | jump ahead | jump back (measured part) |
|---|---|---|---|
| prose 1 MB | 35–36 ms; 2 frames | 1–2 ms; 1 frame | 1 ms; 1 frame |
| prose 5 MB | 43–44 ms; 23 frames (0.8 s of `ui()` time) | 34–35 ms; 20–21 frames (0.7 s) | 2 ms; 1 frame |

The match lands where it did before: 35 % down the pane (37 % for the match
near the end of the 5 MB document, where the offset reaches the bottom). On
1 MB the progressive measurement has finished by the first jump, so only
opening the bar measures toward the match.

## Changes

- **Renderer: `CommonMarkCache::measured_block_top(source_id, offset)`.** The
  content y of the top of the measured top-level block holding a source offset.
  A split point is `(index of the event that starts the next block, position
  before the closing event, position after it)`, so the block holding the
  offset starts at the `end.y` of the last split point whose next event starts
  at or before the offset. `None` past the frontier or without a layout.
- **Renderer: a layout change forgets the active match's position**
  (`forget_active_search_y`, next to `clear_header_positions`). The old full
  render re-recorded it on every jump; without it, a jump to an unchanged match
  after a zoom or width change would have used the old layout's position.
- **App: `Tab::pending_search_jump: Option<PendingSearchJump>`** replaces
  `correct_active_search_pending`, and `take_search_correction_request` is gone.
  `scroll_to_active_match` arms the jump through `Tab::start_search_jump` and no
  longer sets a line-ratio offset. The render call passes the nearer of the
  heading and search targets as `measure_to_source` and no longer sets
  `force_full_render`.
- **App: `resolve_search_jump`** runs after each frame:
  - *Reveal*: the position is recorded; scroll it 35 % down the pane if it is
    outside the view (the 20 px margin test from before), and finish.
  - *Wait*: measurement has not reached the match; it keeps measuring at the
    32 ms target budget.
  - *Locate*: the match's block is measured but the match was not laid out this
    frame; scroll to the block and keep the jump for one more frame.
  - *Stop*: the block was brought into view and the match still has no
    position (nothing to draw, e.g. inside a code fence); leave the view there.
  - *Estimate*: no measured layout; the line-ratio offset.

  The jump is consumed by its last scroll, so it keeps the one-shot rule from
  #19 (devlog 031): nothing corrects the view after the user scrolls away.

## Key Discoveries

- **Measuring passes already record the active match.** The highlight code
  records the match wherever its text is laid out, through
  `content_relative_y(screen_y, render_origin_y, slice_start_y)`, and the
  measuring passes set both origins. So `measure_to_source` toward the match
  yields its exact position without new renderer code; the new test checks it
  against a one-shot layout (±1 px).
- **The old path cost a full layout even for a measured match.**
  `force_full_render` clears `page_size` and the frontier, so the bootstrap
  re-measures everything; a fully measured document paid it on every Enter.
- **Repeated-text fixtures lie.** A 5 MB fixture built by repeating a 1 MB text
  five times measured 0.5 s per full layout: egui's galley cache shapes a
  repeated paragraph once. The unique 5 MB fixture measured 1.9 s (LESSONS).

## Tests

- Renderer (`tests/progressive_measure.rs`):
  - `measuring_toward_an_active_match_records_it_at_the_one_shot_position`:
    within 1 px of a one-shot layout, while the document is still partly
    measured.
  - `measured_block_top_points_at_the_block_holding_an_offset`: between the
    heading above a paragraph and a match inside it; `Some(0.0)` for the first
    block; `None` past the frontier and for a viewer never shown.
  - `a_layout_change_forgets_the_active_match_position`.
- App: `a_pending_search_jump_reveals_waits_locates_or_stops` (the resolver)
  and `a_search_jump_measures_toward_its_match_instead_of_guessing` (arming sets
  no offset).

Mutations, each caught by its test: the next block's top, the previous block's
top, no frontier check, no forget on a layout change, a measuring pass recording
50 px off, no Locate step, no Wait step, a line-ratio offset when arming.

## Future Improvements

- No visible feedback while a far match is being measured (0.7–0.8 s on 5 MB),
  as for headings (devlog 115).
- `force_full_render` has no caller in md-viewer any more; it stays public for
  other users of the renderer.
