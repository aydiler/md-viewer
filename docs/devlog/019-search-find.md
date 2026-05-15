# Feature: Search (Ctrl+F) — Current Doc with Inline Highlights

**Status:** ✅ Complete
**Branch:** `feature/search`
**Date:** 2026-05-15
**Lines Changed:** ~900 across `src/main.rs`, vendored `crates/egui_commonmark/`, `docs/KEYBOARD_SHORTCUTS.md`, `docs/ARCHITECTURE.md`, `docs/LESSONS.md`

## Summary

Implements `Ctrl+F` search over the active tab's content. Matches are highlighted inline by extending the vendored `egui_commonmark` renderer with a search-range API on `CommonMarkCache`. `Enter` / `Shift+Enter` cycle through matches and scroll the active one into view via the existing line-ratio mechanism. `Esc` closes the bar and clears highlights.

Scope is deliberately narrow: current document only, case-insensitive plain substring, no toggles. Cross-tab find-all, regex, whole-word, and a case toggle are deferred to v2.

## Features

- [x] `SearchState` on `MarkdownApp` and `search_matches: Vec<SearchMatch>` on `Tab`
- [x] Pure `find_matches(content, query)` with 13 unit tests (case-insens ASCII, line numbers, UTF-8 byte offsets, cross-newline skip, overlapping, image-alt filter, image-URL filter, link-URL filter, link-text kept, multiple-image cases)
- [x] Image alt-text + image/link URLs excluded from matches (non-renderable bytes)
- [x] `CommonMarkCache::set_search_ranges` / `set_active_search_range` / `clear_search_ranges`
- [x] Renderer y-recording: `cache.record_active_search_y_viewport` + `cache.active_search_y()`
- [x] Two-stage scroll: line-ratio estimate first, exact-y correction next frame
- [x] Inline highlight rendering in `pulldown.rs` `Event::Text` + non-wrapped `Event::Code`
- [x] `render_search_bar` conditional TopBottomPanel between error_bar and tab_bar
- [x] Keyboard: Ctrl+F open, Enter/Shift+Enter cycle, ArrowDown/ArrowUp cycle, Esc close
- [x] On-screen `↑` / `↓` prev/next buttons in the bar (disabled when no matches)
- [x] `File → Find...` menu entry (discoverability)
- [x] `jump_match` + `scroll_to_active_match` extracted as a reusable helper
- [x] `maybe_rebuild_search` resets active_match_index to 0 AND calls scroll_to_active_match (matches Firefox/Chrome/VS Code on query change or tab switch)
- [x] File-watcher reload rebuilds matches on next frame (invalidates `last_tab` shadow)
- [x] `close_search` clears highlight state on every tab
- [x] MCP widget registrations on every interactive search-bar widget; `Match Count` value matches the rendered label
- [x] `docs/KEYBOARD_SHORTCUTS.md` updated (Search section with all 4 navigation paths)
- [x] `docs/ARCHITECTURE.md` updated
- [x] `docs/LESSONS.md` updated with byte-offset filtering and two-stage scroll patterns

## Key Discoveries

### `to_ascii_lowercase` preserves byte offsets; `to_lowercase` does NOT

`find_matches` returns byte ranges into the original content (consumed by the renderer
as `Range<usize>` keys). For case-insensitive matching, the cheap-and-correct trick is
`str::to_ascii_lowercase` on both sides: it only rewrites bytes A–Z and leaves byte
length unchanged. `str::to_lowercase` does proper Unicode case folding which can
*change byte length* (e.g. `"İ".to_lowercase() == "i̇"` — adds a combining mark), so any
match offsets computed against the folded string would be wrong against the original.

Documented v1 limitation: `É` does not match `é`. The case-toggle and full Unicode case
folding are listed in Future Improvements.

```rust
let content_lc = content.to_ascii_lowercase();
let query_lc = query.to_ascii_lowercase();
for (byte_start, _) in content_lc.match_indices(&query_lc) { /* offsets valid */ }
```

### Inline highlight via `RichText::background_color` (not painter overlay)

Highlights are inline in the renderer, not overlay-painted. The vendored `pulldown.rs`
already iterates `pulldown_cmark::Event`s with their source `Range<usize>` spans —
exactly what's needed to map a search-match byte range back to a sub-segment of a
`Text` event. The implementation:

1. `CommonMarkCache::set_search_ranges(Vec<Range<usize>>)` stores ranges before render
2. In `Event::Text`, `event_text_with_highlights` checks if `text.len() == span.len()`
   (markdown escapes / smart-punct transforms would break the 1:1 assumption) and
   either splits into segments or falls back to plain rendering
3. Each segment emits a `RichText` via `emit_text(text, hl, ...)` which applies
   `rich_text.background_color(...)` when `hl != HighlightKind::None`
4. Active match gets a stronger color variant

