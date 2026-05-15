# Changelog

All notable changes to markdown-viewer will be documented in this file.

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

