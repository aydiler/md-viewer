# Feature: Virtualize the markdown renderer for large docs

**Status:** 🚧 In Progress
**Branch:** `feature/virtualize-large-docs`
**Date:** 2026-05-16
**Lines Changed:** TBD

## Summary

End-to-end virtualization for the markdown viewer so large docs (50 k–100 k lines) stay at 60 FPS during scroll and become interactive within ~2 s of opening. Today the renderer walks every pulldown event on every frame (the outer `ScrollArea::show_viewport` in `render_tab_content` provides scroll bars but the inner `CommonMarkViewer::show()` ignores viewport), and syntect re-highlights all code blocks on every frame.

Measured baseline before the change (release build, Xvfb :99, `--no-watch`):

| Doc | Scroll frame time | First-paint settle |
|---|---|---|
| 10 k lines (0.6 MB) | 12 ms (~80 FPS) | ~4 s |
| 50 k lines (3 MB) | **71 ms (~14 FPS)** | ~8 s |
| 100 k lines (6 MB) | **101 ms (~10 FPS)** | ~15 s |

Bench harness lives at `/tmp/md-bench/` (`pulldown_bench/`, `scroll_probe.sh`, `doc_{10000,50000,100000}.md`).

## Features

- [ ] C1 — `Tab::content_version` u64 counter, bumped on load/reload
- [ ] C2 — `ScrollableCache` gains `events`, `content_version`, `layout_signature`
- [ ] C3 — `show_scrollable` caches parsed events keyed by `content_version`
- [ ] C4 — Dense `split_points`: drop `is_inside_a_list()` gate at `parsers/pulldown.rs:347`
- [ ] C5 — Binary-search viewport range over `split_points` (replaces linear `.filter().nth_back(1)`)
- [ ] C6 — `layout_signature` invalidation (width + font_size + line_height_mults + theme_is_dark), replaces width-only check
- [ ] C7 — `CommonMarkViewer::show_scrollable` returns `ScrollAreaOutput<()>`; builder methods `pending_scroll_offset`, `scroll_source`, `content_version`; un-hide from rustdoc
- [ ] C8 — `render_tab_content` switches to `show_scrollable`; preserve selection-protecting wheel hack via returned scroll_output
- [ ] C9 — Lazy syntect `LayoutJob` cache keyed by `(text_hash, theme, font_size)`
- [ ] C10 — Outline panel via `egui::ScrollArea::show_rows`

## Key Discoveries

### "show_scrollable is buggy" → root cause is sparse split_points

The `#[doc(hidden)] // Buggy in scenarios more complex than the example application` warning on `show_scrollable` traces to a single gate at `parsers/pulldown.rs:347`:

```rust
let should_add_split_point = self.list.is_inside_a_list() && is_element_end;
```

Split points are *only* populated for events inside lists. A doc that's mostly headings + paragraphs + code blocks produces no split points, and the viewport-skip math falls back to `Pos2::ZERO` → first_end_position = ZERO → content overlap. The bundled example happens to be a uniform list-only doc, so the bug stays hidden.

### Naive swap to `show_scrollable` would *regress* perf

`show()` has a content-hash event cache (`pulldown.rs:318-327`). `show_scrollable` doesn't — it re-parses on every frame (`pulldown.rs:410-413`). Switching paths without also wiring the cache into `show_scrollable` would make scroll *worse* on large docs (~52 ms parse vs 11 ms clone today).

### `process_event` is shared, so custom features come along for free

Search highlights (`event_text_with_highlights`), header-position recording (`record_header_position`), link hooks, and the inline-code wrap segmenter all live inside `process_event`. Since `show_scrollable` calls `process_event` for every visible event (`pulldown.rs:465`), all four features keep working unchanged — we don't need to touch the fork's custom code surface.

### Text-selection preservation needs the ScrollAreaOutput

The existing wheel-during-selection hack (`src/main.rs:2475-2496`, documented in `docs/LESSONS.md`) reads `scroll_output.state.offset.y`, modifies it post-render, and `state.store()`s it back. `show_scrollable` today wraps the ScrollArea internally and returns nothing. The C7 change makes it return `ScrollAreaOutput<()>` so the app can keep the hack.

## Architecture

### Modified Structs

```rust
// egui_commonmark_backend/src/pulldown.rs
pub struct ScrollableCache {
    pub available_size: Vec2,
    pub page_size: Option<Vec2>,
    pub split_points: Vec<(usize, Pos2, Pos2)>,
    // new:
    pub events: Vec<(pulldown_cmark::Event<'static>, Range<usize>)>,
    pub content_version: u64,
    pub layout_signature: u64,
}
```

```rust
// src/main.rs
struct Tab {
    // existing fields
    content_version: u64,  // new: bumped on every load_file / reload
}
```

### New / changed API

| Function | Purpose |
|----------|---------|
| `CommonMarkViewer::content_version(u64)` | Caller-provided version to key the per-document cache |
| `CommonMarkViewer::pending_scroll_offset(Option<f32>)` | Replaces outer `ScrollArea::vertical_scroll_offset` |
| `CommonMarkViewer::scroll_source(ScrollSource)` | Lets the app retain its drag-disabled wheel-only config |
| `CommonMarkViewer::show_scrollable(...) -> ScrollAreaOutput<()>` | Returns scroll state for post-render selection hack |
| `CommonMarkCache::syntax_layouts: HashMap<u64, LayoutJob>` | Lazy syntect cache, keyed by `(text_hash, theme, font_size)` |

## Testing Notes

End-to-end verification plan in `/home/ahmet/.claude/plans/do-that-declarative-sonnet.md`. Quick reference:

- Re-run `/tmp/md-bench/scroll_probe.sh` against 10 k / 50 k / 100 k. Targets: scroll ≤16 ms at 50 k, ≤20 ms at 100 k; first-paint ≤2 s.
- egui MCP on Xvfb :99: outline click → screenshot diff; wheel scroll responsiveness.
- Selection regression: manual on real desktop (Xvfb's selection model is degenerate).
- Unit tests for dense split_points, layout_signature invalidation, lazy syntect cache.

## Future Improvements

- [ ] Upstream the dense-split_points fix to `egui_commonmark`
- [ ] LRU eviction on `syntax_layouts` if memory grows past ~50 MB for pathological docs
- [ ] Streaming/incremental pulldown_cmark parsing if version-keyed cache proves insufficient
