# Large documents are measured over several frames

**Status:** 🚧 In Progress
**Branch:** `perf/progressive-layout`
**Date:** 2026-11-20

## Summary

`show_scrollable` measured the whole document in the first frame (the bootstrap
pass) before it could paint anything. With #259 and #260 merged, prose is what
remains: 3.7 s to the first content frame for 5 MB, 3.4 s for the 6 MB GitLab
changelog. callgrind on 1 MB of prose shows the time is real text layout, one
layout per paragraph, with nothing to cut.

With `CommonMarkViewer::progressive_measure(true)` (md-viewer turns it on), the
first frame measures only until a time budget has passed and two screens are
covered. Later frames paint the viewport slice as before and measure the rest
in budgeted chunks.

## Features

- [ ] One event loop for the bootstrap and the progressive passes
- [ ] Frontier state on `ScrollableCache`, reset with the split points
- [ ] Frame-1 budget and estimated extent
- [ ] Background continuation in an invisible child
- [ ] Synchronous catch-up when the viewport passes the frontier
- [ ] Deep pending offset and `force_full_render` fall back to the full bootstrap
- [ ] md-viewer opts in
- [ ] Tests: equivalence, first frame, frontier cap, deep scroll, invalidation
- [ ] Measurements

## Key Discoveries

## Architecture

## Testing

## Future Improvements
