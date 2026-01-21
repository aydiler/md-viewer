# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
cargo build              # Debug build
cargo build --release    # Release build (optimized for size)
cargo run                # Run debug build
cargo run -- file.md     # Open a specific file
cargo run -- file.md -w  # Open with live reload (--watch)
make install             # Build release and install to ~/.local/bin
```

The release profile is configured for minimal binary size (`opt-level = "z"`, LTO, strip symbols).

## Architecture

Single-file Rust desktop application (`src/main.rs`, ~835 lines) for viewing markdown files using egui + egui_commonmark.

### Core Components

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

- **PersistedState**: Serializable struct for session persistence (dark_mode, last_file, zoom_level, show_outline). Stored via eframe's storage API with key `"md-viewer-state"`.

- **File Watching**: Uses `notify-debouncer-mini` with 200ms debounce. Events are polled non-blocking via `try_recv()` at start of each `update()` call. Auto-recovers up to 3 times on watcher failure.

- **Header Outline**: `parse_headers()` returns a `ParsedHeaders` struct containing `document_title` (first h1) and `outline_headers` (remaining headers). The first h1 is used as the sidebar title instead of "Outline". Headers are displayed in a resizable left sidebar with level-based indentation via string prefix. Click-to-navigate calculates scroll offset from line number ratio.

- **Global Allocator**: mimalloc for performance

### Key Libraries

| Crate | Purpose |
|-------|---------|
| eframe/egui 0.33 | GUI framework (glow backend for Wayland) |
| egui_commonmark 0.22 | Markdown rendering with syntax highlighting |
| notify 6.1 + notify-debouncer-mini 0.4 | File watching (notify-debouncer-mini 0.4 requires notify 6.x) |
| rfd | Native file dialogs |
| clap | CLI argument parsing |
| regex | Header parsing for outline |

### Rendering Flow

```
update() → check_file_changes() → reload if needed
         → request_repaint_after(100ms) if watching
         → TopBottomPanel (menu bar + LIVE indicator)
         → SidePanel::left (outline, if show_outline && outline_headers exist)
         → CentralPanel → ScrollArea::show_viewport → CommonMarkViewer
```

Uses `show_viewport` for optimized rendering - egui clips content outside the visible area.

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Ctrl+O | Open file dialog |
| Ctrl+W | Toggle file watching |
| Ctrl+D | Toggle dark/light mode |
| Ctrl+Shift+O | Toggle outline sidebar |
| Ctrl+Q | Quit application |
| Ctrl++ / Ctrl+= | Zoom in |
| Ctrl+- | Zoom out |
| Ctrl+0 | Reset zoom to 100% |
| Ctrl+Scroll | Zoom in/out with mouse wheel |

## Target Metrics

- Binary size: ~8.7MB (includes full syntax highlighting via syntect, image support, Wayland+X11)
- Startup time: < 200ms
- Render: 60 FPS with viewport-based lazy rendering
- Platform: Linux X11 and Wayland

## Known Issues

None currently tracked.

## Planned Features

See `docs/IMPLEMENTATION-PLAN.md` for detailed implementation plans for:
- **Phase D**: Simple link handler with navigation history
- **Phase A**: Multi-window support via egui viewports
- **Phase B**: Tab system (custom or egui_dock)
- **Phase C**: Hybrid tabs + multi-window

Key discovery: egui_commonmark has a link hooks mechanism (`cache.add_link_hook()`) that can intercept link clicks instead of opening in browser.

## Worktree Workflow

This repo uses a bare repository setup at `~/markdown-viewer.git`. All work happens in worktrees.

### Creating a Feature Worktree

```bash
# From the bare repo directory
cd ~/markdown-viewer.git
git worktree add ../markdown-viewer-my-feature -b feature/my-feature
cd ../markdown-viewer-my-feature
```

### Managing Worktrees

```bash
git worktree list                           # See all worktrees
git worktree remove ../markdown-viewer-foo  # Remove after merge
```

### Slash Command

Use `/feature <description>` to automatically create a worktree, implement a feature, and report when done.

## System Dependencies (Arch Linux)

```bash
sudo pacman -S --needed base-devel clang pkg-config libxcb libxkbcommon openssl gtk3 fontconfig dbus zenity xdg-desktop-portal xdg-desktop-portal-gtk
```
