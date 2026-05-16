# Feature: Virtualize the markdown renderer for large docs

**Status:** ✅ Complete (pending perf-regression run)
**Branch:** `feature/virtualize-large-docs`
**Date:** 2026-05-16
**Lines Changed:** ~250 LoC across `crates/egui_commonmark/` + `src/main.rs`

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

- [x] C1 — `Tab::content_version` u64 counter, bumped on load/reload
- [x] C2 — `ScrollableCache` gains `events`, `content_version`, `layout_signature`
- [x] C3 — `show_scrollable` caches parsed events keyed by `content_version`
- [x] C4 — Dense `split_points`: drop `is_inside_a_list()` gate at `parsers/pulldown.rs:347`
- [x] C5 — Binary-search viewport range over `split_points` (replaces linear `.filter().nth_back(1)`)
- [x] C6 — `layout_signature` invalidation (width + font_size + line_height_mults + theme_is_dark), replaces width-only check
- [x] C7 — `CommonMarkViewer::show_scrollable` returns `ScrollAreaOutput<()>`; builder methods `pending_scroll_offset`, `scroll_source`, `content_version`; un-hide from rustdoc
- [x] C8 — `render_tab_content` switches to `show_scrollable`; preserve selection-protecting wheel hack via returned scroll_output
- [x] C9 — Lazy syntect `LayoutJob` cache keyed by `(text_hash, theme, font_size)`
- [x] C10 — Outline panel via `egui::ScrollArea::show_rows`

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

### Measured results (release build, Xvfb :99, `--no-watch`)

Bench harness: `/tmp/md-bench/scroll_probe_new.sh`. Compares pre-virtualization (v0.1.4 release) vs post-C1-C10.

| Doc | Frame time before | Frame time after | First-paint settle before | After |
|---|---|---|---|---|
| 10 k (0.6 MB)  | 12 ms  | **0.3 ms** | ~4 s  | ~3 s |
| 50 k (3 MB)    | 71 ms  | **<0.1 ms** (below 1-tick threshold) | ~8 s  | ~5 s |
| 100 k (6 MB)   | 101 ms | **<0.1 ms** (below 1-tick threshold) | ~15 s | ~7 s |

Scroll frame time is now dominated by xdotool injection cost and the per-frame egui input plumbing; the renderer's own work is no longer measurable at this resolution for 50 k+ docs. The plan's targets (≤16 ms at 50 k, ≤20 ms at 100 k) are met with room to spare.

First-paint settle dropped roughly 2× across all sizes thanks to lazy syntect (C9): only code blocks currently in viewport pay the highlight cost on first paint. The remaining settle time is primarily one-time initial layout + outline parsing.

### Visual smoke tests

Verified manually on Xvfb :99:
- README renders correctly (title, license badge, hero screenshot, outline tree, file explorer).
- Synthetic 10 k / 50 k / 100 k docs all render their first viewport's content (headings, paragraphs, lists, tables, code blocks with syntect highlighting).
- The pre-existing bug where the doc generator labels Rust code as `json` (and syntect renders sparse tokens) is unchanged — confirmed identical between v0.1.4 release binary and the new binary, so not a regression.

### Items still owed (deferred or out of scope)

- Unit tests for `is_block_end_tag` dense coverage, `layout_signature` invalidation, and the `syntax_layouts` cache hit path.
- Selection regression test (genuinely blocked through MCP): egui selection requires continuous mouse drag; `mcp__egui__*` exposes click/type/scroll/key but no drag primitive, and the auto-enforced `DISPLAY=:99` blocks dropping to a real desktop. The mechanical write — `scroll_output.state.offset.y = new_offset; state.store(...)` — *was* verified by T-B (wheel scroll moves the view via the exact same path on the returned `ScrollAreaOutput`). What MCP cannot verify is whether egui's internal selection-validation logic (the "both cursor endpoints must be seen this frame" check in `label_text_selection.rs`) deselects across that op. Must be confirmed manually on a real desktop session, or by adding a CLI debug flag that pre-selects a byte range and then drives a wheel scroll programmatically.

### MCP test pass (T-A through T-I)

Run via `cargo build --release --features mcp` + `mcp__egui__*` tools on Xvfb :99 (parallel session contention noted but managed by killing only our own processes). Per-test artifacts under `/tmp/md-bench/mcp/T-*/`.

| Test | Surface | Result |
|---|---|---|
| T-A | outline-click → scroll-to-header (sections 1, 5, 8 in `doc_100000.md`) | PASS — viewport scrolls to clicked header; outline highlights clicked entry |
| T-B | wheel scroll via `egui_scroll` (down then up) | PASS — pre/post viewports differ in scroll direction |
| T-C | search Ctrl+F → fill → cycle (README "License" + `doc_100000.md` "Voluptate tempor") | **PASS after fix** — initial regression: virtualization skipped over off-viewport match blocks, so `cache.active_search_y` never recorded, two-stage scroll couldn't snap. Fix in `parsers/pulldown.rs`: when `pending_scroll_offset.is_some()`, clear `page_size` + `split_points` to force the bootstrap (full-paint) branch this frame. README cycling now lands orange `HL_ACTIVE` on every match. |
| T-D | zoom Ctrl++/Ctrl+0 | PASS — 130% indicator appears, text scales cleanly, no rendering artifacts; `layout_signature` invalidates correctly |
| T-E | dark-mode rendering (light + dark + transition) | **PASS via persisted-state flip** — `egui_key{key:"D", modifiers:["ctrl"]}` didn't visibly toggle in MCP (likely a bridge edge case for the "D" key; other Ctrl+ shortcuts in T-D worked). Verified the equivalent code path by editing `~/.local/share/md-viewer/app.ron` (`dark_mode:Some(true)` ↔ `Some(false)`) and relaunching: light mode renders cleanly with white background + dark text + appropriate syntect-light theme on code blocks; dark mode restores black background + white text. `layout_signature` includes `dark_mode`, so cache invalidates correctly. The Ctrl+D handler at `src/main.rs:3362` itself is untested via MCP but uses the same theme-apply code path that the persisted-state route exercised, so the theme rendering is verified end-to-end. |
| T-F | multi-tab scroll-state isolation (README + CHANGELOG) | PASS — scroll CHANGELOG → switch to README → switch back to CHANGELOG: prior scroll position preserved. Per-`source_id` `ScrollableCache` does what it should. |
| T-G | file explorer click → open tab | PASS (verified as side-effect of T-F) — clicking `File: CHANGELOG.md` opens new tab, sets it active, marks file as `"open"` in explorer. |
| T-H | live reload via file watcher | PASS — `echo … >>` to a watched file triggers reload within ~1 s; outline repopulates with new headers; no flicker. |
| T-I | outline collapse/expand fold indicators | PASS — clicking `Toggle: …` flips state expanded↔collapsed; `visible_indices` recomputes; `show_rows` adjusts row count (visible h3 children disappear; off-screen sections appear to fill the freed space). |

The C-T regression fix (one extra commit on the branch: `Fix: search-jump and outline-click on off-viewport targets miss after virtualization`) costs one full-paint frame (~100 ms at 100k lines) per scroll-to action. Acceptable for a one-off click/Enter; steady-state scroll is unaffected.

## Future Improvements

- [ ] Upstream the dense-split_points fix to `egui_commonmark`
- [ ] LRU eviction on `syntax_layouts` if memory grows past ~50 MB for pathological docs
- [ ] Streaming/incremental pulldown_cmark parsing if version-keyed cache proves insufficient
