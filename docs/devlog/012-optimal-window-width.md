# Feature: Optimal Initial Window Width

**Status:** ✅ Complete
**Branch:** `feature/optimal-width`
**Date:** 2026-01-22
**Lines Changed:** TBD

## Summary

Set the initial window size to the optimal width needed to show text content at its designed width without pagination, with sidebars at their default widths.

## Features

- [x] Define constants for optimal panel widths
- [x] Calculate initial window size based on panel visibility
- [x] Set viewport size using calculated optimal dimensions

## Key Discoveries

### Width Components

Based on existing code:
- **Content**: 600px default width (optimal for 55-75 CPL readability)
- **File Explorer**: 200px default, 150px min, 300px max
- **Outline Sidebar**: 200px default, 120px min, 400px max
- **Separators/margins**: ~20px (panel borders + scrollbars)

### Calculation

Optimal width = Explorer (if shown) + Content + Outline (if shown) + margins

| Configuration | Width |
|---------------|-------|
| Content only | 600 + 20 = 620px |
| Content + Outline | 600 + 200 + 30 = 830px |
| Content + Explorer | 200 + 600 + 30 = 830px |
| All panels | 200 + 600 + 200 + 40 = 1040px |

## Architecture

### Constants (new)

```rust
// Optimal widths for initial window sizing
const CONTENT_OPTIMAL_WIDTH: f32 = 600.0;
const EXPLORER_DEFAULT_WIDTH: f32 = 200.0;
const OUTLINE_DEFAULT_WIDTH: f32 = 200.0;
const PANEL_MARGINS: f32 = 20.0;  // Separators, scrollbars, borders
```

## Testing Notes

- Test with various persisted states (explorer on/off, outline on/off)
- Verify window doesn't exceed screen bounds
- Verify content doesn't scroll horizontally at startup

## Future Improvements

- [ ] Consider screen DPI/scaling factors
- [ ] Cap maximum initial size to screen dimensions
- [ ] Remember user-resized window size in persisted state
