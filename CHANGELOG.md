# Changelog

All notable changes to markdown-viewer will be documented in this file.

## [0.1.11] - 2026-05-24

### Bug Fixes

- Wide tables no longer get nudged sideways by ordinary page scrolling (#22, PR #23, contributed by [@aki1ro](https://github.com/aki1ro)). The post-render `forward_wheel_to_horizontal_scroll` helper introduced for #4 redirected any hovered-table `smooth_scroll_delta.y` into the inner `ScrollArea::horizontal` X offset. The intent was that wheel-over-table would scroll the table sideways without users having to grab the bottom scrollbar; the cost was that whenever the cursor merely crossed a wide table during normal document scrolling, the table jumped left/right. Edge pass-through reduced but did not remove the surprise. Fix: remove the helper and both call sites entirely; plain wheel input stays with the outer document scroller.

### Features

- Shift+wheel over a wide table now scrolls it horizontally (PR #24). Restores the #4 ergonomic that PR #23 had to drop, but gated on the Shift modifier so it can't collide with normal page scrolling. New helper `forward_shift_wheel_to_horizontal_scroll` in `crates/egui_commonmark/.../pulldown.rs` mirrors the prior helper's edge-passthrough logic and only acts when `ui.ctx().input(|i| i.modifiers.shift)` is true. Shift+wheel for horizontal scroll matches the browser convention; Ctrl was already taken by zoom. Documented in `docs/devlog/033-table-shift-wheel.md`.

## [0.1.10] - 2026-05-24

### Bug Fixes

- Search-active scroll lock (#19, PR #21). With the find bar open, wheel-scrolling away from the active match snapped the view back every frame, leaving the user "locked" near the result; Esc to dismiss the bar was the only workaround. Root cause: the post-render corrective scroll in `render_tab_content` was designed as stage 2 of a two-stage scroll (line-ratio estimate → snap to renderer-recorded `active_search_y`). After the disable-virtualization change in this release, the renderer started walking the full event stream every paint, and `record_active_search_y_viewport` fires unconditionally per Active highlight segment (egui's clip rect culls painting but not widget layout). So `active_search_y` became perpetually fresh, and the corrective block — which had no guard for "user just scrolled" — re-fired every frame the match was off-screen. Fix: one-shot `correct_active_search_pending: bool` on `Tab`, set by `scroll_to_active_match` and cleared after the corrective block runs once. Two-stage scroll still converges in 1–2 frames; subsequent wheel input is no longer overridden. Detail in `docs/devlog/031-search-scroll-lock.md`.
- Nested-list rendering crash + scroll lag (4b13e25). Long docs with deeply nested lists could panic in `delayed_events_list_item` because that helper stopped at the first `TagEnd::Item` regardless of nesting depth, leaking inner-list events into the outer `show()` loop where they were registered as split-points. Fix: depth-track nested items/lists, return only when the outer item closes. Also addressed a math-feature parser-options mismatch between `show()` and `show_scrollable()` that produced inconsistent event streams whenever the doc contained `$…$` (currency, env vars, regex).
- Outline-click scroll precision regression (#1, #2; 6eb8001). Click-on-far-heading landed each header progressively further below the viewport top on layout-changed builds. Root cause: `record_header_content_y_if_absent` pinned the first paint's value, which was computed before async font fallbacks settled. Fix: drop the `_if_absent` semantic so every paint refreshes the recorded y.
- Scroll jitter on slow CPUs (9f02fdc). On T470-class machines, scrolling image-heavy 3 800-line docs felt extremely janky for ~30 s before settling. Root cause: `compute_layout_signature` hashed raw `f32.to_bits()` of widths and font heights; sub-pixel jitter from async image/font loading flipped the hash every frame and forced ~32 full re-bootstraps. Fix: quantize the float inputs (pixel for width, 0.1 px for font heights). Bootstraps during a 30 s scroll: 32 → 1.
- Async-load content shift staleness (d356cc9, 809b761). When images finished loading mid-session, stored split-point y-coords went stale and viewport-skip painted wrong content. First attempt (bucket the previous content_h into layout_signature) entered a perpetual bootstrap loop because the two paint paths report content_h differing by ~44 px. Final fix: absolute-drift hysteresis with a 1 024 px threshold — only invalidate when content height shifts by more than the egui-internal oscillation amplitude.
- Deep-scroll rendering regression revert (5fb3b71). The content-y conversion attempted in 4b13e25 fixed outline-click precision but broke deep-scroll rendering in `full_width_content=true` mode on docs with mixed tables/code blocks. Reverted to screen-y storage; outline-click instead uses the `pending_scroll_offset` non-clear pattern (see entry above).

### Performance / Stability

- Disabled `show_scrollable` virtualization in favor of always-bootstrap (21d43c5). The skip-paint virtualization had three independent unfixable-in-band bugs (orphan `Start` events, `content_size.y` inflation, container-state mid-slice). Symptoms were flicker, blank patches, wrong code-block indentation during scroll. Trade-off: docs ≤ ~3 k events stay smooth, ≤ ~10 k borderline, ≥ ~20 k laggy. Acceptable for typical personal use. A re-virtualization design is tracked in `docs/devlog/030-skip-paint-investigation.md`.

### Internal / CI

- Release pipeline hardened (044d872). `validate` job runs upfront so syntax/typo errors fail fast instead of after the long matrix build. Step-level secret gating for the optional `publish-aur` / `publish-aur-bin` / `publish-snap` / `publish-crates` jobs (GitHub Actions blocks `secrets.*` in job-level `if:`, so the pattern is a first step that writes `proceed=true|false` to `$GITHUB_OUTPUT` and every subsequent step gates on it). MCP-strip transform anchors at start-of-line so it doesn't rewrite commented lines and trigger `cargo publish`'s dirty-tree check (9b59101 adds `--allow-dirty` as belt-and-suspenders for local-MCP testers who forget to re-comment).

## [0.1.9] - 2026-05-16

### Internal / CI

- Restored crates.io auto-publish that was removed in PR #11. Fork crates publish under `_extended` renamed identifiers (no upstream conflict) with feature parity vs the registry; publish order is backend → macros → extended → md-viewer with a 45 s sparse-index settle delay between hops. "Already uploaded" treated as success → idempotent on re-tags.

## [0.1.8] - 2026-05-16

### Packaging

- New `md-viewer-bin` AUR package ships the prebuilt linux-x86_64 binary from GitHub Releases instead of compiling from source. `yay -S md-viewer-bin` is a ~5 s install (vs ~2-3 min compile via `md-viewer-git`), no Rust toolchain required. The two packages `conflict` with each other; pacman picks one. PKGBUILD pulls the `.desktop`, icon, and `LICENSE` from raw GitHub URLs pinned to the tagged commit since the release tarball is binary-only. CI: new `publish-aur-bin` job in `release.yml` mirrors `publish-aur` but rewrites both `pkgver=` *and* the four-element `sha256sums=( ... )` array on every tag. Same `AUR_SSH_PRIVATE_KEY` secret powers both publish jobs.

## [0.1.7] - 2026-05-16

### Bug Fixes

- Outline-click on duplicate-titled headers (#17). Two `## Installation` sections used to both resolve to the same y because `CommonMarkCache::header_positions` is keyed by lowercased title; the second occurrence's `insert()` clobbered the first. Fix: composite key `(normalized_title, nth_with_same_text)` rendered as `"installation"` for the 0th occurrence and `"installation#N"` for the Nth duplicate. Parser assigns the index, renderer mirrors it under the same scheme. Includes a corrective two-stage scroll (`pending_header_click_key`) modeled on the existing search-jump corrective so the bootstrap full paint's precise y wins over the line-ratio first-frame estimate.
- Bootstrap branch in `show_scrollable` corrupted recorded positions when triggered by a non-zero `pending_scroll_offset` (search-jump, outline-click landing deep in doc). Root cause: `cache.set_scroll_offset(0.0)` was unconditional, but the inner `.show()` runs inside a ScrollArea that has already been scrolled to the pending offset, so `ui.cursor().top()` is viewport-relative. Every `record_header_position` / `record_active_search_y_viewport` got shifted by the negative scroll offset, then the corrective scroll snapped to those wrong values. Fix: pass `pending_scroll_offset.unwrap_or(0.0)` instead. This is the missing piece that makes the duplicate-headers disambiguation work end-to-end.

### Features

- Click-to-enlarge lightbox now works for regular markdown images too, not just mermaid diagrams (#17). `![alt](url)` images get `Sense::click()` + a `cache.clicked_image` slot that the main app consumes alongside `take_clicked_mermaid` to open the existing lightbox overlay. Pointer cursor on hover, X close button, escape closes.

## [0.1.6] - 2026-05-16

### Features

- Full-width content toggle (#16, contributed by [@aki1ro](https://github.com/aki1ro)). New `View → Full Width` menu item flips between the default 600 px reading-cap (optimal line length per Dyson & Haselgrove 2001) and using the full available content pane. Persisted to `~/.local/share/md-viewer/app.ron` as `full_width_content: bool` so the choice survives restarts. Default remains capped.

### Bug Fixes

- Wide table horizontal scroll now responds to mouse wheel over the table body (#15, closes the second half of #4). egui 0.33's `ScrollArea::horizontal()` only consumes the X delta of `smooth_scroll_delta` and plain wheel only emits Y, so without intervention the page scrolled instead of the table — users had to drag the bottom scrollbar. The vendored fork now calls a post-render `forward_wheel_to_horizontal_scroll` that redirects Y delta into the inner area's X offset when the cursor is hovered, with edge pass-through (at left/right boundary the delta falls back through to the outer vertical area so the page can still scroll past the table).

### Documentation

- README + all 7 screenshots refreshed for v0.1.5 visuals (new `screenshots/search.png` and `screenshots/resizable-tables.png`, plus refreshed `dark-mode.png` / `light-mode.png` / `syntax-highlighting*.png` / `tables.png`). Features section now mentions search (Ctrl+F), resizable table columns, and viewport virtualization; new Search keyboard-shortcuts table.
- New `docs/devlog/022-table-wheel-scroll.md` and `docs/devlog/023-full-width-toggle.md`.

### Internal

- All View menu items now register with the MCP bridge under `Menu: View → …` names with state-value tags (`"on"`/`"off"`, `"dark"`/`"light"`). The View menu button itself is registered as `Menu: View`. This closes the previously-documented "menus aren't in AccessKit" coverage gap — future E2E tests can drive theme/sidebar/zoom/full-width toggles through `egui_click` instead of state-file injection.

## [0.1.5] - 2026-05-16

### Features

- Resizable table columns (closes the column-width side of issue #4). Both markdown `|...|` tables and HTML `<table>` blocks now render via `egui_extras::TableBuilder` instead of `egui::Grid`. Drag the divider between any two columns to resize; cells re-wrap their content to fit. Striping and the outer border are preserved. Wide tables exceeding the panel width get a horizontal scrollbar instead of clipping. Long inline-code paths inside cells wrap to multiple visual lines per row (per-row height auto-computed). See `docs/devlog/021-table-columns-resizable.md` for the verification matrix and the known edge case (tables with many narrow columns at ≤800 px windows can drop right-side columns).

### Performance

- End-to-end virtualization of the markdown renderer. Scroll frame time at 100 k lines drops from ~101 ms to below the 1-tick measurement floor (effectively 60+ FPS); first-paint settle on a 100 k-line / 6 MB doc drops from ~15 s to ~7 s. Achieved via the vendored `egui_commonmark` fork: dense `split_points` at every block-level event end (root cause of the upstream "buggy in scenarios more complex than the example" warning on `show_scrollable`), binary-search viewport range over split_points, parsed-events cache keyed by a per-`Tab` `content_version`, `layout_signature` invalidation that includes zoom and theme (not just width). The app's `render_tab_content` switches to the renderer-owned `ScrollArea` via the new `CommonMarkViewer::show_scrollable` builder that returns `ScrollAreaOutput<()>` so the selection-preserving wheel hack still works.
- Lazy syntect highlighting. `CodeBlock::end` now hits a `(content, lang, theme, font_size)`-keyed `LayoutJob` cache before running syntect, so only visible code blocks pay the highlight cost on first paint and re-highlight is a hash-lookup after that.
- Outline panel virtualized via `egui::ScrollArea::show_rows`. On a 100 k-line doc with ~15 k headers the outline cost drops from O(headers) to O(visible_rows).

### Bug Fixes

- Search-jump and outline-click on off-viewport targets no longer leave the viewport at the line-ratio estimate. When `pending_scroll_offset` is set, the renderer forces a one-frame full paint so `cache.active_search_y` / `header_position` get recorded; the two-stage corrective scroll then snaps precisely. Cost: one ~100 ms frame per jump action (steady-state scroll is unaffected).

### Documentation

- New `docs/devlog/020-virtualize-large-docs.md` with the implementation walk, perf measurements, and the full MCP test pass (T-A through T-J: outline click, wheel scroll, search, zoom, theme, multi-tab isolation, file-explorer click, live reload, outline fold, selection-during-scroll).
- New `docs/devlog/021-table-columns-resizable.md` for the TableBuilder refactor and verification matrix.
- New `docs/LESSONS.md` entries covering virtualization gotchas (sparse split_points, layout_signature scope, selection-preserving wheel hack needs `ScrollAreaOutput`, lazy-syntect cache key) and TableBuilder gotchas (fixed row heights clip multi-line cells, outer `ScrollArea::horizontal` required, header/body Y alignment needs `ui.vertical()`).

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

