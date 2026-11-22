# 112: Copy button without a second scrollbar inset

**Branch:** `fix/copy-button-gutter`
**Date:** 2026-11-22
**Status:** Complete

## Summary

#264 kept the code-block copy button clear of a floating scrollbar by
insetting its slot by `bar_width + bar_outer_margin`. Devlog 111 (#271) then
made the renderer's own scroll area leave a gutter of the bar's full reach on
the right of the column, so inside `show_scrollable` a code block already ends
before the bar, and the button's inset reserved the bar a second time: the icon
ended 18 px from the block's corner instead of 8 (one `button_padding.x`).

## Architecture

- `end_code_block` (`parsers/pulldown.rs`) writes a per-block flag to egui temp
  memory before `CodeBlock::end`: `scrollbar_clear_id(id) -> scrollbar_gutter > 0`.
  It is written on every pass, so it cannot outlive the gutter.
- `code_block` (`egui_commonmark_backend/src/elements.rs`) reads it and drops
  its `bar_reach` when the column already clears the bar.
- `scrollbar_clear_id` is a `#[doc(hidden)]` helper in the backend so both
  crates build the same key.
- The block `Id` (`scope_id().with("_code_block").with(n)`) is unique per
  block, so a plain `show()` viewer never sees the flag and keeps the inset
  (`tests/copy_button.rs`, unchanged).

No public signature changed: a parameter on `code_block`/`CodeBlock::end` or a
field on `CommonMarkOptions` would have been a breaking change for a cosmetic
fix. Toggling `spacing.scroll.floating` around the call was rejected too: it
would also change the code block's own horizontal scrollbar, and with it the
block's height.

## Key Discoveries

- The first version of the test allowed `2 × button_padding.x + 3` of slack
  and passed on the unfixed code (18 px gap against 19 allowed). Measuring
  both builds first gave 18 px before and exactly 8 px after, so the bound is
  now `button_padding.x + 1.5`, and the mutant that ignores the flag is red
  ("18 px short of the block corner, allowed 9.5").

## Tests

`tests/scrollbar_gutter.rs::the_copy_button_sits_in_the_block_corner_once_the_gutter_clears_the_bar`:
in `show_scrollable` with a floating bar, the icon ends within one button
padding of the block's right edge and not past the gutter line.

## Future Improvements

- None.
