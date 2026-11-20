# Feature: Edit-mode selection scrolls and survives the scrollbar (#258)

**Status:** ✅ Complete
**Branch:** `fix/edit-drag-autoscroll`
**Date:** 2026-11-20
**Lines Changed:** `src/main.rs` (edit branch of `render_tab_content`, `EditorFocus`, `edge_autoscroll_speed`, `edit_scrollbar_strip`, tests)

## Summary

Issue #258 reported two problems in Edit mode:

1. Drag-selecting text did not scroll when the pointer reached the top or bottom of the editor, so a selection could not extend past the visible lines.
2. "If you try to scroll using the scrollbar, the selection is cleared."

Both are fixed in md-viewer's edit branch; egui is not patched.

## Features

- [x] A drag selection held near or past the editor's top or bottom edge scrolls the text (`edge_autoscroll_speed`).
- [x] A press on the editor's scrollbar strip keeps the TextEdit's focus and selection (`EditorFocus::keep_through_scrollbar_press`).
- [x] A click elsewhere still takes the focus away, as before.

## Key Discoveries

### TextEdit scrolls to its cursor only when the selection changes

egui's `TextEdit` asks its ScrollArea to show the cursor only on a frame where the cursor moves. A pointer held still below the editor moves nothing, so nothing scrolls. Reproduced on Xvfb with xdotool: on `main`, screenshots taken at the start and 2 s into a held drag were byte-identical.

The fix reads the pointer while the TextEdit response is `dragged()`. Within `EDGE_AUTOSCROLL_ZONE` (24 pt, at most a quarter of the area) of either edge, or past it, it moves the offset by `speed * stable_dt` through `pending_edit_offset`, the same next-frame path the edit/preview scroll sync already uses. Speed starts at 120 pt/s and grows by 12 pt/s per point of depth, up to 3000 pt/s. The band inside the edge matters on a touchpad or a maximised window, where the pointer cannot leave the area. The TextEdit extends the selection itself as text moves under the pointer. On Xvfb a 2 s hold went from line 002 to line 104 with the selection intact.

### A thumb drag kept the selection; a track click did not

The report says "scrollbar". Measured on `main`:

| action | focus after | selection after |
|---|---|---|
| drag the thumb | kept | kept (a typed key replaced it) |
| click the track | lost | gone (a typed key went nowhere) |

So the bug is the track click. A probe printing focus and the TextEdit's cursor range every frame showed why: the click arrives as a press and release in one frame, egui moves the focus away, and on that frame the TextEdit **collapses its selection to the cursor**. `TextEdit::show` does that whenever it does not own the IME events (`builder.rs`, `if !owns_ime_events { … CCursorRange::one(r.primary) }`), which is the case once it is unfocused. Restoring the focus alone would therefore have brought back a cursor without a selection.

`EditorFocus` keeps the selection from the last focused frame. If the editor had focus, lost it this frame, and the press was in the scrollbar strip, it writes the selection back into the TextEdit state and requests focus again.

### Three traps found by the probe

- **`press_origin()` is `None` for a quick click.** Press and release in one frame leave no origin; `latest_pos()` is where the press happened.
- **The bar was outside the computed strip.** With floating scrollbars the strip was taken as `inner_rect.right() - reach .. inner_rect.right()`, but on Xvfb clicks at 3 pt *right* of `inner_rect` hit the bar. `edit_scrollbar_strip` now covers `reach` on both sides of the edge.
- **Read focus from memory.** In the probe, `Response::has_focus()` on the response returned through the frame and `ui.vertical` read `false` on frames where `ctx.memory().focused()` was the TextEdit. The code uses `memory.has_focus(id)` throughout.

### Xvfb keyboard input needs a focused window

`xdotool key --window <id>` did not reach the app until `xdotool windowfocus --sync <id>` ran first (there is no window manager on Xvfb). Without it, a typed key changing nothing looks like "focus lost" for every build, including the fixed one. The control run (select, type, no scrollbar) settled it.

## Architecture

```rust
struct Tab {
    // …
    edit_focus: EditorFocus,   // focus + selection as of the last frame
}

#[derive(Default)]
struct EditorFocus {
    had_focus: bool,
    selection: Option<egui::text::CCursorRange>,
}
```

- `EditorFocus::keep_through_scrollbar_press(ui, edit_id, inner_rect)`: called after the editor's ScrollArea is shown.
- `edit_scrollbar_strip(inner_rect, &ScrollStyle) -> Rect`.
- `edge_autoscroll_speed(pointer_y, top, bottom) -> f32` (points per second, negative is up).

The edit branch now returns the TextEdit's `Response` out of `Frame::show` and `ui.vertical`, so `dragged()` and its id are available after the ScrollArea.

## Testing

- `edge_autoscroll_speed_is_zero_inside_and_signed_at_the_edges`.
- `a_click_on_the_editor_scrollbar_keeps_focus_and_selection`: real egui frames with a ScrollArea and TextEdit. It focuses and selects, then sends a one-frame click on the strip and asserts focus and the exact selection. Mutants, each observed red: the restore disabled ("editor lost focus to its scrollbar"); focus restored without the selection ("selection collapsed"); no `latest_pos` fallback ("editor lost focus").
- `a_click_outside_the_scrollbar_still_takes_the_focus`: guards against the fix keeping focus everywhere.
- Xvfb (`long.md`, 300 lines, Edit mode): held drag auto-scrolls; track click then a typed key replaces the selection; thumb drag still keeps it.

## Future Improvements

- The preview pane has neither problem in the same form (labels, not a TextEdit), but its drag selection does not auto-scroll either; egui's label selection is cleared when an endpoint leaves the viewport (see LESSONS, "Text selection clears when content leaves viewport").
