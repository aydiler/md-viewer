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

Single-file Rust desktop application (`src/main.rs`) for viewing markdown files using egui + egui_commonmark.

### Core Components

- **MarkdownApp**: Main struct implementing `eframe::App`. Holds:
  - `CommonMarkCache` - **must persist across frames** (never recreate per-frame)
  - `content: String` - current markdown text
  - `current_file: Option<PathBuf>` - loaded file path
  - `watcher` + `watcher_rx` - file watching via mpsc channel
  - `scroll_offset` + `content_lines` - viewport tracking for performance

- **PersistedState**: Serializable struct for session persistence (dark_mode, last_file). Stored via eframe's storage API.

- **File Watching**: Uses `notify-debouncer-mini` with 200ms debounce. Events are polled non-blocking via `try_recv()` at start of each `update()` call.

- **Global Allocator**: mimalloc for performance

### Key Libraries

| Crate | Purpose |
|-------|---------|
| eframe/egui 0.33 | GUI framework (glow backend for Wayland) |
| egui_commonmark 0.22 | Markdown rendering with syntax highlighting |
| notify 6.1 + notify-debouncer-mini 0.4 | File watching (versions must match) |
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

## Development Roadmap

See `docs/DEVELOPMENT_PLAN.md` for the 7-phase plan - all phases complete:
- Phases 1-6: Foundation, rendering, file ops, live reload, UI/persistence, performance
- Phase 7: Polish (error handling, watcher recovery, keyboard shortcuts Ctrl+O/W/D/Q)

## Target Metrics

- Binary size: ~8.7MB (includes full syntax highlighting via syntect)
- Startup time: < 200ms
- Render: 60 FPS with viewport-based lazy rendering
- Platform: Linux X11 and Wayland

## System Dependencies (Arch Linux)

```bash
sudo pacman -S --needed base-devel clang pkg-config libxcb libxkbcommon openssl gtk3 fontconfig dbus zenity xdg-desktop-portal xdg-desktop-portal-gtk
```
