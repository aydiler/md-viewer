#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use clap::Parser;
use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind, Debouncer};
use notify::RecommendedWatcher;
use serde::{Deserialize, Serialize};

const APP_KEY: &str = "md-viewer-state";
const MAX_WATCHER_RETRIES: u32 = 3;

/// Persisted state saved between sessions
#[derive(Serialize, Deserialize, Default)]
struct PersistedState {
    dark_mode: Option<bool>,
    last_file: Option<PathBuf>,
}

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Parser, Debug)]
#[command(name = "md-viewer")]
#[command(about = "A lightweight markdown viewer", long_about = None)]
struct Args {
    /// Markdown file to open
    file: Option<PathBuf>,

    /// Enable live reload (watch for file changes)
    #[arg(short, long)]
    watch: bool,
}

fn main() -> eframe::Result<()> {
    env_logger::init();

    let args = Args::parse();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 700.0])
            .with_min_inner_size([400.0, 300.0])
            .with_title("Markdown Viewer")
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "md-viewer",
        options,
        Box::new(move |cc| Ok(Box::new(MarkdownApp::new(cc, args.file, args.watch)))),
    )
}

struct MarkdownApp {
    cache: CommonMarkCache,
    content: String,
    current_file: Option<PathBuf>,
    dark_mode: bool,
    watch_enabled: bool,
    error_message: Option<String>,
    is_dragging: bool,
    // File watcher state
    watcher: Option<Debouncer<RecommendedWatcher>>,
    watcher_rx: Option<Receiver<Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>>>,
    watcher_retry_count: u32,
    // Performance tracking
    content_lines: usize,
    scroll_offset: f32,
}

impl MarkdownApp {
    fn new(cc: &eframe::CreationContext<'_>, file: Option<PathBuf>, watch: bool) -> Self {
        // Load persisted state
        let persisted: PersistedState = cc
            .storage
            .and_then(|s| eframe::get_value(s, APP_KEY))
            .unwrap_or_default();

        // Use persisted dark_mode, or fall back to system default
        let dark_mode = persisted.dark_mode.unwrap_or_else(|| cc.egui_ctx.style().visuals.dark_mode);

        let mut app = Self {
            cache: CommonMarkCache::default(),
            content: SAMPLE_MARKDOWN.to_string(),
            current_file: None,
            dark_mode,
            watch_enabled: watch,
            error_message: None,
            is_dragging: false,
            watcher: None,
            watcher_rx: None,
            watcher_retry_count: 0,
            content_lines: SAMPLE_MARKDOWN.lines().count(),
            scroll_offset: 0.0,
        };

        // Determine which file to load: CLI argument takes priority, then persisted last file
        let file_to_load = file.or(persisted.last_file);

        if let Some(path) = file_to_load {
            app.load_file(&path);
            if watch {
                app.start_watching();
            }
        }

        app
    }

    fn load_file(&mut self, path: &PathBuf) {
        // Remember if we were watching
        let was_watching = self.watcher.is_some();

        // Stop current watcher before loading new file
        self.stop_watching();

        // First check if file exists
        if !path.exists() {
            self.error_message = Some(format!("File not found: {}", path.display()));
            log::error!("File not found: {:?}", path);
            return;
        }

        // Read file as bytes to handle invalid UTF-8 gracefully
        match fs::read(path) {
            Ok(bytes) => {
                // Convert to string with lossy UTF-8 conversion
                let content = String::from_utf8_lossy(&bytes);
                let had_invalid_utf8 = content.contains('\u{FFFD}');

                self.content_lines = content.lines().count();
                self.content = content.into_owned();
                self.current_file = Some(path.clone());
                self.cache = CommonMarkCache::default();

                if had_invalid_utf8 {
                    self.error_message = Some("Warning: File contains invalid UTF-8 characters (replaced with �)".to_string());
                    log::warn!("File {:?} contains invalid UTF-8", path);
                } else {
                    self.error_message = None;
                }

                // Restart watching if it was enabled
                if was_watching || self.watch_enabled {
                    self.start_watching();
                }
            }
            Err(e) => {
                let error_msg = match e.kind() {
                    std::io::ErrorKind::PermissionDenied => {
                        format!("Permission denied: {}", path.display())
                    }
                    std::io::ErrorKind::NotFound => {
                        format!("File not found: {}", path.display())
                    }
                    _ => format!("Failed to load file: {}", e),
                };
                self.error_message = Some(error_msg.clone());
                log::error!("Failed to load file {:?}: {}", path, e);
            }
        }
    }

