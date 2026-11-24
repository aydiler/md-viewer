# Fix: jumping to a heading far down a large document

**Status:** ✅ Complete
**Branch:** `fix/far-heading-precision`
**Date:** 2026-11-25
**Lines Changed:** renderer `parsers/pulldown.rs` (`measure_document`, `measure_from_frontier`, `run_events`, `show_scrollable`, `deepest_offset`), backend `pulldown.rs` (`MeasureFrontier`), two new test files

## Summary

Devlog 115 found that after a jump to the last heading of a 5 MB document the
view eased upward and the heading stayed below the pane, and blamed f32
rounding in absolute coordinates. There were two separate problems, and the
visible one was not the rounding:

1. **The jump overscrolled, and the spring-back velocity outlived the clamp.**
   md-viewer asks for the heading's offset, which near the end of a document is
   past the deepest position (2,162,948 against a maximum of 2,162,526 here).
   egui 0.37 treats that as overscroll and gives the scroll area a spring-back
   velocity (−868 pt/s). The renderer clamped the offset on the next frame,
   but the velocity stayed, and the view slid up about 580 pt under friction.
   That is the "eases upward and settles" from devlog 115.
2. **Measuring passes laid content out millions of points from the screen
   origin.** The bootstrap laid the document out where the scroll area put it,
   and a progressive chunk sat at `content_top + frontier.row_top`. f32 holds
   egui's 1/32 pt grid exactly only near the origin, so every row rounded and
   the error added up. With fractional row heights (125 % scaling, odd text
   sizes) a heading 2.5 M pt down was 176 pt off in the renderer test. With
   md-viewer's own text sizes the error did not show in the runs below.

## Changes

- **Requested offsets never go past the deepest position.** An explicit
  `pending_scroll_offset` is clamped to `deepest_offset` (measured height or
  extent floor, minus the viewport) before egui sees it. On a bootstrap frame
  the extent is not known when the offset would be applied, so the request
  waits one frame (`pending_clamp`), clamped at the end of the bootstrap
  frame. Carried overshoot clamps and stored offsets keep the old rule
  (only an offset past the measured height is pulled back): pinning those to
  the measured bottom made the extent floor learn the trailing padding on
  every scroll to the end (`deep_scroll_keeps_content_extent_and_paints_visible_text`
  caught it).
- **All measuring happens in invisible children near the screen origin.**
  `measure_document` measures the bootstrap (progressively or in full) in an
  invisible child, then the same frame paints the viewport slice like any
  other frame; the bootstrap no longer has its own visible pass.
  `measure_from_frontier` lays out at most `MEASURE_CHUNK` (32,768 pt) per
  child and starts the next child afresh from the frontier it reached.
- **Where the children sit:** `MEASURE_ORIGIN_Y` = 32,768, below the window,
  plus the frontier's content y modulo the chunk (`measure_origin_for`), so a
  resumed chunk has the same sub-pixel phase as one pass from the top would.
- **The frontier's anchor is f64** (`MeasureFrontier::row_top`,
  `min_rect_bottom`): every chunk adds its extent to it, and recorded
  positions round once instead of chunk after chunk.
- **The viewport slice has an explicit scope id** (`viewport_slice`).
- **A measuring pass does not store a table's widths or bound**
  (`store_table_column_widths`, `store_table_bound` return early in an
  invisible ui), so the painted table starts from the state a first paint
  has.

## Key Discoveries

### An offset past egui's bounds leaves a velocity behind

`ScrollArea::vertical_scroll_offset` sets the offset but not the velocity.
An offset past the bounds starts the overscroll spring; setting the offset
back inside on the next frame leaves the spring's velocity, which then moves
a scroll area that is no longer overscrolled. One run that injected the same
offset without input (`MDV_SELFTEST`) did not drift; why was not
investigated, since the renderer test reproduces the drift headless and the
fix does not depend on it.

### egui hit-tests invisible widgets, and `new_child` shifts later ids

Two traps from moving the bootstrap into an invisible child, both found by
`tests/rtl_links.rs`:
- The hit test does not look at `WidgetRect::visible`, so a measuring copy of
  a link laid out under the pointer took the press. The children now sit below
  the window, where every interact rect is clipped to nothing.
