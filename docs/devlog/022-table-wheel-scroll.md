# Feature: Wheel-routing for nested horizontal ScrollAreas (wide tables)

**Status:** ✅ Complete
**Branch:** `feature/table-scroll`
**Date:** 2026-05-15
**Lines Changed:** +56 / -3 in `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`

## Summary

Wide markdown/HTML tables are wrapped in a nested `ScrollArea::horizontal()` inside the document's outer `ScrollArea::vertical()`. Plain mouse-wheel over a table body was routed to the outer (vertical) area, so users could only scroll the table sideways by grabbing the bottom scrollbar — exactly the complaint in issue #4. This PR redirects vertical wheel deltas to horizontal scroll when the cursor is hovered over a still-scrollable nested area, with pass-through at the edges so the page can keep scrolling after the table reaches its boundary.

Closes the second half of issue #4 (the first half — search/find — shipped in v0.1.4).

## Features

- [x] `forward_wheel_to_horizontal_scroll()` helper added in `pulldown.rs`.
- [x] Markdown table call site (around former line 653) captures `ScrollAreaOutput` and calls the helper.
- [x] HTML table call site (around former line 1185) captures `ScrollAreaOutput` and calls the helper.
- [x] Edge-pass-through: at `offset.x=0` (wheel-up) or `offset.x>=max_x` (wheel-down), the helper bails out so the outer area scrolls the page.
- [x] Trackpad horizontal swipes (X delta) untouched — only Y is intercepted.
- [x] E2E verified with xdotool on Xvfb against `/tmp/wide-table-test.md`.

## Key Discoveries

### `ScrollArea::horizontal()` only consumes X delta
The inner `ScrollArea::horizontal()` listens for the X component of `smooth_scroll_delta`. Plain mouse-wheel emits only a Y component, so the inner area sees nothing and the delta flows up to the outer vertical area. Shift+wheel produces the same bug — egui 0.33 does not auto-convert Y→X for nested horizontal areas. The fix has to be added by the caller post-`.show()`.

### `ScrollAreaOutput` exposes everything the caller needs
The `inner_rect`, `content_size`, `state`, and `id` fields are all `pub`. `state: State` is `#[derive(Copy)]`, so `out.state.store(ctx, id)` makes a copy and the caller's `out` stays usable. The `id` already includes any `id_salt` applied at construction, so `state.store(ctx, out.id)` round-trips correctly.

### Sign convention: `new_offset.x = old_offset.x - smooth_scroll_delta.y`
This mirrors the outer ScrollArea's vertical relationship (`new_offset.y = old_offset.y - smooth_scroll_delta.y`). Wheel-down emits `dy < 0`; the subtraction makes `offset.x` increase, scrolling the table right. Verified empirically with xdotool wheel events.

### Edge pass-through is the entire UX
Without it, a table at its right edge swallows further wheel-down events and the user gets stuck — the natural "I want to read past the table" flow breaks. The fix: when `offset.x >= max_x && dy < 0`, return early before touching `smooth_scroll_delta`. The outer area then sees the still-unconsumed Y delta and scrolls the page. Same logic mirrored for the left edge.

### `ui.rect_contains_pointer(inner_rect)` is cheaper than allocating a `Response`
`inner_rect` is already a `Rect` returned by the ScrollArea — no need to create a new `Response` just to call `.hovered()`. Direct rect-pointer test does the same job in one call.

## Architecture

### New Functions

| Function | Purpose |
|----------|---------|
| `forward_wheel_to_horizontal_scroll<R>(ui: &Ui, out: &mut ScrollAreaOutput<R>)` | After a nested horizontal `ScrollArea` has rendered, check hover + pending Y delta, redirect into horizontal offset, zero the consumed Y delta to block the outer area from also consuming. Generic over the inner closure return type so it works at both table call sites. |

## Testing Notes

E2E recipe (mirrors the original bug-confirmation flow):

```bash
# Xvfb already up
setsid env DISPLAY=:99 WINIT_UNIX_BACKEND=x11 WAYLAND_DISPLAY= \
  ./target/debug/md-viewer /tmp/wide-table-test.md </dev/null >/dev/null 2>&1 &
sleep 4
WID=$(DISPLAY=:99 xdotool search --name "wide-table" | head -1)
DISPLAY=:99 xdotool windowsize $WID 1280 800
DISPLAY=:99 xdotool windowmove $WID 0 0

# 1. Cursor on table body → table scrolls right, page stays put
DISPLAY=:99 xdotool mousemove 600 250
for i in $(seq 1 10); do DISPLAY=:99 xdotool click 5; done
# Compare /tmp/postfix-2 vs /tmp/repro-2: columns shifted, heading unchanged.

# 2. Cursor below table (y=600) → page scrolls (regression check)
DISPLAY=:99 xdotool mousemove 600 600
for i in $(seq 1 10); do DISPLAY=:99 xdotool click 5; done
# /tmp/postfix-5: heading gone, end markers visible.

# 3. Edge passthrough: scroll table to right edge, then wheel-down → page scrolls
DISPLAY=:99 xdotool mousemove 600 250
for i in $(seq 1 80); do DISPLAY=:99 xdotool click 5; done
# /tmp/postfix-8: page reached bottom (table at right edge, excess wheel passed through).
```

All three scenarios pass on debug build. `cargo clippy --bin md-viewer -- -D warnings` clean. All 13 existing unit tests still pass.

## Future Improvements

- **Priority 3 in `TARGET_METRICS.md`**: Resizable column dividers via `egui_extras::TableBuilder` swap. Separate, much larger initiative — requires re-implementing cell-content rendering through the TableBuilder row API, which doesn't directly support the recursive `self.event()` markdown-in-cell pattern.
- **Touch trackpad pinch zoom** over a table: works at the egui context level, no change needed.
- **Mobile/touch drag-to-scroll**: ScrollArea handles this natively; not affected by this PR.