    fn open_file_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Markdown", &["md", "markdown"])
            .add_filter("Text", &["txt"])
            .add_filter("All Files", &["*"])
            .pick_file()
        {
            self.load_file(&path);
        }
    }

    fn window_title(&self) -> String {
        match &self.current_file {
            Some(path) => {
                let filename = path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Unknown".to_string());
                format!("{} - Markdown Viewer", filename)
            }
            None => "Markdown Viewer".to_string(),
        }
    }

    fn is_markdown_file(path: &PathBuf) -> bool {
        path.extension()
            .map(|ext| {
                let ext = ext.to_string_lossy().to_lowercase();
                ext == "md" || ext == "markdown" || ext == "txt"
            })
            .unwrap_or(false)
    }

    fn start_watching(&mut self) {
        // Stop any existing watcher first
        self.stop_watching();

        let Some(file_path) = &self.current_file else {
            log::warn!("Cannot start watching: no file loaded");
            return;
        };

        let (tx, rx) = mpsc::channel();

        match new_debouncer(Duration::from_millis(200), tx) {
            Ok(mut debouncer) => {
                if let Err(e) = debouncer.watcher().watch(file_path, notify::RecursiveMode::NonRecursive) {
                    log::error!("Failed to watch file {:?}: {}", file_path, e);
                    self.error_message = Some(format!("Failed to watch file: {}", e));
                    return;
                }

                log::info!("Started watching file: {:?}", file_path);
                self.watcher = Some(debouncer);
                self.watcher_rx = Some(rx);
                self.watch_enabled = true;
                self.watcher_retry_count = 0;
            }
            Err(e) => {
                log::error!("Failed to create file watcher: {}", e);
                self.error_message = Some(format!("Failed to create file watcher: {}", e));
            }
        }
    }

    fn stop_watching(&mut self) {
        if self.watcher.is_some() {
            log::info!("Stopped watching file");
        }
        self.watcher = None;
        self.watcher_rx = None;
    }

    fn check_file_changes(&mut self) -> bool {
        let Some(rx) = &self.watcher_rx else {
            // If watching was enabled but watcher is gone, try to recover
            if self.watch_enabled && self.current_file.is_some() && self.watcher_retry_count < MAX_WATCHER_RETRIES {
                log::info!("Attempting to recover file watcher (attempt {})", self.watcher_retry_count + 1);
                self.watcher_retry_count += 1;
                self.start_watching();
            }
            return false;
        };

        let mut needs_reload = false;

        // Non-blocking check for file change events
        while let Ok(result) = rx.try_recv() {
            match result {
                Ok(events) => {
                    // Reset retry count on successful event
                    self.watcher_retry_count = 0;
                    for event in events {
                        if event.kind == DebouncedEventKind::Any {
                            log::debug!("File change detected: {:?}", event.path);
                            needs_reload = true;
                        }
                    }
                }
                Err(e) => {
                    log::error!("File watcher error: {}", e);
                    // Stop current watcher
                    self.watcher = None;
                    self.watcher_rx = None;

                    // Attempt recovery if under retry limit
                    if self.watcher_retry_count < MAX_WATCHER_RETRIES {
                        self.watcher_retry_count += 1;
                        log::info!("Attempting watcher recovery (attempt {})", self.watcher_retry_count);
                        self.start_watching();
                        if self.watcher.is_some() {
                            self.error_message = Some("File watcher recovered after error".to_string());
                        } else {
                            self.error_message = Some(format!("File watcher error (retry {}): {}", self.watcher_retry_count, e));
                        }
                    } else {
                        self.error_message = Some(format!("File watcher failed after {} retries: {}", MAX_WATCHER_RETRIES, e));
                        self.watch_enabled = false;
                    }
                    return false;
                }
            }
        }

        needs_reload
    }

    fn reload_current_file(&mut self) {
        if let Some(path) = self.current_file.clone() {
            log::info!("Reloading file: {:?}", path);
            self.load_file(&path);
        }
    }
}

