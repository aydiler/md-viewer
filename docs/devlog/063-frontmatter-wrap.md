# 063 — Frontmatter values wrap instead of clipping

**Branch:** `fix/frontmatter-wrap`
**Date:** 2026-09-05
**Status:** complete

## Problem

A frontmatter value longer than its column was cut off mid-word, with no
scrollbar and no way to read the rest. Found while taking README screenshots
for 0.2.0, at a 900 px window: `abstract:` ended at "…has to go some".

This is a defect in #128 (the frontmatter table itself), not an inherited one.

## Cause

`render_frontmatter_table` painted the value with `ui.label(value)` inside an
`egui::Grid`. Three things combine:

- a plain `Label` does not wrap,
- a Grid column grows to its content,
- the surrounding frame's `set_max_width` then clips whatever ran past it.

So the column grew past the frame and the frame cut it.

## Fix

`ui.add(egui::Label::new(value).wrap())`. One line. The frame's max width
already bounds the column — the label just was not being wrapped against it.

An earlier attempt also computed a value-column width from the widest key.
That turned out to be unnecessary once the label wrapped, and was removed
rather than left in as dead scaffolding.

## Key discovery: the first version of the test was blind

The regression test passed **before and after** the fix. It proved nothing.

`CommonMarkOptions::render_frontmatter` defaults to `false`, and it gates both
*parsing* and *rendering* — `latex_delimiters::parse_events` only enables
pulldown-cmark's metadata-block option when it is set. The test helper
(`render_geometry`) never enabled it, so the `---` block was parsed as an
ordinary paragraph. Ordinary paragraphs wrap on their own, hence green on a
clipping build.

Two things came out of this:

1. `render_geometry_frontmatter` — a helper variant that turns the option on.
2. An assertion *inside* the test that the frontmatter table was actually
   rendered (`painted` contains the key), so a future change that drops the
   option fails loudly instead of passing meaninglessly.

The only reason this was caught is that the fix was temporarily reverted and
the test re-run. A test that has never been observed red is not evidence.

**Control matrix:**

| build | result |
|---|---|
| without fix (`ui.label`) | FAIL — `value should wrap across rows, got 1` |
| with fix (`Label::wrap`) | PASS |

Note the clip-rect assertion is not the one that discriminates; the row-count
assertion is. Kept anyway, it costs nothing.

## Files

- `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`
- `crates/egui_commonmark/egui_commonmark/tests/wrapping.rs`