No painter overlay = no misalignment under reflow. The cost: doesn't highlight inside
syntax-highlighted code blocks (those use `syntect` post-collection — would need a
deeper integration).

### Inline-code highlighting: derive interior span from `src_span.len() - text.len()`

pulldown_cmark's `Event::Code` gives the code text without backticks, but the
src_span covers the whole token including delimiters. To highlight inline code,
the renderer computes `delim_total = src_span.len() - text.len()` and, when the
result is even, treats it as `delim_total / 2` backticks on each side and emits
an "interior span" `(src_span.start + bt)..(src_span.end - bt)` for highlighting.

For wrapped inline code (the `inline_code_wrap_segments` 56-char chunks), v1 skips
highlighting since sub-spans per segment aren't trivially derivable.

### Heading path must `accumulate`, not call `ui.label` directly

LESSONS.md "Inline code in headers" already documented this: heading rendering
accumulates `RichText` into `current_heading_rich_texts` and renders them at
`end_tag(Heading)` inside a single `allocate_ui_at_rect` for left alignment. The
new `emit_text` correctly routes RichText to that accumulator (same as existing
`event_text`), so search highlights inside headings work without special-casing.

### File-watcher reload invalidates the rebuild shadow state

`Tab::reload` clears `search_matches`, but `SearchState::last_query` and `last_tab`
remain unchanged — so `maybe_rebuild_search` would skip rebuild on the next frame
("nothing changed from its perspective"). Fix: `reload_changed_tabs` sets
`self.search.last_tab = None` when the active tab is among the reloaded ones,
forcing the next frame to rebuild against the new content.

### MCP bridge has no keyboard-shortcut injection; egui MenuBar absent from AccessKit

Xvfb has no window manager, so `xdotool key --window <id> ctrl+f` doesn't route
keypresses to a focused widget. The egui MCP bridge initially only exposed
`egui_click` / `egui_type` (no keyboard shortcuts) — and egui's `MenuBar` widget
doesn't surface its sub-buttons in the AccessKit tree, so a `File → Find...` click
via MCP also doesn't work.

**Fix shipped in a sibling repo:** added an `egui_key` tool to `egui-mcp` that
synthesizes `egui::Event::Key { key, pressed, modifiers }` via the bridge's
`inject_raw_input` hook. Includes a critical fix: when synthesizing key events,
the bridge must also OR the modifiers into `raw_input.modifiers` because egui
derives `Input.modifiers` from `RawInput.modifiers`, not from the per-event
modifiers field. Without that, `i.key_pressed(F)` fires but `i.modifiers.ctrl`
stays false. This unblocks E2E testing for every egui app, not just md-viewer.

### Search matches must skip non-renderable markdown spans

After the first PR-ready build, the user reported "active highlight stuck on
first results" and "auto-scroll wrong position". Root cause: `find_matches`
counted bytes inside `![alt](url)` and `[text](url)` as matches even though the
alt and URL portions aren't visibly rendered. Cycling to an alt-text match
painted the active highlight at invisible bytes and scrolled to the markdown
source line of the image (which is where the image renders, not where the matched
text would be).

Fix: regex `(!?)\[([^\]]*)\]\(([^)]*)\)` and skip matches inside:
- Group 3 (URL) — always
- Group 2 (alt/text) — only when group 1 is `!` (image)
For "syntax" against README.md: 10 raw matches → 8 after alt filter → 6 after URL filter.

### Line-ratio scroll undershoots badly in image-heavy docs — two-stage scroll

Even after filtering invisible matches, "auto-scroll wrong position" persisted
for match 4 ("JSON syntax highlighting" caption past three large images).
Line-ratio (line N / total lines × content height) assumes uniform per-line
height; images consume ~400 px each but only one "line" of source, so the
estimate undershoots by ~1000+ px for matches past several images.

Fix: two-stage scroll.
1. `scroll_to_active_match` sets `pending_scroll_offset` from the line-ratio
   estimate, with `viewport_height * 0.35` margin. Gets the view in the right
   neighbourhood.
2. During render, when `event_text_with_highlights` emits an Active segment,
   `cache.record_active_search_y_viewport(ui.cursor().top())` records the
   actual content-relative y.
3. After `show_viewport` returns, `render_tab_content` checks
   `cache.active_search_y()`. If the recorded y is outside the visible viewport,
   schedule a corrective `pending_scroll_offset` for the next frame.

Two-frame visual: estimate first, snap precise. All 6 "syntax" matches in
README now land the active match inside the viewport with the brighter
HL_ACTIVE_DARK color clearly visible.

### Debugging discipline: log byte offsets BEFORE chasing renderer hypotheses

Spent ~30 min suspecting `allocate_ui_at_rect` in `end_tag(Heading)` suppressed
`RichText.background_color` paint when active-highlighting a heading. The bug
turned out to be entirely different: the "active" match's byte range pointed
inside image alt-text. The visible heading "Syntax" got the regular (Match)
color because it was a *different* search range — there was no Active span to
paint visibly.

