# 065 — Bound the frontmatter value column (fixes #128 properly)

**Branch:** `fix/128-frontmatter-bounded`
**Date:** 2026-09-07
**Status:** complete

## History this closes

- **#128** introduced the frontmatter key/value table. A value longer than its
  column was clipped.
- **#166** "fixed" that with a bare `Label::wrap()`. It caused **#167**: every
  block below the table stopped painting at a 1040 px window.
- **#170** reverted #166, restoring the clipping.
- This change fixes the original defect without reintroducing #167.

## The two symptoms, one cause

An unbounded value grows its grid column past the frame. That produces two
distinct failures, and the #128 report only described the first:

1. the value itself is clipped mid-word;
2. the oversized block **widens the content column for the whole document**,
   so the prose of every later block is clipped at the pane edge too.

Symptom 2 was measured on the reverted build at 1200 px: prose read
"md-viewer rende…" and "verbatim, s…". The control that pins it to the
frontmatter block is the same document with the `---` block removed — prose
wraps correctly there at the identical window size.

## Why the bound is on `max_width`, not `available_width`

`max_width` comes from `ContentGeometry` (#96) and is identical in the
bootstrap pass that records `split_points` and the slice pass that paints.
`ui.available_width()` is not. Deriving the value column from the ambient
width makes the block's *height* differ between the two passes, and slice
selection then ends the event range early — that is #167 exactly.

So the value column is `max_width - measured_key_width - KEY_GAP -
item_spacing - FRAME_CHROME`, floored at 80 px, and the value wraps inside a
scope set to it. `FRAME_CHROME` is deliberately generous: over-reserving
narrows the column slightly, under-reserving lets it overflow and clip.

Key width is measured with the real strong rendering:

```rust
egui::WidgetText::from(egui::RichText::new(key).strong())
    .into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY,
                 egui::TextStyle::Body)
    .size().x
```

`into_galley` lives on `WidgetText`, not `RichText` — the direct call is an
`E0599`.

## Verification

**App, five widths** (Xvfb, isolated `XDG_*`, painted pixels below y=380):

| window | painted px |
|---|---|
| 900 | 14752 |
| 1040 | 13956 |
| 1080 | 13956 |
| 1200 | 13956 |
| 1600 | 13956 |

1040 is the width at which #166 collapsed to 2750. Nothing collapses now, and
the constant value across widths is the intended `default_width` cap — a
frontmatter document now lays out exactly like the no-frontmatter control,
whereas the reverted build ran prose to x≈1198 at 1600 because of the
inflation described above.

**Control matrix** — both tests were observed red:

| test | without fix | with fix |
|---|---|---|
| `frontmatter_geometry_ignores_ambient_width` | FAIL — `value should wrap across rows, got 1` | PASS |
| `frontmatter_block_stays_within_the_content_column` | FAIL — `value exceeds the content column: 657.06 > 400` | PASS |

## Two traps hit while writing the tests

Both are the same trap in different clothes: **the fixture sitting outside the
regime the test is meant to probe.**

1. At `content_width = 700` the fixture value very nearly fits on one line, so
   `rows > 1` failed on the *fixed* build and the height assertion would have
   been satisfied trivially. Narrowed to 400.
2. `frontmatter_block_stays_within_the_content_column` initially **passed on
   the broken build** — also because of `CONTENT = 700`, where an unbounded
   column barely overshoots. At 400 it detects the defect and reports the
   overshoot as a number.

The second one is exactly #166's mistake repeated, and it was caught only by
running the control. Both constants now carry a comment saying why they are
what they are.

## Harness change

`render_geometry_inner` takes `ui_width` and `content_width` separately;
`render_geometry_frontmatter` exposes that plus `render_frontmatter(true)`.
The older helpers pass the same value for both, preserving their behaviour.
The coupling is what made #167 inexpressible as a test.

## Files

- `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`
- `crates/egui_commonmark/egui_commonmark/tests/wrapping.rs`
- `docs/LESSONS.md`
