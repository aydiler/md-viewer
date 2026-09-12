# Fix: Wide blocks painted over the right sidebar; sidebar resize reset table widths

**Status:** ✅ Complete
**Branch:** `fix/wide-block-clip`
**Date:** 2026-09-13
**Lines Changed:** ~+120 / -30 in `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`, `crates/egui_commonmark/egui_commonmark_backend/src/elements.rs`, plus `tests/pane_clip.rs`

## Summary

Three user-visible reports around wide tables/code blocks and the right
(outline) sidebar:

1. "I can drag tables over the right sidebar" — columns sliding over the
   outline while drag-panning/resizing.
2. "Code blocks also go above the sidebar."
3. "Resizing the right sidebar resets table size."

## What the investigation actually found

- **egui clip asymmetry is real but not the visible leak.** `SidePanel`s clip
  to their own rect; `CentralPanel` content clips to `ctx.content_rect()`
  (whole window), and `Ui::new_child` clones the parent painter — so the
  document inherits a window-wide X clip. However every wide block lives in a
  `ScrollArea::horizontal` whose inner rect is bounded by its parent's
  *available* rect (pane-bounded), so its content clip is `pane + 3px`
  (`clip_rect_margin`). Xvfb E2E (body drag, separator drag, wheel, sidebar
  resize, code selection drag) confirmed: 0.2.2 paints wide blocks at most
  ~3px past the pane edge — covered by the content gutter.
- **The first cut of the fix was a no-op**: capping carve-outs at
  `ui.clip_rect().right()` does nothing because that clip is window-wide.
- **The second cut (painter clip) was actively dangerous**:
  `ui.set_clip_rect(clip ∩ max_rect)` inside the carve-out erased HTML tables
  entirely — in the HTML/main-wrap path the scope's `max_rect` is
  **degenerate (zero height)**, and layout overflowing `max_rect` is legal
  (only the painter clip forbids it). Clipping to `max_rect` clipped to a
  zero-height rect. Reverted.
- **The visible bug is the width policy.** `TableBuilder` keeps user-resized
  widths and never re-shrinks them, so the old reset-on-any-bound-change
  fired on every sidebar nudge — and since columns typically fill the pane,
  *any* shrink overflows, so it reset *every* time.

## Final changes

- Carve-out width caps now use `ui.max_rect().right()` (the actual viewport
  column): bootstrap document column, viewport-slice re-anchor (recorded
  width + `left_offset` used to re-extend past the pane), both table scopes.
  Floors keep rects non-inverted; painter clips are untouched.
- `table_bound_shrink_discards_widths` → `table_shrink_rescale_widths`:
  * growth / unchanged bound → keep the user's widths;
  * shrink they still fit → keep;
  * shrink they can't fit → **rescale the persisted widths proportionally**
    to the new bound (floored at each column's minimum), `reset()` so the
    scaled `Column::initial` values take effect. Proportions survive; no more
    reset-to-fresh-layout. A drag-widened column overflowing by choice is
    never punished — the bound history separates it from a genuine shrink.

## Testing Notes

- `tests/pane_clip.rs` renders through `show_scrollable` (the `.show()` path
  has no renderer-owned ScrollArea and cannot express the geometry), asserts
  `min(shape.rect.right, shape.clip_rect.right) <= pane_right + 8px` on the
  bootstrap and viewport-slice passes, plus an edge-reach guard against
  vacuous fixtures.
- **Every regression test must be verified red on pre-fix code.** Two
  "passing" tests here were vacuous: budget-capped columns never overflow at
  rest, and the blockquote shrinks the table bound so the fixture never
  overflowed. The harness also had a double `begin_pass` that zeroed pointer
  deltas — drags silently did nothing.
- `resizable_table_state_reflows_when_its_bound_changes` and the unit test
  pin the rescale policy (growth keeps, shrink-that-fits keeps,
  shrink-that-overflows scales proportionally).
- Full suites: renderer lib 99 ✅, wrapping 29 ✅, integration ✅, backend ✅;
  app tests + clippy clean.

## Future Improvements

- [ ] The ~3px `clip_rect_margin` overshoot past the pane edge (two nested
      scrollers) is invisible under the 16px content gutter; revisit only if
      the gutter shrinks.
- [ ] HTML tables share the markdown-table scope/rescale code path shape but
      are separate functions — unifying them would halve this surface.

## Follow-up (user E2E): "dragging the table from the almost-right spot drags it behind the sidebar"

Xvfb reproduction pinned the real interaction: pressing within
`SIDEBAR_RESIZE_GRAB_RADIUS` (8px) of the sidebar border — where users reach
for a table's rightmost column separator or scrollbar — grabs egui's
**invisible** `SidePanel` resize strip (`SidePanel` paints no affordance; the
only hint is a cursor change). The sidebar then narrows/widens over the table
and the reflow reads as "the table slid behind the sidebar". The squeeze
survived into the persisted width: the outline collapsed to a ~90px sliver
with its heading truncated.

**Fix:** `paint_sidebar_resize_affordance` — a 2px line at the panel border
that appears (hovered stroke) when the pointer is on the grab strip and stays
bright (active stroke) while resizing, for both the outline (right) and
explorer (left) sidebars. It reads the panel's `__resize` response the same
way `SidePanel::show` does (one frame of latency, matching egui), and paints
in the Foreground layer over the gutter. No input is stolen: the affordance
paints; the resize interaction stays egui's.

**Verification:** Xvfb screenshots — hover brightens the boundary line,
mid-drag keeps it at the moving boundary while the sidebar resizes.

## Follow-up 2 (user E2E): the reset survived the rescale — widths are now fully sticky

User E2E: both symptoms persisted. Root cause of the survival: `height_layout_changed`
also fires when `table_layout_key` changes, and the key hashed the
pane-width-derived `visible_column_budget` — so *every* sidebar resize flipped
it and reset the widths no matter what the shrink policy decided. The
proportional rescale was dead code on this path.

**Final policy — column widths are fully sticky across pane-width changes:**

- `visible_column_budget` (and the dense/fitting branch it fed) removed from
  `table_layout_key`. The key now covers only inputs that genuinely change
  measured heights: desired/minimum widths, line height, body/mono fonts,
  content digest, layout revision, math scale.
- No rescale, no bound tracking: last frame's widths (shadowed via
  `store_table_column_widths`) are re-measured for reserved heights and
  rendered as-is; the outer horizontal scroller absorbs any overflow
  (existing #64/#110 mechanism). `reset()` happens only on genuine layout
  changes (font, zoom, content, math scale).
- `SIDEBAR_RESIZE_GRAB_RADIUS` 8 → 3: the invisible strip no longer reaches
  table separators near the pane edge; the hover/drag affordance line
  (previous commit) keeps resizing discoverable.

**Xvfb verification:** press 5px inside the pane at a table row + drag →
nothing moves (previously: sidebar narrowed to a sliver); sidebar resize →
table pixel-identical across the drag (previously: widths reset/reflowed).

Sticky-width fallout handled: `markdown/html_table_reflows_after_panel_width_changes`
now assert *identical* heights across widths; `dense_overflow_cache_*` and
`table_layout_key_*` updated for the budget-free key (the dense/fitting key
separation test was deleted with the mechanism).
