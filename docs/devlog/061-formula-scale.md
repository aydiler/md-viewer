# Feature: formula scale relative to body typography

**Status:** ✅ Complete
**Branch:** `feature/123-formula-scale`
**Date:** 2026-09-04
**Issue:** #123

## Summary

The math renderer compiled every formula with a hard-coded Typst setting:

```typst
#set text(size: 16pt, fill: black)
```

The application Body style is also 16 logical points, so this was not an
accidental downscale — but it is a fixed coupling. Math glyphs and dense
fractions read as perceptually smaller than surrounding prose, and the only
existing remedy is application zoom, which enlarges the entire UI rather than
the formulas relative to the text.

Adds a scale relative to the resolved Body font size.

## The five places the size has to reach

Taken from the issue, and each one is a real failure if missed:

1. **The Typst source** — otherwise nothing changes visually.
2. **The math cache key** — otherwise the first-rendered size is served for
   every later scale, and changing the setting appears to do nothing.
3. **Cached inline height/baseline** (`cached_inline_math_height`) — it
   recomputes the same hash independently, so it must agree with the render
   path or it looks up a key that was never stored.
4. **The scroll/layout invalidation signature** — otherwise `split_points`
   keep positions measured at the old formula size.
5. **Table row-height measurement** — `table_cell_height` asks
   `cached_inline_math_height` how tall an inline formula is; a stale answer
   clips the formula inside its row.

## Key discoveries

### The cache key has to quantize the size

The point size derives from the resolved Body font height, which fluctuates in
its last float digits between frames — the same behaviour that forced
`compute_layout_signature` to quantize (see LESSONS.md, "layout_signature must
quantize floats"). Hashing the raw `f32` would produce a fresh key almost every
paint, and every miss **recompiles the formula through typst on a worker
thread**. Quantized to 0.01 pt: finer than anything the eye resolves, coarser
than the jitter. A test pins it.

### Two code paths compute the same key independently

`render_math_with_layout` stores under a key it computes; `cached_inline_math_height`
looks up a key it computes separately. If they disagree by a rounding step, the
lookup misses a key that was just stored — and the failure is silent: the row
height falls back to a conservative guess and the formula is clipped. Both now
go through one `math_size_pt` helper, which exists for exactly that reason.

### The size has to travel to the worker thread

Rendering happens off-thread and the worker has no `Ui` to ask for a font
height, so the resolved size rides along in `MathJob` next to the colours.

## Verification

Measured block heights, same document, 100 % vs 150 %:

| block | 100 % | 150 % |
|---|---|---|
| heading | 26 px | 26 px |
| paragraph with inline formulas | 20 px | **31 px** |
| plain text line | 17 px | 17 px |
| display formula | 38 px | **61 px** |
| body text after | 17 px | 17 px |

Formulas grow, prose does not — which is the whole point, and what application
zoom cannot do.

## Future improvements

- Inline and display formulas share one scale. The issue notes they may
  eventually want separate policies; one shared scale was the smaller first
  step and nothing here blocks splitting it later.
- The menu offers fixed steps (100/110/125/150 %). A continuous slider would
  need throttling — every distinct value is a fresh typst compile per formula.
