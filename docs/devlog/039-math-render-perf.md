# 039 — Math rendering performance + inline baseline alignment

**Branch:** `feature/math-rendering`
**Date:** 2026-06-08
**Status:** Implemented, verified live (frame-by-frame settle timing + screenshots)

## Problem

A math-heavy doc (`MODEL.md`, 405 formulas) took **~30 s** to render, showing
`Rendering formula…` placeholders for ~13 s before *anything* appeared, then
rippling down the page. Separately, every inline formula sat visibly **below**
the surrounding text baseline.

## Where the time actually went (measured, per formula)

Instrumented `render_math_formula` with per-phase timers on the real doc:

| phase | avg | note |
|---|---|---|
| mitex convert | 0.2 ms | negligible |
| **engine build** | 16.8 ms | rebuilt per formula |
| **typst compile** | 74.4 ms | dominant |
| **rasterize + composite** | 92.6 ms | per-pixel `powf` loop; scales with image area |
| **one-time font load** | **~12.5 s** | blocked the *first* formulas |

So two distinct costs: a one-time ~12.5 s font load, and ~187 ms/formula
steady-state (× 405, serialized one-at-a-time = minutes).

## Fixes

### 1. Font loading: embedded fonts only (kills the ~12.5 s)
The original `render_math_formula` built a fresh engine per formula with
`search_fonts_with(Default::default())` — a full **system-font disk scan**
(~3.1 s/formula). The first cut cached the scan once (`MATH_FONTS: Vec<Font>`),
but force-loading *every* system font still cost ~12.5 s upfront — for fonts no
formula renders with.

Final approach: load **only typst's embedded default fonts** (New Computer
Modern + NCM Math + Libertinus Serif + DejaVu Sans Mono) via
`typst-kit`'s `embed-fonts` feature, `include_system_fonts(false)`. These are
compiled into the binary and load from memory in milliseconds. Math formulas
only ever use these fonts, so nothing is lost — and NCM Math is the canonical
LaTeX-style math font, so formulas actually look **crisper** than with whatever
system font previously matched.

```toml
typst-kit = { version = "0.14", default-features = false,
              features = ["fonts", "embed-fonts"] }
```
```rust
static MATH_FONTS: LazyLock<Vec<typst::text::Font>> = LazyLock::new(|| {
    let mut s = typst_kit::fonts::Fonts::searcher();
    s.include_system_fonts(false).include_embedded_fonts(true);
    s.search().fonts.iter().filter_map(|f| f.get()).collect()
});
```

### 2. Parallel rendering (was one-at-a-time)
`math_rendering: Option<u64>` (a single in-flight hash) → `HashSet<u64>`, spawn
while `len() < math_concurrency()` (= `min(cores, 6)`). N formulas render
concurrently. typst compile is CPU-bound, so this scales with cores.

### 3. Event-driven repaint (was throttled to 100 ms)
The render loop only advanced on the 100 ms placeholder repaint tick, so
throughput was capped at *slots per 100 ms* regardless of how fast formulas
actually rendered. Now: when a render completes, `request_repaint()` immediately
so the next formula spawns at once.

### 4. Alpha-boost LUT (rasterize composite)
The per-pixel composite ran `a.powf(0.6)` + 3 lerps for every pixel — the single
biggest per-formula cost. Alpha is a `u8` (256 values), so precompute the
composited color for every alpha once into a `[Color32; 256]` LUT; the per-pixel
loop becomes a single array index.

### 5. Inline baseline alignment
Inline content is laid out `Align::BOTTOM`, so a tall formula image bottom-aligned
to the line — sitting well below the text baseline (line-height leading) and
looking "shifted down". Fix: allocate the slot, then `paint_at` the image lifted
by `0.30 × body_text_height` so it rides the text baseline. (Calibrated against a
controlled test doc.)

### Bonus: `warm_math_fonts()` hook
Re-exported through `egui_commonmark`; force-loads `MATH_FONTS` from a background
thread. Now cheap (embedded fonts), so largely moot, but available.

## Results (MODEL.md, 405 formulas, 4-core dev box, Xvfb)

| metric | before | after |
|---|---|---|
| first formulas appear | ~13 s | **~2 s** |
| **visible viewport fully settled** | ~30 s | **~3 s** (frame-diff measured) |
| per-formula font search | 3.1 s | 0 (embedded, once) |
| upfront font load | ~12.5 s | ~ms |

Verified live: launched MODEL.md on Xvfb, sampled every 1 s — viewport
pixel-stable by t=3 s, formulas (display equation, `min(1+w)=+1.3×10⁻¹¹`, etc.)
fully rendered, crisp NCM glyphs, inline math on the baseline. No tofu across
§1/§2/§8/§9/§11/§12 (Greek, `≥ ≤ ≈ ∼ ⟺ ∝ → χ² □`, fractions, sub/superscripts).

## Future (not done)

- The whole 405-formula document still renders fully (virtualization is disabled,
  per devlog 030), so off-screen formulas churn in the background after the
  viewport settles. A viewport-gated render queue (only render formulas near the
  visible area) would cut total work, but the viewport already settles in ~3 s so
  it's not user-visible.
- Reuse one shared engine across formulas (warm comemo, no per-formula 17 ms
  build) — needs a `FileResolver` + `Sync` engine shared across render threads.
  Saves ~17 ms/formula; modest next to the wins above.
