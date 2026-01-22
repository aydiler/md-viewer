# Fix: Outline Header Click Scrolling

**Status:** ✅ Complete
**Branch:** `feature/fix-outline-scroll`
**Date:** 2026-01-22
**Lines Changed:** TBD

## Summary

Fixed outline header clicks not working for headers that haven't been rendered yet. The issue occurs when clicking outline headers that are below the fold - the app uses viewport-based rendering for performance, so only visible headers have their positions recorded. When `get_header_position()` returns `None`, no scroll happens.

**Root Cause:** Header positions are only recorded during rendering via `cache.record_header_position()`. Unrendered headers (below viewport) have no position data.

## Fix Checklist

- [x] Add fallback scroll estimation using line numbers
- [x] Use line numbers from Header struct (field was already present but unused)
- [x] Test with long documents where headers are off-screen
- [x] Verify smooth scrolling to both rendered and unrendered headers

## Key Discoveries

### Viewport-based rendering doesn't record all header positions

**Problem:** egui's `show_viewport` only renders content visible in the viewport for performance. Headers below the fold haven't been rendered yet, so `cache.record_header_position()` was never called for them. When clicking outline entries for these headers, `get_header_position()` returns `None` and no scroll happens.

**Solution:** Implement fallback estimation using line number ratios:

```rust
if let Some(y_pos) = tab.cache.get_header_position(&header.title) {
    // Use exact position if available (header has been rendered)
    tab.pending_scroll_offset = Some((y_pos - 50.0).max(0.0));
} else if tab.last_content_height > 0.0 && tab.content_lines > 0 {
    // Fallback: estimate position based on line number ratio
    let estimated_y = (header.line_number as f32 / tab.content_lines as f32)
        * tab.last_content_height;
    tab.pending_scroll_offset = Some((estimated_y - 50.0).max(0.0));
}
```

This provides approximate scrolling that gets the header into view, after which the exact position is recorded for future clicks.

## Architecture

### Modified Code

**Changed:** `render_tab_content()` in `src/main.rs`

- Changed `clicked_header_title: Option<String>` to `clicked_header_index: Option<usize>` to track which header was clicked (gives access to both title and line_number)
- Modified header click scroll calculation to:
  1. First try `cache.get_header_position()` for exact position (headers that have been rendered)
  2. Fall back to line number ratio estimation for unrendered headers: `(line_number / content_lines) * last_content_height`

**Why two-tier approach?**
- Exact positions are accurate but only available after rendering
- Line ratio estimation works immediately but is approximate (assumes uniform line heights)
- Together they provide: instant approximate scrolling → rendering → future clicks use exact position

## Testing Notes

**Test case:** `/home/ahmet/discord-phone-bridge/worktrees/main/docs/LESSONS.md`
- File with headers spread across ~95 lines
- Previously: Clicking outline headers did nothing (they were below viewport)
- After fix: Clicking scrolls to approximate position using line number estimation

**Verified with egui MCP:**
1. Scrolled to top (clicked "Lessons Learned")
2. Clicked "Add your lessons below as you discover them" in outline (unrendered header at bottom)
3. ✅ Scrolled directly to the header using fallback estimation

**Edge cases handled:**
- Empty content (`content_lines = 0`): Fallback skipped, no crash
- No content height yet (`last_content_height = 0.0`): Fallback skipped, waits for first render
- Already rendered headers: Uses exact position, no approximation needed

**Known limitation:** Line ratio estimation assumes uniform line heights. Works well for typical markdown but may be slightly off for documents with large code blocks or many images. This is acceptable since: (a) it's better than nothing, and (b) exact position is used after first scroll brings it into view.

## Future Improvements

- [ ] Potential enhancement 1
- [ ] Potential enhancement 2
