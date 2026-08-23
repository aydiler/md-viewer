# Feature: App-owned side-panel resize strips

**Status:** ✅ Complete
**Branch:** `fix/sidepanel-drag`
**Date:** 2026-08-23
**Lines Changed:** ~+180 / −20 in `src/main.rs`

## Summary

Replaced egui's built-in `SidePanel::resizable(true)` with app-owned invisible
drag strips registered after all content, fixing two measured defects: the
outline (right) separator could only be grabbed through a ~2px corridor on the
content side, and presses landing on selectable text or the document scrollbar
stole the gesture entirely. Panel widths now live in app state, persist across
restarts, and double-clicking a separator resets it to 200px.

## Problem (measured, Xvfb + synthetic input + pixel diff)

With `resize_grab_radius_side = 2.0` the built-in hit strip is ±2px around the
separator. egui's hit-test picks the *closest containing* widget for a press,
so anything that contains the pointer outranks the nearby strip:

| Press position (edge x=1000) | Pre-fix behaviour |
|---|---|
| x = 998–1004 | resize engages |
| x = 994–997 (3–6px into content) | **text selection** in the document |
| scrollbar zone x ≈ 989–997 (thumb under cursor) | **scroll drag**, not resize |

The explorer separator fared better (~±6px) because its neighbours are empty
frame margins — the asymmetry itself was part of the complaint. Restoring
`resize_grab_radius_side = 5` would widen the strip but *not* fix the steal:
text galleys register as `click_and_drag` widgets at distance 0.

## Features

- [x] Invisible ±6px strip over each panel's inner edge, registered inside the
      CentralPanel pass *after* document content so later-registration wins
      egui's same-position tie-break
- [x] Widths owned by `MarkdownApp` (`explorer_width`, `outline_width`),
      clamped to the previous min/max ranges (150–300 / 120–400)
- [x] Applied via `SidePanel::exact_width` every frame — immune to egui's
      cached `PanelState`
- [x] Cursor + highlighted separator line while hovered/dragging
- [x] Double-click separator → reset to 200px
- [x] Widths persisted in `PersistedState` (survive restarts)

## Key Discoveries

### egui resolves presses by closest-containing, not topmost-overall

`hit_test()` (egui 0.33 `hit_test.rs`) collects widgets within
`interact_radius` (default 5.0) and picks the top-most under the pointer *or
the closest one*. A widget that contains the pointer (distance 0) always beats
a nearby-but-not-containing strip — widening `resize_grab_radius_side` alone
cannot win those contests. The only robust fix is registering your own widget
that *contains* the press point and is registered later than the competitors.

### Later registration wins same-layer ties; clip rects bound strips

Registering the strips inside the CentralPanel closure keeps them in the
background layer (wheel routing, tooltips and menus unaffected), and because
they are registered after the document renderer, they win ties against text
galleys and scroll handles. One side effect: a Ui's interact rect is clipped
to its own clip rect, so each strip effectively covers the content flank plus
`interact_radius` beyond it (~11px corridor) rather than the full ±6px band.

### exact_width beats PanelState caching

`SidePanel` loads its width from `PanelState` in egui memory every frame,
ignoring builder width. But `exact_width(w)` pins `width_range = w..=w`, so
the stale state gets clamped back to our value — no memory surgery needed.

### Trade-off: wheel-dead band

Pointer within the strip band no longer wheel-scrolls the document (ScrollArea
only consumes wheel when hovered, and the strip is now on top). The dead band
is ≤6px hugging the separator — comparable to other editors' separators.

## Architecture

### State

```rust
explorer_width: f32,   // MarkdownApp, persisted via PersistedState
outline_width: f32,
struct StripState { hovered, dragged, double_clicked, pointer_x }
```

### Flow

```
render_file_explorer() -> (ExplorerAction, Rect)   // panel rect exposed
render_outline()       -> Rect
CentralPanel::show(|ui| {
    render_tab_content(...);                       // content first
    render_panel_resize_strips(ui, e_rect, o_rect) // strips last = topmost
})
```

## Verification

Xvfb :99 harness (`xdotool` synthetic drags, ImageMagick column diffs):

- Outline grab: dx −6…+4 from edge all engage resize (was selection/scroll steal)
- Explorer grab: dx −4…+6 engage; double-click reset verified (869→1000)
- Clamp: dragging far past max lands exactly at max_width (800 = 1200−400)
- Persistence: resized widths restored after process restart
- `cargo test`: 47 passed; clippy clean on the binary
