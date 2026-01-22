# Feature: Outline Expand/Collapse All Buttons

**Status:** ✅ Complete
**Branch:** `feature/outline-expand-collapse`
**Date:** 2026-01-22
**Lines Changed:** +14 in `src/main.rs`

## Summary

Add "Expand All" and "Collapse All" buttons to the outline sidebar for quickly showing or hiding all nested headers.

## Features

- [x] Add Expand All button to outline header
- [x] Add Collapse All button to outline header
- [x] Only show buttons when there are headers with children

## Key Discoveries

### Reusing existing infrastructure
The collapse state (`collapsed_headers: HashSet<usize>`) and helper functions (`header_has_children`, `any_header_has_children`) were already implemented for per-header collapse. The expand/collapse all buttons just manipulate this existing state:
- **Expand All**: `collapsed_headers.clear()`
- **Collapse All**: Insert all header indices that have children

## Architecture

### Modified Functions

| Function | Change |
|----------|--------|
| `render_tab_content()` | Added buttons in outline sidebar header |

## Testing Notes

Test with documents that have:
- Nested headers (h2 -> h3 -> h4)
- Flat headers (all same level)
- Mix of headers with and without children

## Future Improvements

- [ ] Keyboard shortcuts for expand/collapse all
- [ ] Remember collapse state in persisted settings
