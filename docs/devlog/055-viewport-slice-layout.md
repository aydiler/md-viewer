# Fix: viewport slices must reproduce the bootstrap's layout

**Status:** ✅ Complete
**Branch:** `fix/viewport-slice-layout`
**Date:** 2026-08-25
**Lines Changed:** +65 / -20 in `crates/egui_commonmark/`, plus one test and one script

## Summary

After #96 (viewport-clipped rendering) landed on main, a document with a long
table scrolled to the end of the table and stopped. Everything below it — Math
and CJK, code blocks, links, Unicode — rendered blank, and the table itself sat
shifted to the right.

Three separate divergences between the two paint paths caused it. The renderer
paints the first frame of a document in full (the *bootstrap* pass, which also
measures `page_size` and records `split_points`) and every later frame as a
*slice* of the event stream. Each bug was the slice deriving something for
itself rather than reproducing what the measuring pass had done.

Virtualization stays enabled. Disabling it also fixes the symptoms — main
shipped that way before #96 — but costs a full paint every frame.

## Root causes

### 1. The slice recomputed its column

`options.max_width(ui)` was evaluated against the slice's own `Ui`. The
bootstrap and viewport passes reserve scrollbar space differently, so the slice
wrapped content at a different column than the pass that measured the document.

### 2. The slice was squeezed into a fixed-height rect

`slice_rect` spanned `slice_top..=slice_bottom`. Content needing more room than
that bound overflowed the `Ui`, inflating the reported extent, which let the
scroll offset run past the real end of the document. This is the "stuck, with a
blank page below the table" half of the report.

### 3. Line state was never reset for a slice

```rust
// before — never true for a slice starting at event 868
if index == 0 {
    self.line.should_not_start_newline_forced = false;
}
```

The bootstrap clears this after its own first event. A slice starting part-way
into the document never sees index 0, so the flag stayed set, the slice's first
block did not open its own row, and it was placed after the leading inline
space. That was the horizontal shift — measured at exactly 44px in the app.

This one is worth remembering: fixing (1) and (2) cleared the blank content but
left the table at x=268 instead of 224. Correct geometry was not sufficient,
because the offset came from renderer state, not from geometry.

## Architecture

### New struct

```rust
/// Layout geometry of the content column, captured at bootstrap.
pub struct ContentGeometry {
    pub width: f32,        // width the content was laid out (and wrapped) at
    pub left_offset: f32,  // left edge, relative to the scroll area's max_rect().left()
}
```

Stored on `ScrollableCache` beside `page_size` and `split_points`, written by
the bootstrap pass, read verbatim by every slice.

### Changed

| Location | Change |
|----------|--------|
| `show()` | Records `ContentGeometry` alongside `page_size` |
| `show_scrollable()` slice path | Uses the recorded width/left instead of recomputing |
| `show_scrollable()` slice rect | Zero-height, mirroring the bootstrap's `allocate_ui_with_layout(vec2(max_width, 0.0), ..)` |
| slice paint loop | Line-state reset keyed off the slice's first event |

## Testing notes

Verified end to end on Xvfb against the reporter's document shape:

| | Stock (#96) | Fixed |
|---|---|---|
| Content pixels per frame | 2691, 3076, 3115, **483, 483, 483, 483** | 2789, 33862, 2939, **4146, 4146, 4146, 4146** |
| Table left edge, deep scroll | 268 (shifted 44px) | **224**, matching the bootstrap |
| Large document, 7 positions | — | no blank frames |
| Slice vs full paint (59 574 px doc) | — | **2.3 ms vs 22.7 ms, ~10x** |

`scripts/visual-regression.sh` automates that check and is validated in both
directions: exit 1 on the broken build (38 px shift detected), exit 0 on the
fixed one.

**A headless test for the shift could not be produced.** Five formulations were
tried and every one passed on the visibly broken build; details are in
`docs/LESSONS.md`. Shipping one would have been worse than shipping none — #96's
own test passed on the broken build for exactly this reason, and its 1 px extent
bound held only because the bug *pinned* the extent. That bound is now 32 px.

`tests/slice_perf.rs` asserts a slice paint is cheaper than a full paint, so
virtualization silently regressing to always-bootstrap fails the suite. It is
wall-clock based; the margin is ~10x, but it is the kind of test that can get
flaky on a loaded CI machine.

## Future improvements

- [ ] Load a real font stack in the crate's test harness; that is the most
      likely missing ingredient for reproducing the shift headlessly.
- [ ] `pending_scroll_offset` jumps do not stick — a forced bootstrap paints at
      the requested offset while the stored state reports another (verified
      present on stock too, so it predates this work).
- [ ] Consider a newtype for split-point coordinates; screen-y vs content-y
      confusion has now caused bugs in several rounds of this work.