impl eframe::App for MarkdownApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let state = PersistedState {
            dark_mode: Some(self.dark_mode),
            last_file: self.current_file.clone(),
        };
        eframe::set_value(storage, APP_KEY, &state);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for file changes and reload if needed
        if self.check_file_changes() {
            self.reload_current_file();
        }

        // Request periodic repaints when watching is enabled
        if self.watcher.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        // Apply theme and style settings
        ctx.set_visuals(if self.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        ctx.style_mut(|style| {
            style.url_in_tooltip = true;
        });

        // Update window title
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.window_title()));

        // Handle keyboard shortcuts
        let mut open_dialog = false;
        let mut toggle_watch = false;
        let mut toggle_dark = false;
        let mut quit_app = false;

        ctx.input(|i| {
            // Ctrl+O: Open file
            if i.modifiers.ctrl && i.key_pressed(egui::Key::O) {
                open_dialog = true;
            }
            // Ctrl+W: Toggle watch
            if i.modifiers.ctrl && i.key_pressed(egui::Key::W) {
                toggle_watch = true;
            }
            // Ctrl+D: Toggle dark mode
            if i.modifiers.ctrl && i.key_pressed(egui::Key::D) {
                toggle_dark = true;
            }
            // Ctrl+Q: Quit
            if i.modifiers.ctrl && i.key_pressed(egui::Key::Q) {
                quit_app = true;
            }
        });

        if open_dialog {
            self.open_file_dialog();
        }
        if toggle_watch && self.current_file.is_some() {
            if self.watcher.is_some() {
                self.stop_watching();
                self.watch_enabled = false;
            } else {
                self.start_watching();
            }
        }
        if toggle_dark {
            self.dark_mode = !self.dark_mode;
        }
        if quit_app {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Handle drag and drop
        self.is_dragging = false;
        ctx.input(|i| {
            if !i.raw.hovered_files.is_empty() {
                self.is_dragging = true;
            }

            for file in &i.raw.dropped_files {
                if let Some(path) = &file.path {
                    if Self::is_markdown_file(path) {
                        self.load_file(path);
                    } else {
                        self.error_message = Some(format!(
                            "Unsupported file type. Please drop a markdown file (.md, .markdown, .txt)"
                        ));
                    }
                }
            }
        });

        // Menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.add(egui::Button::new("Open...").shortcut_text("Ctrl+O")).clicked() {
                        self.open_file_dialog();
                        ui.close();
                    }

                    ui.separator();

                    let is_watching = self.watcher.is_some();
                    let watch_text = if is_watching { "✓ Watch File" } else { "Watch File" };
                    let watch_enabled = self.current_file.is_some();
                    if ui.add_enabled(watch_enabled, egui::Button::new(watch_text).shortcut_text("Ctrl+W")).clicked() {
                        if is_watching {
                            self.stop_watching();
                            self.watch_enabled = false;
                        } else {
                            self.start_watching();
                        }
                        ui.close();
                    }

                    ui.separator();

                    if ui.add(egui::Button::new("Quit").shortcut_text("Ctrl+Q")).clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        ui.close();
                    }
                });

                ui.menu_button("View", |ui| {
                    let theme_text = if self.dark_mode { "☀ Light Mode" } else { "🌙 Dark Mode" };
                    if ui.add(egui::Button::new(theme_text).shortcut_text("Ctrl+D")).clicked() {
                        self.dark_mode = !self.dark_mode;
                        ui.close();
                    }
                });

                // Show current file path on the right
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.watcher.is_some() {
                        ui.label(egui::RichText::new("● LIVE").color(egui::Color32::from_rgb(100, 200, 100)));
                        ui.separator();
                    }

                    if let Some(path) = &self.current_file {
                        ui.label(
                            egui::RichText::new(path.display().to_string())
                                .small()
                                .color(ui.visuals().weak_text_color())
                        );
                    }
                });
            });
        });

        // Main content panel
        let mut clear_error = false;
        egui::CentralPanel::default().show(ctx, |ui| {
            // Show error message if any
            if let Some(error) = &self.error_message {
                let error_text = error.clone();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("⚠").color(egui::Color32::from_rgb(255, 200, 100)));
                    ui.label(egui::RichText::new(&error_text).color(egui::Color32::from_rgb(255, 200, 100)));
                    if ui.small_button("✕").clicked() {
                        clear_error = true;
                    }
                });
                ui.separator();
            }

            // Use show_viewport for optimized rendering - egui will clip content
            // outside the visible area, reducing GPU work for large documents
            let scroll_output = egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_viewport(ui, |ui, viewport| {
                    // Track scroll position for potential future optimizations
                    self.scroll_offset = viewport.min.y;

                    CommonMarkViewer::new()
                        .max_image_width(Some(800))
                        .indentation_spaces(2)
                        .show_alt_text_on_hover(true)
                        .syntax_theme_dark("base16-ocean.dark")
                        .syntax_theme_light("base16-ocean.light")
                        .show(ui, &mut self.cache, &self.content);
                });

            // For very large documents (10000+ lines), show a performance hint
            if self.content_lines > 10000 && scroll_output.content_size.y > 50000.0 {
                ui.with_layout(egui::Layout::bottom_up(egui::Align::RIGHT), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} lines", self.content_lines))
                            .small()
                            .color(ui.visuals().weak_text_color())
                    );
                });
            }
        });
        if clear_error {
            self.error_message = None;
        }

        // Drag and drop overlay
        if self.is_dragging {
            let screen_rect = ctx.available_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drop_overlay"),
            ));

            painter.rect_filled(
                screen_rect,
                0.0,
                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 180),
            );

            painter.text(
                screen_rect.center(),
                egui::Align2::CENTER_CENTER,
                "Drop markdown file here",
                egui::FontId::proportional(24.0),
                egui::Color32::WHITE,
            );
        }
    }
}

