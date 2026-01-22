# Feature: Mouse Wheel Scroll During Text Selection

**Status:** ✅ Complete (with known limitation)
**Branch:** `feature/drag-scroll`
**Date:** 2026-01-22
**Lines Changed:** +30 in `src/main.rs`

## Summary

Allow mouse wheel scrolling while selecting text. Selection persists as long as the selected content remains within the visible viewport.

## Features

- [x] Mouse wheel scroll while selecting text
- [x] Selection preserved during scroll (when content stays in viewport)
- [ ] ~~Edge drag auto-scroll~~ (not feasible - see Known Limitation)

## Known Limitation: Selection Lost When Content Leaves Viewport

**This is an egui design choice, not a bug.**

egui intentionally clears text selection when either selection endpoint scrolls out of the visible area. This behavior is explicitly coded in egui's `label_text_selection.rs`:

```rust
if !state.has_reached_primary || !state.has_reached_secondary {
    // We didn't see both cursors this frame,
    // maybe because they are outside the visible area (scrolling),
    // or one disappeared. In either case we will have horrible glitches, so let's just deselect.
    let prev_selection = state.selection.take();
}
```

### Why This Happens

- egui tracks selection using character indices (content-based)
- However, selection is validated every frame by checking if cursor endpoints were "seen" during rendering
- When labels scroll out of view, they aren't rendered, endpoints aren't "reached"
- egui clears selection to avoid visual glitches

### Impact on This Feature

- Mouse wheel scroll during selection **works** as long as selected text stays in viewport
- Selection **breaks** when selected text scrolls out of the visible area
- Edge-drag auto-scroll was abandoned because it would constantly break selection

## Alternative Solutions (Not Implemented)

### 1. Disable Text Selection Entirely
```rust
ui.style_mut().interaction.selectable_labels = false;
```
Trade-off: No selection, but perfect scrolling. Could be added as a toggle.

### 2. Use TextEdit Instead of Labels
Modify egui_commonmark to render text via `TextEdit::multiline(&mut text.as_str())`. TextEdit handles its own selection state and auto-scrolls to keep cursor visible.

### 3. Custom Selection Handling
Track selection externally as `(start_pos, end_pos)` in content coordinates, render highlights manually, bypass egui's selection system entirely. Requires significant effort.

### 4. Modify egui Core
Change `label_text_selection.rs` to preserve selection based on content positions rather than requiring widgets to be rendered. Would require upstream changes to egui.

## Key Discoveries

### Mouse wheel scroll during selection

egui's `raw_scroll_delta` is available even during text selection (primary button down). By manually applying scroll AFTER rendering, we can scroll while selecting:

```rust
// Get scroll input
let raw_scroll = ui.ctx().input(|i| i.raw_scroll_delta.y);

// Render content first
let mut scroll_output = scroll_area.show_viewport(ui, |ui, viewport| { ... });

// Apply scroll AFTER rendering to preserve selection
if raw_scroll.abs() > 0.0 {
    let new_offset = (current_offset - raw_scroll).clamp(0.0, max_scroll);
    scroll_output.state.offset.y = new_offset;
    scroll_output.state.store(ui.ctx(), scroll_output.id);
}
```

### Boundary protection

Avoid calling `state.store()` when scroll would hit top/bottom boundaries - this can trigger selection loss:

```rust
let would_hit_top = new_offset < 0.5;
let would_hit_bottom = new_offset > max_scroll - 0.5;

if offset_changed && !would_hit_top && !would_hit_bottom {
    // Safe to store
}
```

## Architecture

### Modified Functions

| Function | Purpose |
|----------|---------|
| `render_tab_content()` | Added manual scroll handling for wheel-during-selection |

### Constants

```rust
const EDGE_SCROLL_ZONE: f32 = 50.0;  // Pixels from edge to trigger scroll
const EDGE_SCROLL_SPEED: f32 = 400.0; // Pixels per second at edge
```

## Testing Notes

- Test with long documents
- Test selecting text and dragging to top/bottom
- Test mouse wheel while selecting
- Test with different zoom levels

## Future Improvements

- [ ] Horizontal auto-scroll for wide content
- [ ] Configurable scroll speed
- [ ] Smooth acceleration curve
