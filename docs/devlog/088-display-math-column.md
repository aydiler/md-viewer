# Prose after display math wraps at the column again (#236)

**Status:** Complete
**Branch:** `fix/display-math-column`
**Date:** 2026-11-13

## Summary

With Full Width off, every paragraph after a display formula wrapped a few
pixels wider than the 600 px reading column, for the rest of the document
(14 px further right in the minimal repro, 32 px in the #207 document). Any
display formula triggered it, even `$$x + y$$`.

## Root cause

The display arm of `render_math_with_layout`
(`egui_commonmark_backend/src/misc.rs`) wrapped the formula in
`ui.add_space(8.0)` calls. The enclosing ui is a horizontal wrapping layout, so
`add_space` there is **horizontal**:

- the trailing 8 px pushed the ui's right edge from 800 to 808 (probe:
  `max_rect` before the formula `[200 – 800]`, after it `[200 – 808]`), and
  every later paragraph wrapped against that edge;
- the leading 8 px shifted the formula's centered box right, so the formula
  sat 4 px off-center.

Neither call ever produced vertical space; the `newline` calls around the
`DisplayMath` event do that.

## Fix

Remove both `add_space` calls. The vertical layout is unchanged byte for byte
(Xvfb: the y of every line in the test documents is identical before and
after).

## What did not work first

The issue guessed the `vertical_centered` box was sized from the ambient width
instead of the column, and the first attempt bounded it with
`options.max_width(ui)`. The new test stayed red. A probe printing
`ui.max_rect()` before and after the formula showed the box was already the
column width; the extra 8 px came after it.

## Verification

- `prose_after_display_math_wraps_at_the_column`
  (`tests/centered_column.rs`, `math` feature): runs frames until the
  formula's raster is painted (guarded, so it cannot pass on the placeholder),
  then requires the paragraphs before and after it to wrap identically and the
  formula to be centered on the column within 2 px.
  - red on `main` (wrap differs: "… i " vs "… i jj ");
  - red on the ambient-width attempt;
  - red on a mutant restoring only the leading `add_space`
    ("formula centered at 504.0, column center 500.0");
  - green with the fix.
- Renderer suite, app features: 174 passed. Default features: 186 passed.
  Root: 81 passed. Test inventory vs `main`: nothing missing, one added.
- Xvfb, 1800 px window, Full Width off: second paragraph line ends 1188/1159,
  same as the first, for a top-level `$$x + y$$`, a 2-term and an 18-term
  formula in a list item. Full Width on: unchanged. The #207 document's
  Section 2 now wraps exactly like Section 1.

## Key Discoveries

- In a horizontal wrapping `Ui`, `add_space` is horizontal. Spacing meant to
  separate blocks vertically has to live inside a vertical scope (or come from
  `end_row`/`newline`).
