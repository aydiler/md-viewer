# Implementation Plan: Markdown File Links (D→A→B→C)

This document provides a detailed implementation plan for adding markdown file link support to the markdown viewer, progressing through four phases of increasing complexity.

## Table of Contents

1. [Phase D: Simple Link Handler](#phase-d-simple-link-handler)
2. [Phase A: Multi-Window Support](#phase-a-multi-window-support)
3. [Phase B: Tab System](#phase-b-tab-system)
4. [Phase C: Hybrid (Tabs + Multi-Window)](#phase-c-hybrid-tabs--multi-window)
5. [Shared Components](#shared-components)
6. [References](#references)

---

## Shared Components

These components are used across all phases and should be implemented first.

### Link Detection Module

Create a new module for parsing and detecting markdown links.

```rust
// src/links.rs (new file) or add to main.rs

use std::path::{Path, PathBuf};
use regex::Regex;

/// Represents a detected markdown link
#[derive(Debug, Clone)]
pub struct MarkdownLink {
    pub text: String,
    pub destination: String,
    pub resolved_path: Option<PathBuf>,
    pub is_local_md: bool,
}

/// Extract all markdown links from content
pub fn extract_links(content: &str) -> Vec<MarkdownLink> {
    // Matches [text](destination) pattern
    let link_re = Regex::new(r"\[([^\]]*)\]\(([^)]+)\)").unwrap();

    link_re.captures_iter(content)
        .map(|cap| {
            let text = cap.get(1).map_or("", |m| m.as_str()).to_string();
            let destination = cap.get(2).map_or("", |m| m.as_str()).to_string();
            let is_local_md = is_local_markdown_link(&destination);

            MarkdownLink {
                text,
                destination,
                resolved_path: None,
                is_local_md,
            }
        })
        .collect()
}

/// Check if a link points to a local markdown file
pub fn is_local_markdown_link(destination: &str) -> bool {
    // Skip URLs with schemes (http://, https://, mailto:, etc.)
    if destination.contains("://") || destination.starts_with("mailto:") {
        return false;
    }

    // Check for .md or .markdown extension
    let lower = destination.to_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

/// Resolve a relative link path against a base file path
pub fn resolve_link_path(link: &str, base_file: &Path) -> Option<PathBuf> {
    let base_dir = base_file.parent()?;

    // Handle anchor links (e.g., "file.md#section")
    let path_part = link.split('#').next()?;

    let resolved = if path_part.starts_with('/') {
        // Absolute path
        PathBuf::from(path_part)
    } else {
        // Relative path
        base_dir.join(path_part)
    };

    // Canonicalize to resolve .. and . components
    resolved.canonicalize().ok().or(Some(resolved))
}

/// Extract and resolve all local markdown links
pub fn extract_local_md_links(content: &str, base_file: &Path) -> Vec<PathBuf> {
    extract_links(content)
        .into_iter()
        .filter(|link| link.is_local_md)
        .filter_map(|link| resolve_link_path(&link.destination, base_file))
        .collect()
}
```

### Link Hook Registration

Add to `MarkdownApp`:

```rust
impl MarkdownApp {
    /// Register link hooks for all local .md links in the current content
    fn register_link_hooks(&mut self) {
        // Clear previous hooks
        self.cache.link_hooks_clear();

        let Some(current_file) = &self.current_file else {
            return;
        };

        // Extract all local markdown links
        let links = extract_local_md_links(&self.content, current_file);

        // Register each as a hook
        for link_path in links {
            let hook_key = link_path.to_string_lossy().to_string();
            self.cache.add_link_hook(hook_key);
        }
    }

    /// Check if any link hooks were clicked and return the clicked path
    fn get_clicked_link(&self) -> Option<PathBuf> {
        for (path_str, clicked) in self.cache.link_hooks() {
            if *clicked {
                return Some(PathBuf::from(path_str));
            }
        }
        None
    }
}
```

---

## Phase D: Simple Link Handler

**Goal**: Replace current content when clicking a local `.md` link, with back/forward navigation.

**Complexity**: Low (~80-120 lines)

### New State Fields

```rust
struct MarkdownApp {
    // ... existing fields ...

    // Navigation history
    history: Vec<PathBuf>,
    history_index: usize,
}
```

### Implementation

```rust
impl MarkdownApp {
    fn new(cc: &eframe::CreationContext<'_>, file: Option<PathBuf>, watch: bool) -> Self {
        // ... existing initialization ...

        let mut app = Self {
            // ... existing fields ...
            history: Vec::new(),
            history_index: 0,
        };

        // ... rest of initialization ...
        app
    }

    /// Navigate to a file, adding to history
    fn navigate_to(&mut self, path: &PathBuf) {
        // If we're not at the end of history, truncate forward history
        if self.history_index < self.history.len() {
            self.history.truncate(self.history_index);
        }

        // Add current file to history before navigating (if exists)
        if let Some(current) = &self.current_file {
            if self.history.last() != Some(current) {
                self.history.push(current.clone());
                self.history_index = self.history.len();
            }
        }

        // Load the new file
        self.load_file(path);

        // Add new file to history
        if self.current_file.is_some() {
            self.history.push(path.clone());
            self.history_index = self.history.len();
        }
    }

    /// Go back in history
    fn navigate_back(&mut self) -> bool {
        if self.history_index > 1 {
            self.history_index -= 1;
            let path = self.history[self.history_index - 1].clone();
            self.load_file(&path);
            true
        } else {
            false
        }
    }

    /// Go forward in history
    fn navigate_forward(&mut self) -> bool {
        if self.history_index < self.history.len() {
            self.history_index += 1;
            let path = self.history[self.history_index - 1].clone();
            self.load_file(&path);
            true
        } else {
            false
        }
    }

    /// Check if back navigation is available
    fn can_go_back(&self) -> bool {
        self.history_index > 1
    }

    /// Check if forward navigation is available
    fn can_go_forward(&self) -> bool {
        self.history_index < self.history.len()
    }
}
```

### Update `load_file()` to Register Hooks

```rust
fn load_file(&mut self, path: &PathBuf) {
    // ... existing load_file code ...

    match fs::read(path) {
        Ok(bytes) => {
            // ... existing content loading ...

            // Register link hooks for local .md files
            self.register_link_hooks();
        }
        Err(e) => {
            // ... existing error handling ...
        }
    }
}
```

### Update `update()` for Link Handling and Navigation

```rust
impl eframe::App for MarkdownApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for clicked links BEFORE rendering (hooks are reset during render)
        let clicked_link = self.get_clicked_link();

        // ... existing file change check ...

        // Handle keyboard shortcuts
        let mut go_back = false;
        let mut go_forward = false;

        ctx.input(|i| {
            // ... existing shortcuts ...

            // Alt+Left or Backspace: Go back
            if (i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft))
               || (!i.modifiers.ctrl && i.key_pressed(egui::Key::Backspace)) {
                go_back = true;
            }
            // Alt+Right: Go forward
            if i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight) {
                go_forward = true;
            }
        });

        // Handle navigation
        if go_back {
            self.navigate_back();
        }
        if go_forward {
            self.navigate_forward();
        }

        // Handle clicked link
        if let Some(path) = clicked_link {
            if path.exists() {
                self.navigate_to(&path);
            } else {
                self.error_message = Some(format!("File not found: {}", path.display()));
            }
        }

        // ... rest of update() ...

        // Menu bar with navigation buttons
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                // Navigation buttons
                ui.add_enabled_ui(self.can_go_back(), |ui| {
                    if ui.button("◀").on_hover_text("Back (Alt+Left)").clicked() {
                        self.navigate_back();
                    }
                });
                ui.add_enabled_ui(self.can_go_forward(), |ui| {
                    if ui.button("▶").on_hover_text("Forward (Alt+Right)").clicked() {
                        self.navigate_forward();
                    }
                });

                ui.separator();

                // ... existing menu items ...
            });
        });

        // ... rest of update() ...
    }
}
```

### Keyboard Shortcuts Added

| Shortcut | Action |
|----------|--------|
| Alt+Left / Backspace | Go back in history |
| Alt+Right | Go forward in history |

---

## Phase A: Multi-Window Support

**Goal**: Open linked markdown files in new OS windows using egui viewports.

**Complexity**: Medium (~150-200 lines)

**Prerequisites**: Phase D components (link detection, hook registration)

### New State Structures

```rust
use egui::{ViewportBuilder, ViewportId};

/// Represents a child window displaying a markdown file
struct ChildWindow {
    /// Unique viewport ID
    id: ViewportId,
    /// File path being displayed
    path: PathBuf,
    /// Markdown content
    content: String,
    /// Markdown cache (must be per-window)
    cache: CommonMarkCache,
    /// Whether the window is open
    show: bool,
    /// Parsed headers for outline
    document_title: Option<String>,
    outline_headers: Vec<Header>,
    /// Scroll position
    scroll_offset: f32,
    /// Content metrics
    content_lines: usize,
    last_content_height: f32,
}

impl ChildWindow {
    fn new(path: PathBuf) -> Option<Self> {
        let content = std::fs::read_to_string(&path).ok()?;
        let content_lines = content.lines().count();
        let parsed = parse_headers(&content);

        Some(Self {
            id: ViewportId::from_hash_of(&path),
            path,
            content,
            cache: CommonMarkCache::default(),
            show: true,
            document_title: parsed.document_title,
            outline_headers: parsed.outline_headers,
            scroll_offset: 0.0,
            content_lines,
            last_content_height: 0.0,
        })
    }

    fn window_title(&self) -> String {
        self.path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Markdown".to_string())
    }
}

struct MarkdownApp {
    // ... existing fields from Phase D ...

    /// Child windows for linked files
    child_windows: Vec<ChildWindow>,

    /// Settings shared across windows
    dark_mode: bool,
    zoom_level: f32,
    show_outline: bool,
}
```

### Opening Links in New Windows

```rust
impl MarkdownApp {
    /// Open a markdown file in a new window
    fn open_in_new_window(&mut self, path: &PathBuf) {
        // Check if already open
        for window in &mut self.child_windows {
            if window.path == *path {
                window.show = true;
                return;
            }
        }

        // Create new window
        if let Some(window) = ChildWindow::new(path.clone()) {
            self.child_windows.push(window);
        }
    }

    /// Handle link click - decide whether to open in same view or new window
    fn handle_link_click(&mut self, path: &PathBuf, ctx: &egui::Context) {
        // Check for modifier keys
        let open_in_new_window = ctx.input(|i| {
            i.modifiers.ctrl || i.modifiers.command
        });

        if open_in_new_window {
            self.open_in_new_window(path);
        } else {
            // Default: navigate in current view (Phase D behavior)
            self.navigate_to(path);
        }
    }
}
```

### Rendering Child Windows

Add to `update()`:

```rust
impl eframe::App for MarkdownApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ... existing update code ...

        // Render child windows
        // Note: We need to collect data first to avoid borrow issues
        let dark_mode = self.dark_mode;
        let zoom_level = self.zoom_level;
        let show_outline = self.show_outline;

        // Use indices to avoid borrow checker issues
        let window_count = self.child_windows.len();
        for i in 0..window_count {
            if !self.child_windows[i].show {
                continue;
            }

            let window = &mut self.child_windows[i];
            let viewport_id = window.id;
            let title = window.window_title();

            ctx.show_viewport_deferred(
                viewport_id,
                ViewportBuilder::default()
                    .with_title(title)
                    .with_inner_size([900.0, 700.0])
                    .with_min_inner_size([400.0, 300.0]),
                move |ctx, _class| {
                    // Apply shared settings
                    ctx.set_visuals(if dark_mode {
                        egui::Visuals::dark()
                    } else {
                        egui::Visuals::light()
                    });
                    ctx.set_zoom_factor(zoom_level);

                    // Render content
                    Self::render_child_window_content(ctx, window, show_outline);

                    // Handle close request
                    if ctx.input(|i| i.viewport().close_requested()) {
                        window.show = false;
                    }
                },
            );
        }

        // Clean up closed windows
        self.child_windows.retain(|w| w.show);
    }

    fn render_child_window_content(
        ctx: &egui::Context,
        window: &mut ChildWindow,
        show_outline: bool,
    ) {
        // Optional: Menu bar for child window
        egui::TopBottomPanel::top("child_menu").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&window.path.display().to_string())
                        .small()
                        .color(ui.visuals().weak_text_color())
                );
            });
        });

        // Optional: Outline sidebar
        if show_outline && !window.outline_headers.is_empty() {
            egui::SidePanel::left("child_outline")
                .resizable(true)
                .default_width(180.0)
                .show(ctx, |ui| {
                    let title = window.document_title.as_deref().unwrap_or("Outline");
                    ui.heading(title);
                    ui.separator();

                    for header in &window.outline_headers {
                        let indent = (header.level.saturating_sub(2) as usize) * 2;
                        let prefix = " ".repeat(indent);
                        ui.selectable_label(false, format!("{}{}", prefix, header.title));
                    }
                });
        }

        // Main content
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                CommonMarkViewer::new()
                    .max_image_width(Some(800))
                    .show(ui, &mut window.cache, &window.content);
            });
        });
    }
}
```

### Keyboard Shortcuts Updated

| Shortcut | Action |
|----------|--------|
| Click link | Open in current view |
| Ctrl+Click link | Open in new window |
| Alt+Left | Go back (current view) |
| Alt+Right | Go forward (current view) |

---

## Phase B: Tab System

**Goal**: Implement a tab bar for switching between multiple documents in a single window.

**Complexity**: High (~300-400 lines)

**Options**:
1. **Custom tabs using `selectable_value`** - Lighter weight, full control
2. **[egui_dock](https://docs.rs/egui_dock)** - Full-featured with docking support

### Option B1: Custom Tab Implementation

#### Tab State Structure

```rust
/// Represents a single tab with its document state
#[derive(Clone)]
struct Tab {
    /// Unique identifier
    id: egui::Id,
    /// File path
    path: PathBuf,
    /// Document content
    content: String,
    /// Markdown cache
    cache: CommonMarkCache,
    /// Parsed headers
    document_title: Option<String>,
    outline_headers: Vec<Header>,
    /// Scroll state
    scroll_offset: f32,
    last_content_height: f32,
    pending_scroll_offset: Option<f32>,
    /// Content metrics
    content_lines: usize,
    /// File watcher state
    watcher: Option<Debouncer<RecommendedWatcher>>,
    watcher_rx: Option<Receiver<Result<Vec<DebouncedEvent>, notify::Error>>>,
    /// Navigation history for this tab
    history: Vec<PathBuf>,
    history_index: usize,
    /// Tab is modified (for visual indicator)
    modified: bool,
}

impl Tab {
    fn new(path: PathBuf) -> Option<Self> {
        let content = std::fs::read_to_string(&path).ok()?;
        let parsed = parse_headers(&content);

        Some(Self {
            id: egui::Id::new(&path),
            path: path.clone(),
            content,
            cache: CommonMarkCache::default(),
            document_title: parsed.document_title,
            outline_headers: parsed.outline_headers,
            scroll_offset: 0.0,
            last_content_height: 0.0,
            pending_scroll_offset: None,
            content_lines: content.lines().count(),
            watcher: None,
            watcher_rx: None,
            history: vec![path],
            history_index: 1,
            modified: false,
        })
    }

    fn title(&self) -> String {
        self.path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string())
    }
}
```

#### Refactored App State

```rust
struct MarkdownApp {
    /// All open tabs
    tabs: Vec<Tab>,
    /// Index of active tab
    active_tab: usize,

    /// Global settings (shared across tabs)
    dark_mode: bool,
    zoom_level: f32,
    show_outline: bool,
    watch_enabled: bool,

    /// UI state
    error_message: Option<String>,
    is_dragging: bool,

    /// Tab being dragged for reordering
    dragging_tab: Option<usize>,
}
```

#### Tab Bar Rendering

```rust
impl MarkdownApp {
    /// Render the tab bar
    fn render_tab_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Horizontal scroll for many tabs
            egui::ScrollArea::horizontal()
                .max_width(ui.available_width() - 30.0) // Leave room for + button
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let mut tab_to_close: Option<usize> = None;
                        let mut tab_to_activate: Option<usize> = None;

                        for (idx, tab) in self.tabs.iter().enumerate() {
                            let is_active = idx == self.active_tab;

                            // Tab button with close button
                            ui.horizontal(|ui| {
                                // Tab selection
                                let response = ui.selectable_label(
                                    is_active,
                                    &tab.title()
                                );

                                if response.clicked() {
                                    tab_to_activate = Some(idx);
                                }

                                // Middle-click to close
                                if response.middle_clicked() {
                                    tab_to_close = Some(idx);
                                }

                                // Close button (only show on hover or active)
                                if is_active || response.hovered() {
                                    if ui.small_button("×").clicked() {
                                        tab_to_close = Some(idx);
                                    }
                                }

                                // Context menu
                                response.context_menu(|ui| {
                                    if ui.button("Close").clicked() {
                                        tab_to_close = Some(idx);
                                        ui.close_menu();
                                    }
                                    if ui.button("Close Others").clicked() {
                                        // Keep only this tab
                                        self.close_other_tabs(idx);
                                        ui.close_menu();
                                    }
                                    if ui.button("Close All").clicked() {
                                        self.close_all_tabs();
                                        ui.close_menu();
                                    }
                                });
                            });

                            ui.separator();
                        }

                        // Apply changes after iteration
                        if let Some(idx) = tab_to_activate {
                            self.active_tab = idx;
                        }
                        if let Some(idx) = tab_to_close {
                            self.close_tab(idx);
                        }
                    });
                });

            // New tab button
            if ui.button("+").on_hover_text("New Tab (Ctrl+T)").clicked() {
                self.open_file_dialog_for_new_tab();
            }
        });
    }

    fn close_tab(&mut self, idx: usize) {
        if self.tabs.len() <= 1 {
            // Don't close the last tab, show default content instead
            return;
        }

        self.tabs.remove(idx);

        // Adjust active tab index
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if self.active_tab > idx {
            self.active_tab -= 1;
        }
    }

    fn close_other_tabs(&mut self, keep_idx: usize) {
        let kept_tab = self.tabs.remove(keep_idx);
        self.tabs.clear();
        self.tabs.push(kept_tab);
        self.active_tab = 0;
    }

    fn close_all_tabs(&mut self) {
        // Keep at least one tab with default content
        self.tabs.clear();
        self.tabs.push(Tab::default());
        self.active_tab = 0;
    }

    fn open_in_new_tab(&mut self, path: &PathBuf) {
        // Check if already open
        for (idx, tab) in self.tabs.iter().enumerate() {
            if tab.path == *path {
                self.active_tab = idx;
                return;
            }
        }

        // Create new tab
        if let Some(tab) = Tab::new(path.clone()) {
            self.tabs.push(tab);
            self.active_tab = self.tabs.len() - 1;
        }
    }
}
```

#### Updated Update Loop

```rust
impl eframe::App for MarkdownApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply global settings
        ctx.set_visuals(if self.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        ctx.set_zoom_factor(self.zoom_level);

        // Update window title based on active tab
        if let Some(tab) = self.tabs.get(self.active_tab) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(
                format!("{} - Markdown Viewer", tab.title())
            ));
        }

        // Keyboard shortcuts
        ctx.input(|i| {
            // Ctrl+T: New tab
            if i.modifiers.ctrl && i.key_pressed(egui::Key::T) {
                self.open_file_dialog_for_new_tab();
            }
            // Ctrl+W: Close current tab
            if i.modifiers.ctrl && i.key_pressed(egui::Key::W) {
                self.close_tab(self.active_tab);
            }
            // Ctrl+Tab: Next tab
            if i.modifiers.ctrl && i.key_pressed(egui::Key::Tab) {
                if !i.modifiers.shift {
                    self.active_tab = (self.active_tab + 1) % self.tabs.len();
                } else {
                    // Ctrl+Shift+Tab: Previous tab
                    self.active_tab = if self.active_tab == 0 {
                        self.tabs.len() - 1
                    } else {
                        self.active_tab - 1
                    };
                }
            }
            // Ctrl+1-9: Switch to tab by number
            for n in 1..=9 {
                let key = match n {
                    1 => egui::Key::Num1,
                    2 => egui::Key::Num2,
                    3 => egui::Key::Num3,
                    4 => egui::Key::Num4,
                    5 => egui::Key::Num5,
                    6 => egui::Key::Num6,
                    7 => egui::Key::Num7,
                    8 => egui::Key::Num8,
                    9 => egui::Key::Num9,
                    _ => continue,
                };
                if i.modifiers.ctrl && i.key_pressed(key) {
                    let idx = n - 1;
                    if idx < self.tabs.len() {
                        self.active_tab = idx;
                    }
                }
            }
        });

        // Top panel with menu and tab bar
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // Menu bar
            egui::MenuBar::new().ui(ui, |ui| {
                // ... existing menus ...
            });

            ui.separator();

            // Tab bar
            self.render_tab_bar(ui);
        });

        // Render active tab content
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            // Outline sidebar
            if self.show_outline && !tab.outline_headers.is_empty() {
                egui::SidePanel::left("outline").show(ctx, |ui| {
                    // ... outline rendering using tab.outline_headers ...
                });
            }

            // Main content
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    CommonMarkViewer::new()
                        .max_image_width(Some(800))
                        .show(ui, &mut tab.cache, &tab.content);
                });
            });
        }
    }
}
```

### Option B2: Using egui_dock

Add dependency to `Cargo.toml`:

```toml
[dependencies]
egui_dock = "0.15"  # Check for latest version
```

#### Implementation with egui_dock

```rust
use egui_dock::{DockArea, DockState, TabViewer, NodeIndex};

/// Tab data for egui_dock
struct DocTab {
    path: PathBuf,
    content: String,
    cache: CommonMarkCache,
    document_title: Option<String>,
    outline_headers: Vec<Header>,
}

/// TabViewer implementation
struct DocTabViewer<'a> {
    dark_mode: bool,
    show_outline: bool,
    // Mutable access to app for link handling
    added_tabs: &'a mut Vec<DocTab>,
}

impl TabViewer for DocTabViewer<'_> {
    type Tab = DocTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string())
            .into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        CommonMarkViewer::new()
            .max_image_width(Some(800))
            .show(ui, &mut tab.cache, &tab.content);
    }

    fn closeable(&mut self, _tab: &mut Self::Tab) -> bool {
        true
    }

    fn on_close(&mut self, _tab: &mut Self::Tab) -> bool {
        true // Allow closing
    }
}

struct MarkdownApp {
    dock_state: DockState<DocTab>,
    dark_mode: bool,
    show_outline: bool,
}

impl MarkdownApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Create initial dock state with one tab
        let tabs = vec![DocTab::default()];
        let dock_state = DockState::new(tabs);

        Self {
            dock_state,
            dark_mode: true,
            show_outline: true,
        }
    }

    fn open_file_in_new_tab(&mut self, path: PathBuf) {
        if let Some(tab) = DocTab::new(path) {
            // Add to the focused leaf or create new
            self.dock_state.push_to_focused_leaf(tab);
        }
    }
}

impl eframe::App for MarkdownApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut added_tabs = Vec::new();

        egui::CentralPanel::default().show(ctx, |ui| {
            let mut viewer = DocTabViewer {
                dark_mode: self.dark_mode,
                show_outline: self.show_outline,
                added_tabs: &mut added_tabs,
            };

            DockArea::new(&mut self.dock_state)
                .show_inside(ui, &mut viewer);
        });

        // Add any tabs that were requested
        for tab in added_tabs {
            self.dock_state.push_to_focused_leaf(tab);
        }
    }
}
```

### Keyboard Shortcuts for Tabs

| Shortcut | Action |
|----------|--------|
| Ctrl+T | New tab |
| Ctrl+W | Close current tab |
| Ctrl+Tab | Next tab |
| Ctrl+Shift+Tab | Previous tab |
| Ctrl+1-9 | Switch to tab 1-9 |
| Middle-click tab | Close tab |

---

## Phase C: Hybrid (Tabs + Multi-Window)

**Goal**: Combine tabs with the ability to tear off tabs into new windows or open links in new windows.

**Complexity**: Very High (~500+ lines)

This phase combines Phase A and Phase B, adding:

1. **Tab tear-off**: Drag a tab out of the window to create a new window
2. **Tab merge**: Drag a tab from one window to another
3. **Window management**: Track all windows and their tab states

### Architecture

```rust
/// Represents a window (main or child)
struct AppWindow {
    id: ViewportId,
    dock_state: DockState<DocTab>,
    is_main: bool,
}

struct MarkdownApp {
    /// All windows (main + children)
    windows: Vec<AppWindow>,

    /// Global settings
    dark_mode: bool,
    zoom_level: f32,
    show_outline: bool,
}
```

### Key Features

1. **Drag tab to edge**: Creates new split in current window
2. **Drag tab outside window**: Creates new OS window with that tab
3. **Ctrl+Click link**: Opens in new window
4. **Shift+Click link**: Opens in new tab
5. **Regular click**: Opens in current tab

### Using egui_dock's Window Support

egui_dock has built-in support for undocking tabs into separate windows:

```rust
impl TabViewer for DocTabViewer<'_> {
    // ... existing implementations ...

    fn allowed_in_windows(&self, _tab: &mut Self::Tab) -> bool {
        true // Allow tabs to be undocked into separate windows
    }
}

impl eframe::App for MarkdownApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // DockArea automatically handles multiple windows when
        // allowed_in_windows returns true
        DockArea::new(&mut self.dock_state)
            .show(ctx, &mut self.tab_viewer);
    }
}
```

---

## References

### egui Documentation
- [egui GitHub Repository](https://github.com/emilk/egui)
- [egui API Docs](https://docs.rs/egui/latest/egui/)
- [Multiple Viewports (Context7)](https://context7.com/emilk/egui/llms.txt) - `ctx.show_viewport_deferred()`

### egui_commonmark
- [egui_commonmark Docs](https://docs.rs/egui_commonmark/latest/egui_commonmark/)
- Link hooks: `CommonMarkCache::add_link_hook()`, `get_link_hook()`

### Tab Libraries
- [egui_dock](https://docs.rs/egui_dock/latest/egui_dock/) - Full docking/tabbing support
- [egui_tabs](https://crates.io/crates/egui_tabs) - Simple tab widget
- [egui_tiles](https://docs.rs/egui_tiles) - Hierarchical tile/tab manager

### egui UI Patterns
- `ui.selectable_value()` - For custom tab selection
- `ui.selectable_label()` - For tab-like buttons
- `ScrollArea::horizontal()` - For scrollable tab bars

### Key egui Concepts
- **ViewportId**: Unique identifier for OS windows
- **ViewportBuilder**: Configure window properties
- **show_viewport_deferred**: Create child windows
- **Context persistence**: `ctx.data_mut()` / `ctx.data()`

---

## Implementation Checklist

### Phase D (Simple)
- [ ] Add link detection module
- [ ] Implement `register_link_hooks()` in `load_file()`
- [ ] Add navigation history (`history`, `history_index`)
- [ ] Implement `navigate_to()`, `navigate_back()`, `navigate_forward()`
- [ ] Add back/forward buttons to menu bar
- [ ] Add Alt+Left/Right keyboard shortcuts
- [ ] Handle clicked link hooks in `update()`

### Phase A (Multi-Window)
- [ ] Add `ChildWindow` struct
- [ ] Add `child_windows: Vec<ChildWindow>` to app
- [ ] Implement `open_in_new_window()`
- [ ] Render child windows with `show_viewport_deferred`
- [ ] Handle Ctrl+Click for new window
- [ ] Clean up closed windows

### Phase B (Tabs)
- [ ] Decide: Custom tabs vs egui_dock
- [ ] Create `Tab` struct with per-document state
- [ ] Refactor `MarkdownApp` to use tabs
- [ ] Implement tab bar UI
- [ ] Add tab keyboard shortcuts (Ctrl+T, Ctrl+W, etc.)
- [ ] Implement tab close, reorder, context menu

### Phase C (Hybrid)
- [ ] Combine Phase A and Phase B
- [ ] Enable tab tear-off to new windows
- [ ] Support tab merging between windows
- [ ] Unified link click handling (current/tab/window)
