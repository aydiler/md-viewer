# Feature: Outline Scroll Position Fix

**Status:** ✅ Complete
**Branch:** `feature/outline-scroll-fix`
**Date:** 2026-01-22

## Summary

Fixed inaccurate scroll positioning when clicking outline headers. The previous implementation used a line-number ratio which was wildly inaccurate because markdown rendering has variable heights for different content types.

## Solution

Track actual header positions during rendering in egui_commonmark, then use those positions for scrolling.

## Key Changes

### egui_commonmark_backend/src/misc.rs
- Added `header_positions: HashMap<String, f32>` to `CommonMarkCache`
- Added `current_scroll_offset: f32` for coordinate conversion
- Added methods: `set_scroll_offset()`, `record_header_position()`, `get_header_position()`, `clear_header_positions()`

### egui_commonmark/src/parsers/pulldown.rs
- Added `current_heading_y: Option<f32>` and `current_heading_text: String` to `CommonMarkViewerInternal`
- Record header position before spacing in `Tag::Heading` handler
- Accumulate header text during text events
- Save position to cache in `TagEnd::Heading` handler

### src/main.rs (tab architecture)
- Changed `clicked_header_line` to `clicked_header_title` in `render_tab_content()`
- Look up actual position from `tab.cache.get_header_position()` instead of line-ratio calculation
- Apply 25px offset for proper visual alignment
- Call `tab.cache.set_scroll_offset(viewport.min.y)` before rendering

## Key Discoveries

### ui.cursor() is viewport-relative inside show_viewport
The `ui.cursor().top()` inside a `ScrollArea::show_viewport` callback returns viewport-relative coordinates, not content-relative. Must add `viewport.min.y` (scroll offset) to get content position.

### Only record positions once
Recording positions on every render causes jumping because the viewport-relative position changes as you scroll. Solution: only record on first encounter using `contains_key()` check.

### Headers outside viewport aren't rendered
Due to `show_viewport` optimization, headers below the initial viewport aren't rendered until scrolled into view. Positions are recorded progressively as user scrolls through document.

### Visual alignment offset needed
Raw position puts header exactly at viewport top edge. Subtracting 25px provides better visual alignment with some breathing room above the header.

## Files Changed

- `crates/egui_commonmark/egui_commonmark_backend/src/misc.rs`
- `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`
- `src/main.rs`

## Testing

1. Open a long markdown file with multiple headers
2. Scroll through entire document once (to record all positions)
3. Click any header in outline - should scroll to show header near top of viewport
4. Click same header again - should not jump (consistent position)
