# Changelog

All notable changes to markdown-viewer will be documented in this file.

## [0.1.5] - 2026-05-16

### Performance

- End-to-end virtualization of the markdown renderer. Scroll frame time at 100 k lines drops from ~101 ms to below the 1-tick measurement floor (effectively 60+ FPS); first-paint settle on a 100 k-line / 6 MB doc drops from ~15 s to ~7 s. Achieved via the vendored `egui_commonmark` fork: dense `split_points` at every block-level event end (root cause of the upstream "buggy in scenarios more complex than the example" warning on `show_scrollable`), binary-search viewport range over split_points, parsed-events cache keyed by a per-`Tab` `content_version`, `layout_signature` invalidation that includes zoom and theme (not just width). The app's `render_tab_content` switches to the renderer-owned `ScrollArea` via the new `CommonMarkViewer::show_scrollable` builder that returns `ScrollAreaOutput<()>` so the selection-preserving wheel hack still works.
- Lazy syntect highlighting. `CodeBlock::end` now hits a `(content, lang, theme, font_size)`-keyed `LayoutJob` cache before running syntect, so only visible code blocks pay the highlight cost on first paint and re-highlight is a hash-lookup after that.
- Outline panel virtualized via `egui::ScrollArea::show_rows`. On a 100 k-line doc with ~15 k headers the outline cost drops from O(headers) to O(visible_rows).

### Bug Fixes

- Search-jump and outline-click on off-viewport targets no longer leave the viewport at the line-ratio estimate. When `pending_scroll_offset` is set, the renderer forces a one-frame full paint so `cache.active_search_y` / `header_position` get recorded; the two-stage corrective scroll then snaps precisely. Cost: one ~100 ms frame per jump action (steady-state scroll is unaffected).

### Documentation

- New `docs/devlog/020-virtualize-large-docs.md` with the implementation walk, perf measurements, and the full MCP test pass (T-A through T-J: outline click, wheel scroll, search, zoom, theme, multi-tab isolation, file-explorer click, live reload, outline fold, selection-during-scroll).
- New `docs/LESSONS.md` entries covering the virtualization gotchas: why `show_scrollable` was upstream-tagged "buggy" (sparse split_points), `layout_signature` must include zoom + theme, the renderer's selection-preserving wheel hack needs `ScrollAreaOutput`, and the lazy-syntect cache-key composition.

## [0.1.4] - 2026-05-15

### Features

- Search (Ctrl+F) with inline highlights and precise scroll-to-match (#14, closes #4). Find bar above the tab bar; case-insensitive matches in the active tab get an inline yellow highlight, the active match gets a brighter orange. Enter / Shift+Enter / ↑ / ↓ cycle matches; Esc closes the bar. Matches inside image alt-text and image/link URLs are skipped so cycling only lands on visibly-rendered text. Two-stage scroll lands the active match in viewport even in image-heavy documents.

### Bug Fixes

- Wide inline-code tokens (long file paths, fully-qualified identifiers) overflowed the content column at narrow widths and overlapped adjacent text at wide widths. Long tokens are now split into fixed-size chunks separated by row breaks (#5).

### Documentation

- Document snap `--destructive-mode` glibc trap (Ubuntu 22.04 compatibility), inline-code wrap segmentation choice, and the open feature-request priority order in LESSONS.md and TARGET_METRICS.md.

### Miscellaneous

- Replace placeholder app icon with a generated document icon.
- Tighten Flatpak `finish-args` for Flathub linter; prep Flatpak manifest for Flathub submission.

## [0.1.3] - 2026-05-15

### Bug Fixes

- Resolve clippy warnings for CI
- Prevent dollar amounts from rendering as math formulas

### Features

- Add LaTeX math rendering via typst + mitex

### Performance

- Eliminate per-frame allocations, syscalls, and re-parsing

### Styling

- Apply rustfmt formatting
## [0.1.2] - 2026-03-04

### Bug Fixes

- File watcher recovery when watcher fails to start
- Properly apply underline and color to markdown links
- Bring link underline closer to text by removing extra line height
- Enable HTTP image loading and add DejaVu Sans font fallback
- Directory expansion now works with single click
- Canonicalize tab paths for consistent file watcher matching
- Re-check expansion state after click for immediate child rendering
- Toggle directory expansion after row render for immediate children
- Load children for expanded directories on session restore
- Use floor_char_boundary for outline header truncation
- Resolve relative image paths against markdown file directory
- Improve HTML table readability with vertical separators and cell padding
- Use unique IDs for code block horizontal ScrollAreas
- Truncate long file paths in menu bar to prevent overlap with buttons
- Re-enable content zoom (disabled during MCP testing)

### Documentation

- Update README screenshots with nav arrows
- Update README with latest features and refresh screenshots
- Document SVG text rendering requirements and limitations
- Update README with new features and refresh screenshots

### Features

- Add context menu to copy file contents from explorer
- Increase link visibility with underline and hyperlink color
- Add system font fallbacks for Unicode support
- Add navigation buttons and virtual display CPU fix
- Lazy load file explorer directories on expand
- Enable SVG text rendering for shields.io badges
- Enable file watching by default
- Add middle-click to close tabs from file explorer
- Add file explorer sorting options
- Render HTML tables as grids instead of raw text
- Add mermaid diagram rendering support
- Switch mermaid renderer from mermaid-rs-renderer to merman
- Add mermaid diagram click-to-enlarge lightbox
- Async mermaid rendering + lightbox zoom-to-cursor

### Miscellaneous

- Add snap artifacts to gitignore, update deps
- Upgrade merman from 0.1 to 0.3
- Bump version to 0.1.2

### Performance

- Fix file explorer O(n×m) scan and lazy-load session restore
- Eliminate idle CPU usage with event-driven file watcher repaints
- Disable egui memory persistence and clear stale data on startup

### Styling

- Apply rustfmt to font fallback tuples
## [0.1.1] - 2026-01-30

### Bug Fixes

- **ci:** Correct rust-toolchain action name
- **ci:** Correct Ubuntu package names for libxcb
- **ci:** Remove local-only MCP dependency before build
- **ci:** Fix clippy warnings and MCP feature handling
- **ci:** Fix sed order to preserve mcp feature
- **ci:** Use cargo test instead of cargo test --lib
- **release:** Use rust plugin for snap, allow-dirty for crates.io

### Miscellaneous

- Release v0.1.1

### Styling

- Apply cargo fmt

### Ci

- Add GitHub Actions for CI and releases

