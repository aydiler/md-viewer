# Feature: Mixed-direction text (bidi, #108)

**Status:** ✅ Complete (reordering); RTL paragraph alignment open
**Branch:** `feature/bidi` (stacked on `chore/egui-0.37`)
**Date:** 2026-11-14

## Summary

A line like `English العربية 123 עברית end` kept its words in typed order:
- each Arabic or Hebrew run read left to right;
- runs were not reordered around numbers.

egui has no released bidi support. The epaint part of the open upstream PR
emilk/egui#8822 (which contains #8818) applies cleanly to our vendored epaint
0.37 and fixes it. Details and the removal recipe are in
`crates/epaint/PATCH.md`.

## Features

- [x] Mixed-direction lines in visual order (UAX #9).
- [x] Pure RTL paragraphs get an RTL base direction: trailing punctuation
  goes to the left.
- [x] Selection and copy across a mixed line: the highlight and the copied
  text agree, in logical order.
- [ ] RTL paragraphs right-aligned in the document. Not done; see below.

## Key Discoveries

### #8822's diff is cumulative
It contains #8818. Apply one or the other, not both. #8818's diff on its own
applies too and fixes the mixed-line order; #8822 adds paragraph direction
and the direction-aware `Row` hit-testing.

### Stock egui 0.37 works with the patched epaint
#8822 also changes egui (`label.rs`, selection painting, caret movement). We
do not vendor egui, but nothing in stock egui 0.37 breaks. Measured on Xvfb
with drag-select plus Ctrl+C:

| selection | highlight | clipboard |
|---|---|---|
| the whole RTL block plus `e` of `end` | that block | ` العربية 123 עברית e` |
| end of `العربية`, `123`, start of `עברית` | those glyphs | `ية 123 עבר` |

The clipboard was read with `tkinter` (no `xclip` on this machine). The
keystroke reached the app only with `xdotool key --window`, after a click.

### The renderer's own tests do not see the vendored epaint
The renderer workspace (`crates/egui_commonmark`) has no `[patch]`, so its
tests build against crates.io epaint. To run them against the vendored
copy:

```
--config 'patch.crates-io.epaint.path="<abs>/crates/epaint"'
```

That also rewrites the renderer's `Cargo.lock`; revert it afterwards. The
regression test therefore lives in the app crate
(`mixed_direction_line_is_in_visual_order` in `src/system_fonts.rs`), which
does build with the patch.
- It goes red against the pre-bidi epaint.
- It needs no installed fonts: a glyph missing from egui's defaults still
  gets a position.

### Reading glyph order off a screenshot is unreliable
A screenshot seemed to show RTL paragraphs with an LTR base. A headless
probe printing each galley's `row.rtl_paragraph` and glyph x positions
showed the opposite: `ש` rightmost, `.` leftmost. Check layout data, not
pixels, for direction.

### Why RTL paragraphs still start at the left
The renderer lays out the whole document in one wrapping left-to-right
`Ui`; paragraphs are not scopes. #8822's start alignment shifts rows
inside a galley, but the galley itself is placed at the row's left edge.
Right-aligning would mean:
- buffering each RTL paragraph into its own right-to-left scope, the way
  headings are buffered;
- and that touches search highlights, `split_points`, list markers and
  outline positions.

Left for a separate change.

## Architecture

- `crates/epaint`: #8822's `src/text/*` changes, plus
  `[dependencies.unicode-bidi]` (also in `crates/epaint/Cargo.lock`).
- `src/system_fonts.rs`: one test.
- No app or renderer code changes.

## Testing

- App 81, renderer with the app's features (against stock epaint, and
  again against the vendored one), epaint `--lib` 88: all pass.
- Clippy is clean.
- Mutation: the new test fails with the pre-bidi `crates/epaint`.
- Xvfb: `bidi.md` (mixed line, Arabic, Hebrew, number inside RTL, an RTL
  list item, English control) and the emoji/script doc.

## Future Improvements

- Right-align RTL paragraphs (see above).
- Drop the patch once an egui release includes #8818/#8822.
