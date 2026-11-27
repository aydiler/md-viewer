# 130: Find-bar jumps without laying out the whole document

**Status:** 🚧 In Progress
**Branch:** `perf/search-jump-measure`
**Date:** 2026-11-27

## Summary

Every find-bar step laid out the whole document in one frame: opening the bar
or typing a character (through `maybe_rebuild_search`), Enter, Shift+Enter and a
tab switch all call `scroll_to_active_match`, which set a line-ratio offset and
granted the next frame `force_full_render`. `show_scrollable` then dropped the
measured state and measured everything again, even when the document was
already fully measured. Devlog 115 removed the same full layout from heading
jumps and listed the search jump as its follow-up.

## Measured before (main, release build, Xvfb, 3 runs)

A probe (local patch, not committed) opens the find bar at frame 30 with a token
that occurs at about 50 % and 95 % of the document, jumps to the next match at
frame 90, and again at frame 150, which wraps back to the first match in the
already-measured part. It prints each frame's `ui()` time.

| document | open the bar | jump ahead | jump back (measured part) |
|---|---|---|---|
| prose 1 MB | 355–359 ms | 376–410 ms | 354–382 ms |
| prose 5 MB | 1872–1887 ms | 1821–1880 ms | 1852–1905 ms |

Each is one frame. With the find bar open, every typed character costs the
same. A 5 MB fixture built by repeating a 1 MB text five times measured only
~0.5 s: egui's galley cache lays a repeated paragraph out once, so large-document
fixtures need unique text.

## Changes

- [ ] Renderer: `CommonMarkCache::measured_block_top`
- [ ] Renderer: a layout change forgets the active match's position
- [ ] App: `PendingSearchJump` and `resolve_search_jump` replace the one-shot
      correction flag and `force_full_render`
- [ ] Tests and mutations
- [ ] Measured after