Diagnostic that would have caught it sooner: log `active = Some(byte_start..byte_end)`
plus `content[byte_start - 40 .. byte_end + 40]`. If the context shows
`![Syntax Highlighting](...)`, the match is in alt-text — investigate the
filter, not the renderer.

## Architecture

### New Structs

```rust
struct SearchState {
    is_open: bool,
    query: String,
    last_query: String,        // shadow for change detection
    focus_requested: bool,
    active_match_index: usize,
}

struct SearchMatch {
    byte_start: usize,
    byte_end: usize,
    line_number: usize,        // 1-based
}
```

### New Functions

| Function | Purpose |
|----------|---------|
| `find_matches(&str, &str) -> Vec<SearchMatch>` | Pure scan; case-insensitive substring; 1-based line numbers |
| `Tab::rebuild_search(&str)` | Wrapper calling `find_matches` and storing result |
| `MarkdownApp::render_search_bar(&Context)` | Conditional find-bar panel |
| `MarkdownApp::jump_match(i32)` | Move active_match_index, set scroll target |
| `CommonMarkCache::set_search_ranges(Vec<Range<usize>>)` | Push match byte ranges into renderer |
| `CommonMarkCache::set_active_search_range(Option<Range<usize>>)` | Active match for stronger highlight |
| `CommonMarkCache::clear_search_ranges()` | Reset on close / tab switch |

### Theme-aware colors

```rust
const HL_LIGHT: Color32 = Color32::from_rgb(255, 229, 127);
const HL_DARK:  Color32 = Color32::from_rgb(102, 92, 46);
const HL_ACTIVE_LIGHT: Color32 = Color32::from_rgb(255, 167, 38);
const HL_ACTIVE_DARK:  Color32 = Color32::from_rgb(156, 107, 26);
```

## Testing Notes

**Automated**
- `cargo test --bin md-viewer --no-default-features` — 13/13 tests pass (~5 ms total)
  - empty query / empty content
  - case-insensitive ASCII
  - multiple matches per line
  - 1-based line numbers across multi-line content
  - UTF-8 byte-offset preservation (e.g. `café`)
  - matches that would cross a newline are skipped
  - `match_indices` semantics (non-overlapping): `"aaaa" / "aa" → [0, 2]`
  - image alt-text filter
  - image URL filter
  - link URL filter (with link text kept)
  - multiple-image cases
- `cargo clippy` — no warnings introduced (the two pre-existing vendored-crate
  warnings about `_max_width` and `allocate_ui_at_rect` are unrelated to this PR)

**E2E via egui MCP** (with the new `egui_key` tool from the egui-mcp sibling repo)
- Open find bar (`egui_key {key: "F", modifiers: ["ctrl"]}`)
- Type query (`egui_fill` n3 "syntax") — match count "1 / 6"
- Cycle every match 1→6 via `egui_key {key: "ArrowDown"}` — each verified by:
  - Match Count widget value updates "N / 6"
  - Screenshot captured, pixels sampled for HL_ACTIVE_DARK rgb(156,107,26)
  - Every match has non-zero active-orange pixels inside the visible viewport
- Wraparound 6→1 verified (Enter at last match)
- Backward cycling verified (Shift+Enter from match 5 → 4)
- Esc closes bar, all highlights gone (back to 70 AccessKit nodes)

**Visual confirmation per match for "syntax" against README:**
| # | Location | Visible? |
|---|----------|----------|
| 1 | intro paragraph | ✓ |
| 2 | Features bullet "Syntax Highlighting" | ✓ |
| 3 | h3 heading "Dark Mode -- Syntax Highlighting" | ✓ |
| 4 | italic caption "*JSON syntax highlighting*" past 3 images | ✓ (via two-stage scroll) |
| 5 | Technical Details bullet | ✓ |
| 6 | link description "Syntax highlighting" | ✓ |

**Edge cases verified by code review**
- Smart-punct transforms break `text.len() == span.len()` → falls back to no highlight
- Wrapped inline-code (>56 chars) → no highlight (interior_span is None)
- Tab close while bar is open → next frame's `maybe_rebuild_search` rebuilds for new active tab
- File reload of active tab while bar is open → `last_tab = None` invalidation triggers rebuild
- Empty query → empty match vector → match-count label is empty string
- 0 matches → label shows "0 matches", Enter/Shift+Enter is a no-op

## Future Improvements

- [ ] Cross-tab find-all panel
- [ ] Case-sensitivity toggle
- [ ] Whole-word toggle
- [ ] Regex
- [ ] F3 / Shift+F3 alternates (browser convention; ↑↓ already cover the intuitive case)
- [ ] Per-tab match-count badges in tab bar
- [ ] Search history
- [ ] Renderer y-recording is currently only for the *active* match. Recording all matches would unlock features like a minimap-style scrollbar overlay showing every match's position.
