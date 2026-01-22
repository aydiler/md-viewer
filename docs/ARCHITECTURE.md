# Architecture

Single-file Rust desktop application (`src/main.rs`, ~1100 lines) for viewing markdown files using egui + egui_commonmark with a custom tab system.

## Core Components

- **MarkdownApp**: Main struct implementing `eframe::App`. Holds:
  - `tabs: Vec<Tab>` - list of open tabs
  - `active_tab: usize` - index of the currently active tab
  - `dark_mode: bool` - global theme setting
  - `zoom_level: f32` - global zoom (0.5 to 3.0)
  - `show_outline: bool` - toggle outline sidebar visibility
  - `watch_enabled: bool` - file watching state
  - `watcher` + `watcher_rx` - file watching via mpsc channel
  - `watched_paths: HashSet<PathBuf>` - unified watcher for all open tabs
  - `hovered_tab: Option<usize>` - for showing close button on hover

- **Tab**: Per-tab state for a document. Each tab has:
  - `id: egui::Id` - unique identifier
  - `path: PathBuf` - file path
  - `content: String` - markdown text
  - `cache: CommonMarkCache` - **must persist across frames** (never recreate per-frame, only reset on file load)
  - `document_title: Option<String>` - first h1 used as sidebar title
  - `outline_headers: Vec<Header>` - parsed headers for outline
  - `scroll_offset`, `pending_scroll_offset`, `last_content_height` - scroll state
  - `local_links: Vec<String>` - cached local markdown links
  - `history_back`, `history_forward: Vec<PathBuf>` - per-tab navigation history

- **PersistedState**: Serializable struct for session persistence:
  - `dark_mode: Option<bool>`
  - `zoom_level: Option<f32>`
  - `show_outline: Option<bool>`
  - `open_tabs: Option<Vec<PathBuf>>` - restore tabs on startup
  - `active_tab: Option<usize>` - restore active tab position

- **File Watching**: Uses `notify-debouncer-mini` with 200ms debounce. Watches all open tab paths. On change, reloads matching tabs. Auto-recovers up to 3 times on watcher failure.

- **Header Outline**: `parse_headers()` returns a `ParsedHeaders` struct containing `document_title` (first h1) and `outline_headers` (remaining headers). Rendered as a resizable left sidebar.

- **Link Navigation**: Uses egui_commonmark's link hook mechanism. Ctrl+Click opens links in new tabs, regular click navigates within the current tab.

- **Global Allocator**: mimalloc for performance

## Key Libraries

| Crate | Purpose |
|-------|---------|
| eframe/egui 0.33 | GUI framework (glow backend for Wayland) |
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
         → CentralPanel → render_tab_content()
           → SidePanel::left (outline, if show_outline && headers exist)
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
