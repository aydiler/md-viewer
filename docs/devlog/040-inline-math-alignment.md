# 040 — Inline math: baseline alignment, sizing, spacing, display breaks

**Branch:** `feature/math-rendering`
**Date:** 2026-06-08
**Status:** Implemented, verified live

## Problem

Inline math (`$…$`) rendered as composited typst images looked wrong in several
ways:
1. **Shifted down** — formulas sat below the text baseline.
2. **Weird spacing** — short symbols (`$w$`, `$σ$`) had doubled horizontal gaps.
3. **Inconsistent vertical position** — "some too high, some too low" across
   formula shapes (a plain `x` vs a subscript vs a fraction).
4. **Display equations inline** — `$$…$$` rendered to the right of the preceding
   text (when the source kept them in the same paragraph) and sank down.

## Fixes (in `egui_commonmark_backend/src/misc.rs`, `…/parsers/pulldown.rs`)

### Horizontal spacing — drop the baked-in x-margin
Inline formulas were rendered with a typst page margin of `(x: 2pt, y: 2pt)`. The
2pt side margins baked whitespace into each image, *added on top of* the natural
word spaces in the surrounding text → doubled gaps. Fixed: inline x-margin `0pt`
(the text supplies word spacing). Keep `y: 2pt` so accents/descenders aren't
clipped.

### Display equations on their own line
`DisplayMath` was rendered inline within the paragraph's `horizontal_wrapped`
layout, so `text:\n$$…$$` put the equation to the right of the text and the
row's bottom-alignment pushed the tall equation down. Fixed: emit `newline(ui)`
before and after `DisplayMath` so it always breaks to its own centered line.

### Inline vs display style
Both inline and display formulas were built as `$ {math} $` — **whitespace
inside the `$…$` makes typst render a *block* (displaystyle) equation**:
oversized operators, centered, and *no inline baseline*. Switched inline
formulas to `${math}$` (no whitespace → inline/textstyle): correctly sized for
running text, and the equation now carries a baseline.

### Per-formula baseline alignment
The core of "some high, some low": a single constant lift aligns image *bottoms*,
but each formula's baseline sits at a different height inside its image (plain
`x` near the bottom, a fraction's bar in the middle, a subscript above the
bottom). Solution: read each formula's true baseline and align it.

- `math_baseline_ratio(page)` extracts the baseline as a fraction of image
  height. **Preferred path:** typst sets a real baseline on the inline
  equation's own frame (the *page* frame doesn't carry it, but a nested group's
  frame does) — `find_baseline` recurses to the first frame with
  `has_baseline()`. **Fallback** (block equations): average the y of the
  largest-font text runs (single symbol → its baseline; fraction → the axis).
- `baseline_ratio` is threaded through `MathRendered` → `MathState::Ready`.
- lift = `text_descent − image_descent`, where
  `image_descent = (1 − baseline_ratio)·image_height` (exact, per formula) and
  `text_descent` is the body-text descent **computed exactly from egui's
  metrics** — not a tuned constant. epaint places a glyph baseline at
  `font_ascent` below the row top and adds the line-height leading *below* the
  baseline, so `text_descent = line_height − font_ascent`. `font_ascent` is read
  from a one-glyph reference galley (`painter().layout_no_wrap("x", …)` →
  `rows[0].row.glyphs[0].font_ascent`); `line_height = font_size × 1.5` (matching
  `TypographyConfig::default().line_height = Multiplier(1.5)`).
- **Crucially, the lift is NOT applied by painting the image shifted outside its
  allocated box** — that gets clipped in dense paragraphs (the "cut off at the
  bottom" bug). Instead allocate a box `lift` taller than the image and paint the
  image at its *top*, leaving transparent padding below. Bottom-alignment drops
  the padding to the line bottom and raises the baseline by exactly `lift`, with
  the image always fully inside its box.

The baseline ratios are consistent across shapes (`x`, `H²`, `min(1+w)=…×10⁻¹¹`,
`w_DE`, `c_s²`, `ρ_DE` all 0.866; fraction 0.840), so a single global constant
aligns everything.

`TEXT_DESCENT_FRAC = 0.50`. The app uses 16pt body text at 1.5× line height, so
the descent from the line-box bottom to the baseline is a large fraction of the
line height — hence the value is ~0.5, not the ~0.16 a naive font-descent
estimate suggests. (A `MATH_LIFT` env override was used during tuning, then
removed — it sat in the per-formula, per-frame paint path.)

**Tuning, the hard way.** This took far too many iterations because of two
testing traps:
1. **Crowded-line measurement** — a column-bottom *histogram* of a mixed
   text+formula line can't resolve a few-px global offset (it falls inside the
   baseline peak's spread), so early values (0.16, 0.25) were called "aligned"
   when formulas were actually several px low.
2. **Sparse test doc** — measuring isolated formulas with *blank lines* between
   them gave each formula vertical room, which hid the clipping/over-lift that
   only shows in dense paragraphs. That led to 0.50, which then read as "too
   high / cut off" on the real (dense) doc.

**The real fix (after the tuned constant kept leaving a ~1px residual):** stop
tuning a constant and compute `text_descent` from egui's actual metrics. The
constant was wrong because it multiplied a *fraction* against
`text_style_height` — which returns the font's natural height (~1.2× size, ≈19px
for 16pt), **not** the 24px the renderer actually draws body text at (1.5× via
the typography config). That ~5px gap is the leading, all below the baseline, and
it left formulas ~1px low however the fraction was tuned. Reading the metrics
directly (`line_height − font_ascent = 24.0 − 14.9 = 9.1px`) removes the guess
entirely. Verified on a dense paragraph: `≥0`, `w=−1`, the fraction, `w_DE`,
`c_s²`, `ρ_DE` all sit on the text baseline.

Lesson: column-bottom *histograms* and eyeballing can't resolve a ~1px offset, so
they led to several wrong "aligned" calls. The trustworthy path was computing the
position from first principles (egui's own glyph metrics) rather than measuring
the rendered result.

## Key discoveries

- **typst `$ x $` (spaces) = block, `$x$` (no spaces) = inline.** Block
  equations have no inline baseline (`has_baseline=false`); inline ones do. This
  was the single most important realization — it fixed both sizing and the
  baseline source.
- **A glyph run's position in a typst frame *is* its baseline.** When the frame
  doesn't expose a baseline, averaging the largest-font runs' y recovers it
  (and gives the axis for fractions).
- **`text_style_height` is the line height, not the font size.** With 1.5× line
  height the leading sits partly below the baseline, so the descent fraction
  (`adjust`) is ~0.25, not the ~0.16 a naive font-descent estimate gives. This
  is why early values read "too low".
- **Pixel measurement at ±2px is noisy.** Column-bottom *histograms* (a dominant
  peak = the shared baseline; secondary low peaks = descenders, not misaligned
  formulas) are far more reliable than per-cluster bottom edges, which the tight
  inline spacing makes unsplittable.

## Verification

Live on the real doc (`MODEL.md`): the dense §1 line (`≥0`, `≤0`, `w=−1`,
`min(1+w)=+1.3×10⁻¹¹`, `max(1+w)=−6×10⁻¹²`) shows a single dominant column-bottom
peak at the text baseline — formula glyphs and text share the line, with
sub/superscripts and relation symbols (math-axis) positioned correctly around
it. Display equations sit on their own centered lines.
