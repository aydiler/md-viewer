# Fix: Wide blocks painted over the right sidebar; sidebar resize reset table widths

**Status:** ✅ Complete
**Branch:** `fix/wide-block-clip`
**Date:** 2026-09-13
**Lines Changed:** ~+120 / -30 in `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`, `crates/egui_commonmark/egui_commonmark_backend/src/elements.rs`, plus `tests/pane_clip.rs`

## Summary

Three user-visible symptoms, one shared root cause plus one coarse policy:

1. A wide table could be drag-panned so its columns drew **on top of the right
   (outline) sidebar**; code blocks leaked the same way.
2. Code blocks overlapped the sidebar at the pane's right edge.
3. **Resizing the right sidebar reset every table's manually resized column
   widths.**

The escape hatch: egui 0.33 clips `SidePanel`s to their own rect
(`panel.rs` `set_clip_rect(panel_rect)`), but clips `CentralPanel` content to
`ctx.content_rect()` — the **whole window**. `Ui::new_child` clones the parent
painter, so every descendant inherits that window-wide clip; an explicit child
`max_rect` is deliberately *not* intersected with it (the carve-out feature
from #64 relies on exactly that). The outline panel paints *before* the
central panel each frame, so anything the central panel paints beyond its own
max_rect lands on top of the sidebar. Prose wraps and never exceeds the pane —
only the two wide-block types, both horizontal scrollers, ever did.

## Fixes

- **Clip discipline** — every carved-out scope is now capped at the scroll
  viewport's right edge (`ui.clip_rect().right()`), which is pane-bounded
  because the renderer-owned vertical ScrollArea sets it:
  - document column cap in the bootstrap/full-render pass (`show()`);
  - the same cap when the steady-state viewport slice re-anchors recorded
    geometry at `content_left` (margin/indentation used to re-extend it);
  - the table and HTML-table carve-out rects (`ui.cursor()` + `table_bound`);
  - the code-block frame width in `elements::code_block`.
  The horizontal drag-pan feature is untouched — columns just can't leave the
  pane anymore (egui's nested 3px `clip_rect_margin` allowances remain and are
  covered by the app's content gutter).

- **Smarter width reset** — `table_layout_bound_changed` (reset on *any* bound
  change) is replaced by `table_bound_shrink_discards_widths`: reset only when
  the bound **shrank** *and* the persisted widths no longer fit it. Widths are
  shadowed each frame by `store_table_column_widths` (from `body.widths()`).
  Growth keeps the manual layout (`auto_shrink` closes the gap); a
  drag-widened column overflowing by choice is never punished, because the
  bound history distinguishes a drag from a real shrink.

## Key Discoveries

### CentralPanel is not clipped to its own rect

```rust
// egui 0.33 containers/panel.rs
panel_ui.set_clip_rect(panel_rect);      // SidePanel — overflow hidden
panel_ui.set_clip_rect(ctx.content_rect()); // CentralPanel — whole window!
```

### egui_extras column drags do not redistribute width

`TableBuilder` resize changes one column by the pointer delta without taking
space from its neighbours, so the width sum can overflow the bound *by
choice*. Any "reset when widths overflow the bound" rule would fight the user
mid-drag; the shrink detection must be bound-history based.

## Testing Notes

- `tests/pane_clip.rs` renders an indented wide table + long code line through
  `show_scrollable` (the production path — `.show()` never creates the
  document ScrollArea whose clip makes the cap bite) and asserts
  `min(shape.rect.right, shape.clip_rect.right) <= pane_right + 8px` on both
  the bootstrap and the viewport-slice pass. The 8px slack = two nested 3px
  scroll clip margins + rounding. A second test guards the fixture from going
  vacuous (content must reach the pane edge).
- `resizable_table_state_reflows_when_its_bound_changes` now codifies the new
  policy: growth retains, shrink-that-fits retains, shrink-that-overflows
  resets to the fresh `Column::initial` contract.
- Full suites: renderer lib 99 ✅, integration targets ✅ (incl. 29 wrapping),
  backend 15 ✅; app tests + clippy clean.

## Future Improvements

- [ ] Scale (rather than reset) persisted widths on an overflowing shrink, to
      preserve relative column proportions.
