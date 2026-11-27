# Feature: inline formulas in right-to-left text

**Status:** ✅ Complete
**Branch:** `feature/rtl-inline-math`
**Date:** 2026-11-27
**Lines Changed:** renderer `parsers/pulldown.rs` (`rtl_block_starts`,
`block_is_rtl`, `RtlBlock`, `render_rtl_block`, new placeholder helpers),
backend `misc.rs` (`inline_math_piece`, `inline_math_geometry`,
`prepare_math`), `tests/rtl_alignment.rs`, root `tests/rtl_math.rs`

## Summary

The last #108 gap that could be closed here (after devlog 128). A right-to-left
paragraph, heading or list item with an inline formula went down the
left-to-right path, because `render_rtl_block` draws the block as one galley
and a galley holds only text. Such a block was left-aligned, words in typed
order. Now the formula reserves its box inside the galley and is painted there.

## How the box is reserved

Each formula appends a run of U+FFFC characters in a transparent section:

- **Width:** a glyph's advance grows with its font size, so the size makes
  the run as wide as the formula. The size is capped at twice the body size; a
  wider formula gets more glyphs (`append_math_placeholder`).
- **Height:** the section's `line_height` is the formula's height plus the lift
  that `render_math_with_layout` already uses to put the formula's baseline on
  the text's. The row grows to it, and the text sits at the row's bottom as it
  does next to a tall inline image.
- **Painting** (`paint_rtl_math`): after the `Label`, the placeholder glyphs are
  found in galley order (any U+FFFC in the user's text is swapped for U+FFFD,
  which has the same UTF-8 length). Over each run goes a patch of the page
  background, then the texture at the top of the box.

The backend's `inline_math_piece` does what `render_math` does (poll finished
renders, start new ones, schedule repaints) without allocating, and returns the
texture, the fitted size and the lift. `render_math_with_layout` now calls the
same `prepare_math` and `inline_math_geometry`, so both paths compute the same
numbers.

## Key Discoveries

### Four placeholders that did not work

A throwaway root-workspace probe (patched epaint) tried each:

| placeholder | outcome |
|---|---|
| one glyph widened by `extra_letter_spacing` | epaint adds the spacing before every glyph except the *paragraph's* first (`is_first_glyph_in_section` is set from `paragraph.glyphs.is_empty()`), so n glyphs get n×s, and the gap does not sit next to the glyph |
| one U+FFFC at a font size scaled to the width | exact box, right order, wraps as a unit; but U+FFFC has ink (a replacement box) and is rasterized at every size: 240×450 px of atlas for a 300 px formula |
| a space-like character | epaint trims whitespace (NBSP included) at row edges, so a formula ending a row would hang outside it |
| the LaTeX source, transparent, letter-spaced to the width | negative spacing overhangs into the neighbours, and egui recolours selected glyphs (transparent glyphs still have vertices), so the source would show |

Splitting the scaled glyph into several capped at twice the body size kept the
exact box (within 0.8 px of pixel rounding between glyphs) at ≤ 336 px² of atlas
per glyph.

### The patch is what hides selection

egui's `paint_text_selection` recolours the vertices of every selected glyph to
the selection colour, so the placeholders' replacement boxes appear when the
paragraph is selected. The patch, painted after the label, covers them,
including the part of a replacement box that reaches below the row (the first
Xvfb selection showed a dashed line there until the patch took the glyphs' ink
into account). Its colour is `panel_fill`, which is why formulas stay on the
old path in table cells, whose rows are striped. Blockquotes and alerts only
draw a bar, so they are fine; the renderer detects alerts from their text, not
from pulldown-cmark's `BlockQuote(Some(kind))`, so a rule keyed on that kind
would never have fired. Formulas inside links stay on the old path too.

### Left-to-right text before a formula sits higher

Comparing baselines against a left-to-right paragraph first failed by 5 px. The
right-to-left formula was right; the reference was not. In a left-to-right
wrapping row, a label laid out before a taller widget keeps the row height it
saw, so `abcd` in `abcd $x^2$ efgh` sits 5 px above `efgh`. The test compares
with the text after the formula. That quirk predates this change and is left
alone.

## Testing Notes

- `tests/rtl_alignment.rs` (renderer, formula size seeded with
  `seed_inline_math_size_for_test`): the paragraph sits at the right edge with
  the formula inside it at its size; paint order is text, patch, image.
- Root `tests/rtl_math.rs` (real render): the formula lies between its two
  words in right-to-left order with the same drop below the baseline as in a
  left-to-right paragraph (±1 px); in a wrapped paragraph no glyph overlaps it.
- `rtl_blocks_take_inline_math_only_over_the_page_background`: eligibility,
  with and without the `math` feature.
- All fail on main (run with main's `pulldown.rs` and `misc.rs`).
- Mutations, each caught: no patch (paint-order test), one placeholder glyph
  (`wide_formula_placeholders_stay_small_and_exactly_as_wide`), lift dropped
  (root baseline test). The baseline test runs at a 32 px body: at egui's
  default 12.5 px every formula's lift is 0, and that mutation passed.
- Not caught headlessly: a patch that covers only the row. With egui's default
  fonts the replacement glyph's ink stays inside the row, so the assertion that
  the patch contains every placeholder's ink is a guard only; with md-viewer's
  system fonts the ink reaches below the row, which the Xvfb selection
  screenshot checks.
- Xvfb: a left-to-right math document is pixel-identical to main (0 differing
  pixels, dark and light).

## Future Improvements

- [ ] Inline images in right-to-left text could use the same placeholders,
  sized from the texture poll.
- [ ] Copying gives U+FFFC per placeholder glyph; egui offers no hook to
  substitute the LaTeX source.
