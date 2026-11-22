# Architecture

Single-file Rust desktop application (`src/main.rs`, ~1300 lines) for viewing markdown files using egui + egui_commonmark with a custom tab system.

## Core Components

- **MarkdownApp**: Main struct implementing `eframe::App`. Holds:
  - `tabs: Vec<Tab>` - list of open tabs
  - `active_tab: usize` - index of the currently active tab
  - `dark_mode: bool` - global theme setting
  - `zoom_level: f32` - global zoom (0.5 to 3.0)
  - `show_outline: bool` - toggle outline sidebar visibility
  - `show_tab_bar: bool` - tab bar visibility (#208); the bar still shows while the active tab is in Edit mode (`tab_bar_visible`)
  - `show_explorer: bool` - toggle file explorer visibility
  - `file_explorer: FileExplorer` - file explorer state
  - `watch_enabled: bool` - file watching state
  - `watcher` + `watcher_rx` - file watching via mpsc channel
  - `watched_paths: HashSet<PathBuf>` - unified watcher for all open tabs
  - `hovered_tab: Option<usize>` - for showing close button on hover
  - `recent_files: Vec<RecentEntry>` - recently opened files for the welcome page
  - `welcome_show_all: bool` - whether the welcome page's recent list is expanded

- **Tab**: Per-tab state for a document. Each tab has:
  - `id: egui::Id` - unique identifier
  - `path: PathBuf` - file path
  - `content: String` - markdown text
  - `cache: CommonMarkCache` - **must persist across frames** (never recreate per-frame, only reset on file load)
  - `document_title: Option<String>` - first h1 used as sidebar title
  - `outline_headers: Vec<Header>` - parsed headers for outline
  - `scroll_offset`, `pending_scroll_offset`, `last_content_height`, `last_viewport_height` - scroll state
  - `local_links: Vec<String>` - cached local markdown links
  - `history_back`, `history_forward: Vec<PathBuf>` - per-tab navigation history

- **PersistedState**: Serializable struct for session persistence:
  - `dark_mode: Option<bool>`
  - `zoom_level: Option<f32>`
  - `show_outline: Option<bool>`
  - `show_tab_bar: Option<bool>` - `None` (state written before #208) shows the bar
  - `show_explorer: Option<bool>` - file explorer visibility
  - `explorer_root: Option<PathBuf>` - file explorer root directory
  - `expanded_dirs: Option<Vec<PathBuf>>` - expanded directories in explorer
  - `open_tabs: Option<Vec<PathBuf>>` - restore tabs on startup
  - `active_tab: Option<usize>` - restore active tab position
  - `recent_files: Option<Vec<RecentEntry>>` - recently opened files (welcome page)

- **FileExplorer**: Left sidebar showing markdown files in a directory tree:
  - `root: Option<PathBuf>` - root directory to display
  - `tree: Vec<FileTreeNode>` - hierarchical file tree
  - `expanded_dirs: HashSet<PathBuf>` - which directories are expanded

- **FileTreeNode**: Enum representing a node in the file explorer tree:
  - `File { path, name }` - a markdown file
  - `Directory { path, name, children }` - a directory containing markdown files

- **File Watching**: Uses `notify-debouncer-mini` with 200ms debounce. Watches all open tab paths. On change, reloads matching tabs. Auto-recovers up to 3 times on watcher failure.

- **Header Outline**: `parse_headers()` returns a `ParsedHeaders` struct containing `document_title` (first h1) and `outline_headers` (remaining headers). Rendered as a resizable left sidebar.

- **Link Navigation**: Uses egui_commonmark's link hook mechanism. Ctrl+Click opens links in new tabs, regular click navigates within the current tab.

- **Search (Ctrl+F)**: Current-document find bar with inline highlights. `SearchState` lives on `MarkdownApp`; per-tab `search_matches: Vec<SearchMatch>` cache match byte ranges. Highlights are painted by the vendored `egui_commonmark` renderer via a new `CommonMarkCache::set_search_ranges` API; the renderer splits `Event::Text` and (non-wrapped) `Event::Code` at range boundaries and applies a background color to the matching segments. Enter/Shift+Enter cycle matches with line-ratio scroll-into-view; Esc closes the bar and clears highlights on all tabs.

- **Markdown strong text**: md-viewer registers the renderer's `STRONG_FONT_FAMILY` (`MarkdownStrong`) during `setup_fonts`, then enables `CommonMarkViewer::use_strong_font_family(true)` so `**strong**` spans can use a real bold face while generic `egui_commonmark` consumers remain opt-in.

- **Startup fonts (devlog 100)**: `MarkdownApp::new` installs a base font set (primary sans, its bold, preset mono, egui's bundled faces) via `system_fonts::FontLoader`. `finish_startup_fonts` then checks the active document, tab paths and recent files with `base_covers`. When the base set lacks a character it installs the full set (script, symbol and color-emoji fallbacks) before the first frame; otherwise a `font-fallbacks` thread builds it and `update()` installs it from `pending_full_fonts`, unless the font config changed meanwhile. Runtime family, preset and size changes still go through `setup_fonts`, which builds the full set synchronously.

- **Font picker family list (devlog 109)**: `FontFamilyList` on `MarkdownApp` holds the names the View → Fonts dialog offers. `render_font_settings` calls `request(spawn_font_family_scan)` when the dialog opens, which starts the `font-family-scan` thread (`scan_pickable_font_families`) at most once per run; `update()` collects the result with `poll()`. Nothing scans at startup.

- **Off-screen code blocks (devlog 101)**: `CodeBlock::end` gives a code block that lies wholly outside the clip rect (`code_block_is_offscreen`) a plain layout job and does not cache it; the syntect job is built and cached in `syntax_layouts` the first time the block is painted where it shows. Highlighting only sets colors, so both jobs have the same size and the bootstrap's measurements stay valid.

- **Table cell measurement (devlog 102)**: the column-width optimizer measures cells at many candidate widths. `replay_wrap` answers a cell's row count and last-row width by replaying epaint's line breaking (`line_break`, `RowBreakCandidates`) over the glyphs of the unwrapped galley, which is cached for the frame; it falls back to a full layout for text with a newline, right-to-left text, and labels that start after other content in the row. A change to epaint's line breaking must be mirrored there.

- **One parse per document (devlog 110)**: `CommonMarkViewerInternal::parsed_events` parses the text (or returns the cached stream for an unchanged text hash) and stores it in `CommonMarkCache` as an `Arc` (`ParsedEvents`), together with its right-to-left block starts. `show()` and `show_scrollable` both take the stream from there, so `ScrollableCache::events` is the same allocation and the two cannot diverge (devlog 027). On the app side, `parse_document` feeds one `pulldown_cmark` pass to `HeaderCollector` (outline) and `LinkCollector` (link hooks); `is_local_markdown_link` rejects a destination without a `.` before classifying it.

- **Progressive measurement (devlog 103)**: with `CommonMarkViewer::progressive_measure(true)` (md-viewer sets it), `show_scrollable`'s first frame measures only until two screens are covered and 8 ms have passed, records `ScrollableCache::measure_frontier`, and reports an extent extrapolated by source length. Later frames paint the viewport slice, capped at the frontier, catch up synchronously when the viewport comes within a screen of it, and measure 4 ms more per frame in an invisible child (`measure_from_frontier`) until the stream ends. The child resumes on the bootstrap's own cursor row (`row_top`, `min_rect_bottom`) with the first-block flag cleared, so the final split points, extent and heading positions equal a one-frame bootstrap. A pending offset past the estimate and `force_full_render` take the full bootstrap. A heading jump (outline click, `#fragment`) to a heading not measured yet does not: md-viewer passes the heading's source offset as `measure_to_source`, the renderer measures 32 ms per frame instead of 4 ms until the frontier passes it, and `resolve_header_jump` scrolls once `get_header_position` has it, falling back to the line-ratio estimate if the document is measured past the heading without a position (devlog 115).

- **Keyboard document scrolling**: Plain document scroll keys are handled in `MarkdownApp::update` after mode-specific shortcuts are checked. `KeyboardScrollAction` maps Up/Down to fixed line steps and Page Up/Page Down to viewport-relative page steps through `keyboard_scroll_target`, which clamps against the active tab's `last_content_height`. The chosen target is assigned to the tab's `pending_scroll_offset`, so keyboard scrolling uses the same renderer-owned `ScrollArea` pipeline as outline and search jumps. Arrow keys are reserved for search-result navigation while the find bar is open, and document scrolling ignores Ctrl/Alt/Command-modified keypresses so it does not steal existing shortcuts.

- **Wide table scrolling**: Wide markdown / HTML tables are wrapped in a nested `egui::ScrollArea::horizontal()` so columns wider than the content area can still be reached. Plain vertical wheel stays with the outer document scroller; table horizontal movement uses the bottom scrollbar, native horizontal input, or `Shift+vertical-wheel` (egui turns it into a horizontal delta via `horizontal_scroll_modifier`, which the table's ScrollArea consumes; `tests/shift_wheel_table.rs`) so the cursor crossing a wide table during normal scrolling does not change its horizontal offset.

- **Right-to-left blocks (#108)**: when the cached events are parsed, `rtl_block_starts` records every top-level or blockquote paragraph/heading, list item (judged by its lead text) and table cell whose first strong character is right to left (and that holds no image, math, HTML, footnote reference or task checkbox). The renderer collects such a block's text and links instead of emitting a label per piece, then `render_rtl_block` draws it as one right-aligned, selectable galley and hit-tests clicks against the links' character ranges. An RTL list item reserves no marker slot on the left (`List::start_item(.., rtl)`); `render_rtl_item` puts the marker to the right of the text and indents nested items from the right. Table cells right-align within their column; column order is unchanged (devlogs 091, 092).

- **Global Allocator**: mimalloc for performance

## Key Libraries

| Crate | Purpose |
|-------|---------|
| eframe/egui 0.37 | GUI framework (glow backend for Wayland); text shaping via harfrust |
| epaint 0.37 (vendored) | Text layout; carries the emoji-presentation patch and upstream bidi layout (emilk/egui#8822): mixed-direction lines in visual order (devlog 090) |
| egui_commonmark 0.22 | Markdown rendering with syntax highlighting |
| notify 6.1 + notify-debouncer-mini 0.4 | File watching |
| rfd | Native file dialogs |
| clap | CLI argument parsing |
| regex | Header parsing for outline |

## Rendering Flow

```
update() → check_file_changes() → reload affected tabs
         → request_repaint_after(100ms) if watching
         → Apply theme, zoom settings
         → TopBottomPanel (menu bar + LIVE indicator + file path)
         → TopBottomPanel (error bar, if any)
         → TopBottomPanel (tab bar) → render_tab_bar()
         → SidePanel::left (file explorer) → render_file_explorer()
           → render_tree_node() (recursive)
         → CentralPanel → render_tab_content()
           → SidePanel::right (outline, if show_outline && headers exist)
           → ScrollArea::show_viewport → CommonMarkViewer
           → check_link_hooks() → handle navigation
         → Drag-and-drop overlay
```

Uses `show_viewport` for optimized rendering - egui clips content outside the visible area.

## Custom Tab System

The tab system uses a simple `Vec<Tab>` with an `active_tab` index:
- Tab bar rendered using `ui.selectable_label()` in a horizontal scroll area
- Close button (×) shown on hover or for active tab
- Context menu with "Close" and "Close Others" options
- Middle-click to close tabs
- Ctrl+Click on links opens in a new tab
- Regular click navigates within the current tab
- Each tab maintains independent navigation history (Alt+Left/Right)
- Session restore opens previously open tabs and restores active tab
- Closing the last tab is allowed; `tabs` may be empty, which renders the **welcome page** (issue #28). `render_welcome` (shown from `render_tab_content` when no tab is active) has Open File / Open Folder buttons and a recent-files list. `push_recent`/`record_recent` maintain `recent_files` (deduped, capped at `RECENT_FILES_CAP`, persisted). A fresh launch with no file shows it too — the old built-in sample document was removed.

## File Explorer

Left sidebar showing all markdown files in a hierarchical directory tree:
- Root directory determined by: CLI file → persisted state → first open tab → cwd
- Can be re-pointed at runtime via File → Open Folder… (`open_folder_dialog` → `FileExplorer::set_root`)
- Recursive scanning with 10 level depth limit
- Filters: .md, .markdown, .txt files only
- Skip hidden files (starting with .)
- Only shows directories containing markdown files
- Sorted: directories first, then files, alphabetically
- Toggle visibility with Ctrl+Shift+E
- Click file to open in new tab (or focus if already open)
- Expand/collapse directories with arrow buttons
- Refresh button to rescan directory
- Session persistence for: visibility, root directory, expanded directories
