# Development Plan: Lightweight Markdown Viewer

A Rust-based markdown viewer using egui and egui_commonmark.

## Project Overview

**Goal:** Build a fast, lightweight markdown viewer with live reload, syntax highlighting, and cross-platform support.

**Target metrics:**
- Binary size: < 5MB (< 3MB without syntax highlighting)
- Startup time: < 200ms
- Render performance: 60 FPS
- Memory: Minimal overhead with large documents

---

## Phase 1: Project Foundation

### 1.1 Initialize Cargo Project
- [x] Run `cargo init`
- [x] Configure `Cargo.toml` with dependencies
- [x] Set up release profile optimizations

### 1.2 Core Dependencies
```toml
eframe = "0.33"
egui = "0.33"
egui_commonmark = { version = "0.22", features = ["better_syntax_highlighting", "svg", "load-images"] }
image = { version = "0.25", features = ["png", "jpeg", "gif"] }
mimalloc = "0.1"
serde = { version = "1", features = ["derive"] }
```

### 1.3 System Dependencies (Arch Linux)
- [x] Install: `base-devel clang pkg-config libxcb libxkbcommon openssl gtk3 fontconfig dbus zenity xdg-desktop-portal xdg-desktop-portal-gtk`

**Deliverable:** Compiling skeleton project with dependencies resolved.

---

## Phase 2: Core Rendering Engine

### 2.1 Application Structure
- [x] Define `MarkdownApp` struct with:
  - `CommonMarkCache` (persistent across frames)
  - `content: String`
  - `current_file: Option<PathBuf>`
  - Theme state

### 2.2 Basic Markdown Rendering
- [x] Implement `eframe::App` trait
- [x] Set up `CentralPanel` with `ScrollArea`
- [x] Configure `CommonMarkViewer` with:
  - `max_image_width(Some(800))`
  - `indentation_spaces(2)`
  - `show_alt_text_on_hover(true)`
  - Syntax themes for dark/light modes

### 2.3 GFM Feature Verification
- [x] Tables with alignment
- [x] Task lists (checkboxes)
- [x] Strikethrough
- [x] Footnotes
- [x] Alert blocks (`[!NOTE]`, `[!WARNING]`, `[!TIP]`)
- [x] Code block syntax highlighting

**Deliverable:** Window displaying hardcoded markdown with all GFM features.

---

## Phase 3: File Operations

### 3.1 CLI Integration
- [x] Add `clap` dependency
- [x] Define CLI arguments:
  - `file: Option<PathBuf>` - file to open
  - `--watch` / `-w` - enable live reload

### 3.2 File Loading
- [x] Implement `load_file(&mut self, path: &PathBuf)`
- [x] Reset `CommonMarkCache` on new file load
- [x] Update window title with filename
- [x] Handle file read errors gracefully

### 3.3 Native File Dialogs
- [x] Add `rfd = "0.17"` dependency
- [x] Implement "Open..." menu item
- [x] File filter: `["md", "markdown", "txt"]`

### 3.4 Drag and Drop
- [x] Enable via `ViewportBuilder::with_drag_and_drop(true)`
- [x] Handle `ctx.input().raw.dropped_files`
- [x] Visual overlay during drag hover
- [x] Filter for markdown extensions

**Deliverable:** Open files via CLI, dialog, or drag-and-drop.

---

## Phase 4: Live Reload

### 4.1 File Watching Setup
- [x] Add dependencies:
  - `notify = "6.1"` (compatible with debouncer)
  - `notify-debouncer-mini = "0.4"`
- [x] Create `mpsc` channel for events
- [x] Implement `start_watching()` and `stop_watching()`

### 4.2 Event Handling
- [x] Non-blocking check via `try_recv()` in update loop
- [x] Reload file on modification event
- [x] Request repaint after reload
- [x] Handle watcher disconnection

### 4.3 UI Indicators
- [x] "LIVE" indicator in menu bar when watching
- [x] Toggle button in File menu
- [x] Periodic repaint request (100ms interval)

**Deliverable:** Files auto-reload when modified externally.

---

## Phase 5: User Interface

