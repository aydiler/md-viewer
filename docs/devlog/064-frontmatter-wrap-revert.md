# 064 — Revert the frontmatter wrap fix (#166), which caused #167

**Branch:** `fix/167-frontmatter-slice`
**Date:** 2026-09-07
**Status:** revert complete; proper fix pending

## What happened

#166 changed one line in `render_frontmatter_table`:

```rust
ui.label(value)  ->  ui.add(egui::Label::new(value).wrap())
```

It fixed a real defect (a long frontmatter value clipped mid-word) and shipped
with a regression test that had been verified to fail without it.

It also caused #167: at a 1040 px window, **every block below the frontmatter
table stops painting**. The outline still lists later headings and the scroll
thumb still reports a long document; the pane is simply blank.

## Why this is a revert and not a forward fix

The bug #166 fixed is cosmetic and local — one value is cut off. The bug it
introduced loses all content below the block. `main` is the 0.2.0 release
candidate, so it goes back to the smaller, older defect while the real fix is
developed without release pressure. #128's clipping is reopened by this revert.

## Mechanism

`split_points` record each block's start/end y in the bootstrap pass. The slice
path selects its event range with

```rust
let below = split_points.partition_point(|(_, start, _)| start.y <= viewport.max.y);
let last_event_index = split_points.get(below + 1).map(|(i, _, _)| *i)...;
```

`.wrap()` made the block's height depend on the **ambient** `Ui` width, and the
bootstrap pass's ambient width differs from the paint pass's. At 1040 px the
block was recorded 1103 px taller than it painted, pushing every later split
point past the viewport bottom, so `below` collapsed to 1 and the range ended
at event 11 of 63.

Measured with a temporary `MDV_DIAG_SPLIT=1` probe:

| build | window | `sp[0] end.y` | `below` | `last_ev` | content |
|---|---|---|---|---|---|
| with #166 | 1040 | 1336 | 1 | 11 | missing |
| with #166 | 1080 | 236 | 6 | 63 | complete |
| #166 reverted | 1040 | 214 | 6 | 63 | complete |

Deterministic — at 1040 the last of 203 frames equals the first, so the values
never converge.

`MDV_DIAG_SLICE=1` reported zero off-screen slice placements in **both** the
working and the broken run. That true negative is what excluded the placement
hypothesis and redirected the search to range selection.

## The direction for the real fix

#166 deleted a computation that bounded the value column against the widest key,
judging it unnecessary once the label wrapped. That bound is precisely what made
the block's height a function of the passed `max_width` — which `ContentGeometry`
(#96) guarantees is identical across both passes — instead of ambient width.

So the fix is to wrap against a width derived from `max_width`, not to wrap
against whatever the current `Ui` offers. It needs a regression test that varies
the ambient width while holding `max_width` fixed, and asserts the block's height
does not move; the existing helper couples the two, so the harness needs
extending first.

## Files

- `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`
- `crates/egui_commonmark/egui_commonmark/tests/wrapping.rs`
- `docs/LESSONS.md`
