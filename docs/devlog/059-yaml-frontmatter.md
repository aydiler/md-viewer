# Feature: YAML frontmatter rendering

**Status:** ✅ Complete
**Branch:** `feature/yaml-frontmatter`
**Date:** 2026-09-04
**Issue:** #117

## Summary

Documents that open with a `---` delimited YAML frontmatter block currently render it as content: a thematic break, the raw `key: value` lines as a paragraph, another thematic break. Reported in #117 as "any doc using it renders with a broken-looking block at the top".

Renders it as a two-column key/value table instead, which is what VS Code does and what the reporter asked for.

## Why the parser never saw it as frontmatter

`pulldown-cmark` supports this via `Options::ENABLE_YAML_STYLE_METADATA_BLOCKS`, and the renderer already carries `Tag::MetadataBlock` / `TagEnd::MetadataBlock` arms — but they are empty no-ops, and `parser_options()` never set the flag. So the events were never produced, and the block fell through to ordinary block parsing.

Two things therefore had to change together: enable the parser option, and give the metadata arms something to do. Enabling the option alone would make frontmatter silently vanish.

## Design

Opt-in, following the `use_strong_font_family` precedent in this codebase: default `false` so existing library consumers of `egui_commonmark_extended` see no change, enabled explicitly by md-viewer.

- `CommonMarkOptions::render_frontmatter: bool`
- `CommonMarkViewer::render_frontmatter(bool)` builder
- threaded into the parse path next to the existing `math_enabled` flag

The flag has to reach the *parser*, not just the renderer, which is why it travels through `latex_delimiters::parse_events`.

## Key discoveries

### egui's `Grid` does not turn `spacing.x` into a column gap

`Grid::spacing(vec2(x, y))` visibly changed row spacing but left the columns
touching: a key as long as the column's widest entry rendered flush against its
value — `authorJane Doe`. The gap has to be produced *inside* the key cell:

```rust
ui.horizontal(|ui| {
    ui.label(RichText::new(key).strong());
    ui.add_space(12.0);
});
```

Only visible by looking at a screenshot. The unit tests for the parsing were
green throughout, because the defect is in layout, not in the split.

### A full `/tmp` looks exactly like a broken change

Ten doctests failed with `error: linking with x86_64-linux-gnu-gcc failed`
during this work. The real message was several screens down:
`LLVM ERROR: IO failure on output stream: Disk quota exceeded`. Doctests link
into `/tmp`, which is a 16 GB tmpfs here, and debug builds of this project are
~800 MB each — a handful of them copied aside for A/B comparison fills it.

Two tells that it was not the change: every failure was in a method the branch
never touched, and the failure count differed between runs (10, then 5). A code
defect does not fluctuate.

## Future improvements

- Nested mappings and sequences fold into the parent value
  (`nested: key: value other: thing`). Matching VS Code, and no source text is
  dropped, but a document with deep frontmatter reads poorly. Rendering nested
  levels would need a real YAML parser and a nested display.
- TOML frontmatter (`+++`) is not handled. `pulldown-cmark` has
  `ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS` for it; the renderer side would be
  identical, only the parser flag differs.
- The block is always shown when enabled. A collapsible header would suit long
  frontmatter, at the cost of persisted per-document UI state.