- `Ui::new_child` advances the parent's automatic id counter even with an
  explicit id. A measuring child laid out before the slice on some frames
  (bootstrap, catch-up) and not on others gave the slice's widgets different
  ids from frame to frame, so a press on a link did not belong to the link
  drawn on the next frame. The explicit scope id fixes that for the catch-up
  measurement too, which already ran before the slice.

### A measuring pass must not leave state for the painted ui

`scripts/visual-regression.sh` passed on this branch, but its frames painted
less than main's (4,803 against 5,207 content pixels, deterministic over three
runs): the fixture's note column wrapped. Tables keep their last column widths
and bound in egui memory. The measuring pass, now run in the same frame just
before the painted slice, stored them, so the painted table met "last frame's"
widths on its first paint and derived drag caps from them while its own
egui_extras state was still fresh. Every column shrank by 2 pt one frame
earlier than on main, before the reflow that follows the fixture's math
images, and stayed narrow. With the measuring pass leaving that state alone
the frames match main's pixel for pixel in the document pane.

The 2 pt shrink itself also happens on main, one frame later; see Future
Improvements.

### Precision needs small, positive, phase-aligned coordinates

Each attempt was caught by `tests/far_positions.rs`:

| layout | worst heading error at 2.5 M pt |
|---|---|
| main (absolute coordinates) | 176 pt |
| chunks at origin 0, phase-aligned, 64 K chunks | < 1 pt |
| same, origin −2^18 (above the window) | 150 pt |
| origin +2^15, 32 K chunks, f32 anchor | 1.5–2 pt |
| origin +2^15, 32 K chunks, f64 anchor | < 1 pt |

Without phase alignment, progressive and one-shot geometry disagreed by 3 pt
on one block (`progressive_measure.rs`): egui snaps some sizes to physical
pixels, so a child shifted by half a pixel lays the same block out
differently.

### The first fixture could not fail

At 100 % scaling every row height in the fixture was a multiple of 0.5 pt,
which f32 holds exactly at any height: all paths passed on main. 125 %
scaling makes rows snap to 0.8 pt and the test fail on main by 111–176 pt.

## Tests

- `tests/far_positions.rs`: 1,600 identical sections reaching 2.5 M pt at
  125 % scaling; every heading must sit a whole number of section heights
  below the first (±1 pt) after progressive measurement, a one-frame
  bootstrap, a catch-up to a deep offset, an offset past the estimate, and
  `force_full_render` at a deep offset. All five failed on main (111–176 pt).
- `tests/table_measure_state.rs`: the frame after the measuring frame keeps
  the table's column widths. Red with the measuring pass storing them, green
  on main.
- `tests/pending_offset.rs`: an offset past the deepest position, on a
  measured document and on a bootstrap frame, must land on the measured bottom
  and not move for 60 frames. Both failed on main (221 and 334 pt of drift).
- Mutants, each red: chunking off (`MEASURE_CHUNK` = ∞), all five far-position
  tests; the clamp removed, one pending-offset test; the deferral removed, the
  other.

## In the app

Xvfb, optimized builds, 5 MB prose (`prose_5m.md` from devlog 100) with an
anchor link to the last heading at the top:

| build | after the jump | 25 s later |
|---|---|---|
| main | lands at the bottom (2,162,526) | slid to 2,161,944; the last heading is below the pane |
| this branch | lands at the bottom | still there; "Section 14384" and its paragraph in view |

The same with the outline click (main slid 300 pt). A heading in the middle
(Section 7200) and a deep one not at the end (Section 14000, 125 % scaling)
land 50 pt below the pane top on both builds.

## Future Improvements

- A table's columns shrink by 2 pt on its second painted frame, on main too
  (`table_measure_state.rs` fixture, frame 2): the drag caps derived from the
  stored widths come out 2 pt under each width. Whether the note column then
  wraps depends on whether a reflow (math images loading) resets the widths
  first.

- The renderer's `cargo clippy -D warnings` fails on an empty line after a doc
  comment in `html_table.rs` (on main too).
