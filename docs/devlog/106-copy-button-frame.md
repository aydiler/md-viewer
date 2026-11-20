# The code-block copy button: a frame that fits, clear of the scrollbar

**Status:** ✅ Complete
**Branch:** `fix/copy-button-frame`
**Date:** 2026-11-20
**Issue:** #256

## Summary

Hovering the copy button in a code block showed a frame about twice as tall
as the icon, stretching downward. Checking it on Xvfb turned up a second
problem: with Full Width (the default) the right half of the button lay under
the document's scrollbar, and a click there went to the scrollbar and copied
nothing.

The button now sits in a right-aligned slot inside the block's top-right
corner. The slot is as wide as the block, and is inset by a floating
scrollbar's width when scrollbars float, which is egui's default.

## Key Discoveries

- **The tall frame was a wrapped label, not the font.** With md-viewer's
  fonts the label's galley is 22 px tall at 16 px, the same as an "A". The
  button was 36 px tall because `ui.put` into a zero-size rect measures the
  widget against zero available width. The label is "📋" followed by U+FE0E,
  so it wrapped into two rows.
- **egui's scrollbars float over the content.** `ScrollStyle::floating` is the
  default, so the bar takes no layout width, and a code block that fills the
  pane reaches under it. Hovering expands the bar to `bar_width` (10 px) at
  the pane's right edge, exactly where the button was.
- **Measured on Xvfb (main → branch):**
  - the button rect went from 24.4 × 36 to 24.5 × 18;
  - clicks at x = 1012 and 1019 copied, while x = 1026 hit the scrollbar;
  - after the fix, clicks at the button's left edge, center and right edge
    all copy, and the button ends at 1016, before the bar.

## Architecture

- `code_block` (`egui_commonmark_backend/src/elements.rs`) places the button
  through `scope_builder` with `Layout::right_to_left(Align::TOP)` in a slot
  from the block's left edge to its right edge minus the floating bar's reach
  (`bar_width + bar_outer_margin`) and the button padding.

## Testing

- `tests/copy_button.rs` finds the painted icon, hovers it, and checks three
  things:
  - the label is laid out on one row;
  - the hover frame is no taller than one row plus padding;
  - with floating scrollbars, the frame ends at least `bar_width` inside the
    block.
- On main it fails because the label wraps onto 2 rows. With the slot alone
  it passes, so a `wrap_mode(Extend)` I had added as well was redundant and
  was removed.

## Future Improvements

- Code blocks (and tables) still extend under the floating scrollbar in Full
  Width mode. A right gutter as wide as the bar would keep their content clear
  too.
