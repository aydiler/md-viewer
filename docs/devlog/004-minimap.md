# Feature: VS Code-Style Minimap

**Status:** ✅ Complete
**Branch:** `feature/minimap`
**Date:** 2026-01-22
**Lines Changed:** +200 in `src/main.rs`

## Summary

Implemented a VS Code-style minimap as a right sidebar showing a scaled-down overview of the document. Uses colored blocks (not character rendering) to represent different content types. Includes click-to-navigate support with viewport indicator.

## Features

- [x] Right-side panel showing document overview
- [x] Colored blocks representing content types (headers, code, lists, text)
- [x] Current viewport indicator (semi-transparent rectangle with border)
- [x] Click-to-navigate anywhere in document
- [x] Drag-to-scrub for continuous navigation
- [x] Toggle visibility with Ctrl+M
- [x] Persist show_minimap state
- [ ] Hover-to-preview position (future enhancement)

## Design Decisions

### Colored Blocks vs Character Rendering

Chose colored blocks because:
1. Markdown has natural visual structure (headings, code blocks, lists)
2. Character rendering at minimap scale provides no useful information
3. Significantly better performance (no text layout calculation)
4. Visual patterns become clearer for navigation

### Line Color Mapping

| Content Type | Color (Light Mode) | Color (Dark Mode) |
|-------------|-------------------|-------------------|
| H1 | #2563EB (bright blue) | #60A5FA |
| H2 | #3B82F6 (blue) | #93C5FD |
| H3-H6 | #6B7280 (gray) | #9CA3AF |
| Code block | #374151 (dark gray) | #4B5563 |
| List item | #10B981 (green) | #34D399 |
| Blockquote | #8B5CF6 (purple) | #A78BFA |
| Regular text | #D1D5DB (light gray) | #6B7280 |

### Block Width Variation

Different block types use different widths for visual distinction:
- Headers: full width (72px)
- Text: near-full (68px)
- Blockquotes: medium (64px)
- Code blocks: indented (60px)
- List items: most indented (56px)

## Key Discoveries

### egui::Painter API
Use `ui.allocate_painter(size, sense)` to get a painter and response tuple. The painter draws in screen coordinates - use the returned rect's position directly.

### Proportional Scroll Synchronization
Map positions using ratio: `position_ratio = scroll_offset / total_content_height`. The minimap height maps to content height, so clicking at ratio `r` should scroll to `r * content_height`.

### Click vs Drag Handling
Use `Sense::click_and_drag()` to enable both click-to-jump and drag-to-scrub behaviors. Check `response.clicked() || response.dragged()` and use `response.interact_pointer_pos()` for the current position.

### egui 0.33 API Changes
- `screen_rect()` deprecated - use `available_rect()` or `content_rect()`
- `rect_stroke()` now requires a 4th argument: `StrokeKind` (Inside, Outside, Middle)

## Architecture

### New Types

```rust
enum BlockType {
    Header(u8),    // level 1-6
    CodeBlock,
    ListItem,
    Blockquote,
    Text,
    Blank,
}

struct MinimapBlock {
    block_type: BlockType,
    start_line: usize,
    line_count: usize,
}
```

### New Fields in MarkdownApp

```rust
show_minimap: bool,
minimap_blocks: Vec<MinimapBlock>,
minimap_pending_scroll: Option<f32>,
```

### New Functions

| Function | Purpose |
|----------|---------|
| `parse_minimap_blocks()` | Analyze markdown content into typed blocks |
| `block_color()` | Get color for block type based on theme |

## Testing Notes

- Tested with LESSONS.md (~200 lines) - works well
- Tested with sample markdown - visual structure clear
- Click-to-navigate functional
- Viewport indicator properly tracks scroll position
- Ctrl+M toggle works
- State persists across sessions

## Future Improvements

- [ ] Hover preview showing line number
- [ ] Highlight current section in minimap
- [ ] Search result indicators (yellow markers)
- [ ] Smooth scroll animation on click
- [ ] Resizable minimap width