### 5.1 Menu Bar
- [x] File menu:
  - Open...
  - Watch file (toggle)
  - Separator
- [x] View menu:
  - Dark/Light mode toggle

### 5.2 Status Display
- [x] Current file path (right-aligned, dimmed)
- [x] Live indicator with green dot

### 5.3 Theme Support
- [x] Persist `dark_mode` preference
- [x] Apply `egui::Visuals::dark()` / `light()`
- [x] Sync syntax highlighting themes

### 5.4 Session Persistence
- [x] Implement `eframe::App::save()`
- [x] Store: dark_mode, last file path
- [x] Restore on startup

**Deliverable:** Complete UI with menu bar, themes, and persistence.

---

## Phase 6: Performance Optimization

### 6.1 Memory Allocator
- [x] Add `mimalloc = "0.1"`
- [x] Set global allocator:
  ```rust
  #[global_allocator]
  static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
  ```

### 6.2 Render Optimization
- [x] Use `ScrollArea::show_viewport` for lazy rendering
- [x] Never recreate `CommonMarkCache` per frame
- [x] Track viewport offset and content lines for large documents
- [x] Show line count indicator for documents > 10,000 lines

### 6.3 Binary Size
- [x] Release profile:
  - `opt-level = "z"`
  - `lto = true`
  - `codegen-units = 1`
  - `panic = "abort"`
  - `strip = true`
- [ ] Optional: nightly build with `-Zlocation-detail=none`

**Deliverable:** Smooth 60 FPS scrolling, binary ~8.7MB (includes full syntax highlighting, image support, Wayland+X11).

---

## Phase 7: Polish & Edge Cases

### 7.1 Error Handling
- [x] File not found display
- [x] Permission denied handling
- [x] Invalid UTF-8 handling (lossy conversion with warning)
- [x] Watcher failure recovery (auto-retry up to 3 times)

### 7.2 Window Configuration
- [x] Default size: 900x700
- [x] Minimum size: 400x300
- [x] Window title reflects current file

### 7.3 Wayland Compatibility
- [x] Use `glow` backend (not `wgpu`)
- [x] Enable both `wayland` and `x11` features
- [x] Test on Wayland without XWayland

### 7.4 Accessibility
- [x] URL tooltips enabled
- [x] Alt text on image hover
- [x] Keyboard navigation support (Ctrl+O/W/D/Q)

**Deliverable:** Production-ready application.

---

## File Structure

```
markdown_viewer/
├── Cargo.toml
├── Cargo.lock
├── src/
│   └── main.rs
├── docs/
│   └── DEVELOPMENT_PLAN.md
└── README.md
```

---

## Testing Checklist

### Functional Tests
- [x] Open file via CLI argument
- [x] Open file via File > Open dialog
- [x] Open file via drag-and-drop
- [x] Live reload triggers on file save
- [x] Theme toggle persists across sessions
- [x] All GFM features render correctly

### Performance Tests
- [x] Large document (10,000+ lines) scrolls smoothly (viewport-based rendering)
- [x] Memory usage stable during long sessions (mimalloc + CommonMarkCache)
- [x] Startup time under 200ms

### Platform Tests
- [x] X11 display server
- [x] Wayland display server
- [x] File dialogs work on both

---

## Milestones

| Milestone | Phases | Target |
|-----------|--------|--------|
| MVP | 1-3 | Basic viewer with file loading |
| Beta | 4-5 | Live reload and full UI |
| Release | 6-7 | Optimized and polished |

---

## Dependencies Summary

| Crate | Version | Purpose |
|-------|---------|---------|
| eframe | 0.33 | Desktop GUI framework |
| egui | 0.33 | Immediate-mode GUI |
| egui_commonmark | 0.22 | Markdown rendering |
| image | 0.25 | Image format support |
| rfd | 0.17 | Native file dialogs |
| notify | 6.1 | File system watching |
| notify-debouncer-mini | 0.4 | Event debouncing |
| clap | 4 | CLI argument parsing |
| mimalloc | 0.1 | Performance allocator |
| serde | 1 | Serialization |
