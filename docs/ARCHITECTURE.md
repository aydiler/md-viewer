# Architecture

Single-file Rust desktop application (`src/main.rs`, ~1050 lines) for viewing markdown files using egui + egui_commonmark.

## Core Components

- **MarkdownApp**: Main struct implementing `eframe::App`. Holds:
  - `CommonMarkCache` - **must persist across frames** (never recreate per-frame, only reset on file load)
  - `content: String` - current markdown text
  - `current_file: Option<PathBuf>` - loaded file path
  - `watcher` + `watcher_rx` - file watching via mpsc channel
  - `scroll_offset` + `content_lines` - viewport tracking for performance
  - `document_title: Option<String>` - first h1 header used as sidebar title
  - `outline_headers: Vec<Header>` - parsed headers for outline (excludes first h1)
  - `show_outline: bool` - toggle sidebar visibility
  - `pending_scroll_offset` - scroll target for outline navigation
  - `history_back` + `history_forward` - navigation history for back/forward
  - `local_links: Vec<String>` - cached local markdown links for link hook handling

- **PersistedState**: Serializable struct for session persistence (dark_mode, last_file, zoom_level, show_outline). Stored via eframe's storage API with key `"md-viewer-state"`.

- **File Watching**: Uses `notify-debouncer-mini` with 200ms debounce. Events are polled non-blocking via `try_recv()` at start of each `update()` call. Auto-recovers up to 3 times on watcher failure.

- **Header Outline**: `parse_headers()` returns a `ParsedHeaders` struct containing `document_title` (first h1) and `outline_headers` (remaining headers). The first h1 is used as the sidebar title instead of "Outline". Headers are displayed in a resizable left sidebar with level-based indentation via string prefix. Click-to-navigate calculates scroll offset from line number ratio.

- **Link Navigation**: Uses egui_commonmark's link hook mechanism to intercept clicks on local markdown links. `parse_local_links()` extracts all relative markdown file links and anchor-only links from content (skipping code blocks). Links are registered via `cache.add_link_hook()` and checked after each render via `get_link_hook()`. Navigation resolves paths relative to current file's directory and maintains back/forward history. Anchor-only links (`#section`) are intercepted but ignored (prevents browser errors).

- **Global Allocator**: mimalloc for performance

## Key Libraries

| Crate | Purpose |
|-------|---------|
| eframe/egui 0.33 | GUI framework (glow backend for Wayland) |
| egui_commonmark 0.22 | Markdown rendering with syntax highlighting |
| notify 6.1 + notify-debouncer-mini 0.4 | File watching (notify-debouncer-mini 0.4 requires notify 6.x) |
| rfd | Native file dialogs |
| clap | CLI argument parsing |
| regex | Header parsing for outline |

## Rendering Flow

```
update() → check_file_changes() → reload if needed
         → request_repaint_after(100ms) if watching
         → TopBottomPanel (menu bar + LIVE indicator)
         → SidePanel::left (outline, if show_outline && outline_headers exist)
         → CentralPanel → ScrollArea::show_viewport → CommonMarkViewer
         → check_link_hooks() → navigate_to_link() if local link clicked
```

Uses `show_viewport` for optimized rendering - egui clips content outside the visible area.
