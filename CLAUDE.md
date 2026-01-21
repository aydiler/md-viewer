# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
cargo build              # Debug build
cargo build --release    # Release build (optimized for size)
cargo run                # Run debug build
cargo run -- file.md     # Open a specific file
cargo run -- file.md -w  # Open with live reload (--watch)
```

The release profile is configured for minimal binary size (`opt-level = "z"`, LTO, strip symbols).

## Architecture

Single-file Rust desktop application (`src/main.rs`, ~600 lines) for viewing markdown files using egui + egui_commonmark.

### Core Components

- **MarkdownApp**: Main struct implementing `eframe::App`. Holds:
  - `CommonMarkCache` - **must persist across frames** (never recreate per-frame, only reset on file load)
  - `content: String` - current markdown text
  - `current_file: Option<PathBuf>` - loaded file path
  - `watcher` + `watcher_rx` - file watching via mpsc channel
  - `scroll_offset` + `content_lines` - viewport tracking for performance

- **PersistedState**: Serializable struct for session persistence (dark_mode, last_file, zoom_level). Stored via eframe's storage API with key `"md-viewer-state"`.

- **File Watching**: Uses `notify-debouncer-mini` with 200ms debounce. Events are polled non-blocking via `try_recv()` at start of each `update()` call. Auto-recovers up to 3 times on watcher failure.

- **Global Allocator**: mimalloc for performance

### Key Libraries

| Crate | Purpose |
|-------|---------|
| eframe/egui 0.33 | GUI framework (glow backend for Wayland) |
| egui_commonmark 0.22 | Markdown rendering with syntax highlighting |
| notify 6.1 + notify-debouncer-mini 0.4 | File watching (notify-debouncer-mini 0.4 requires notify 6.x) |
| rfd | Native file dialogs |
| clap | CLI argument parsing |

### Rendering Flow

```
update() → check_file_changes() → reload if needed
         → request_repaint_after(100ms) if watching
         → TopBottomPanel (menu bar + LIVE indicator)
         → CentralPanel → ScrollArea::show_viewport → CommonMarkViewer
```

Uses `show_viewport` for optimized rendering - egui clips content outside the visible area.

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Ctrl+O | Open file dialog |
| Ctrl+W | Toggle file watching |
| Ctrl+D | Toggle dark/light mode |
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

## System Dependencies (Arch Linux)

```bash
sudo pacman -S --needed base-devel clang pkg-config libxcb libxkbcommon openssl gtk3 fontconfig dbus zenity xdg-desktop-portal xdg-desktop-portal-gtk
```