const SAMPLE_MARKDOWN: &str = r#"# Markdown Viewer

A lightweight markdown viewer built with **egui** and **egui_commonmark**.

## Features

- Fast rendering at 60 FPS
- Syntax highlighting for code blocks
- GitHub Flavored Markdown support

## Tables

| Feature | Status | Notes |
|:--------|:------:|------:|
| Tables | ✓ | Left, center, right alignment |
| Task lists | ✓ | Interactive checkboxes |
| Strikethrough | ✓ | ~~deleted text~~ |
| Footnotes | ✓ | See below[^1] |

## Task List

- [x] Project setup
- [x] Core rendering
- [ ] File loading
- [ ] Live reload
- [ ] Theme toggle

## Text Formatting

Regular text with **bold**, *italic*, and ~~strikethrough~~.

You can also combine ***bold and italic*** together.

## Code Examples

Inline code: `cargo build --release`

```rust
fn main() {
    println!("Hello, markdown!");
}
```

```python
def greet(name: str) -> str:
    return f"Hello, {name}!"
```

```javascript
const sum = (a, b) => a + b;
console.log(sum(2, 3));
```

## Alerts

> [!NOTE]
> This is a note with helpful information.

> [!TIP]
> This is a tip for better usage.

> [!IMPORTANT]
> This is important information you should know.

> [!WARNING]
> This is a warning about potential issues.

> [!CAUTION]
> This is a caution about dangerous actions.

## Blockquotes

> This is a regular blockquote.
>
> It can span multiple paragraphs.

## Links

Visit [egui](https://github.com/emilk/egui) for more information.

## Footnotes

[^1]: This is a footnote with additional details.
"#;
