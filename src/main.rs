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
use regex::Regex;
use serde::{Deserialize, Serialize};

const APP_KEY: &str = "md-viewer-state";
const MAX_WATCHER_RETRIES: u32 = 3;

/// Persisted state saved between sessions
#[derive(Serialize, Deserialize, Default)]
struct PersistedState {
    dark_mode: Option<bool>,
    last_file: Option<PathBuf>,
    zoom_level: Option<f32>,
    show_outline: Option<bool>,
    show_minimap: Option<bool>,
}

/// Represents a markdown header for the outline
#[derive(Clone)]
struct Header {
    level: u8,
    title: String,
    line_number: usize,
}

/// Content block type for minimap coloring
#[derive(Clone, Copy, PartialEq)]
enum BlockType {
    Header(u8),    // level 1-6
    CodeBlock,
    ListItem,
    Blockquote,
    Text,
    Blank,
}

/// A block of content for minimap rendering
#[derive(Clone)]
struct MinimapBlock {
    block_type: BlockType,
    start_line: usize,
    line_count: usize,
}

/// Result of parsing markdown headers
struct ParsedHeaders {
    /// Document title (first h1, if any)
    document_title: Option<String>,
    /// Outline headers (excludes the first h1)
    outline_headers: Vec<Header>,
}

/// A child window displaying a markdown file
struct ChildWindow {
    id: egui::ViewportId,
    path: PathBuf,
    content: String,
    cache: CommonMarkCache,
    open: bool,
    document_title: Option<String>,
    outline_headers: Vec<Header>,
    scroll_offset: f32,
    last_content_height: f32,
    pending_scroll_offset: Option<f32>,
    content_lines: usize,
    local_links: Vec<String>,
    history_back: Vec<PathBuf>,
    history_forward: Vec<PathBuf>,
}

impl ChildWindow {
    fn new(id: egui::ViewportId, path: PathBuf) -> Self {
        let content = fs::read_to_string(&path).unwrap_or_default();
        let parsed = parse_headers(&content);
        let local_links = parse_local_links(&content);
        let content_lines = content.lines().count();

        let mut cache = CommonMarkCache::default();
        for link in &local_links {
            cache.add_link_hook(link);
        }

        Self {
            id,
            path,
            content,
            cache,
            open: true,
            document_title: parsed.document_title,
            outline_headers: parsed.outline_headers,
            scroll_offset: 0.0,
            last_content_height: 0.0,
            pending_scroll_offset: None,
            content_lines,
            local_links,
            history_back: Vec::new(),
            history_forward: Vec::new(),
        }
    }

    fn window_title(&self) -> String {
        let filename = self.path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        format!("{} - Markdown Viewer", filename)
    }

    fn load_file(&mut self, path: &PathBuf) {
        if !path.exists() {
            return;
        }

        if let Ok(bytes) = fs::read(path) {
            let content = String::from_utf8_lossy(&bytes);
            self.content_lines = content.lines().count();
            self.content = content.into_owned();
            self.path = path.clone();
            self.cache = CommonMarkCache::default();

            let parsed = parse_headers(&self.content);
            self.document_title = parsed.document_title;
            self.outline_headers = parsed.outline_headers;

            self.local_links = parse_local_links(&self.content);
            for link in &self.local_links {
                self.cache.add_link_hook(link);
            }
        }
    }

    fn navigate_to_link(&mut self, link: &str) {
        if link.starts_with('#') {
            return;
        }

        let Some(current_dir) = self.path.parent() else {
            return;
        };

        let path_part = link.split('#').next().unwrap_or(link);
        let target_path = current_dir.join(path_part);

        let target_path = match target_path.canonicalize() {
            Ok(p) => p,
            Err(_) => return,
        };

        self.history_back.push(self.path.clone());
        self.history_forward.clear();
        self.load_file(&target_path);
    }

    fn check_link_hooks(&mut self) -> Option<String> {
        for link in &self.local_links {
            if let Some(true) = self.cache.get_link_hook(link) {
                return Some(link.clone());
            }
        }
        None
    }

    fn can_go_back(&self) -> bool {
        !self.history_back.is_empty()
    }

    fn can_go_forward(&self) -> bool {
        !self.history_forward.is_empty()
    }

    fn navigate_back(&mut self) {
        if let Some(prev_path) = self.history_back.pop() {
            self.history_forward.push(self.path.clone());
            self.load_file(&prev_path);
        }
    }

    fn navigate_forward(&mut self) {
        if let Some(next_path) = self.history_forward.pop() {
            self.history_back.push(self.path.clone());
            self.load_file(&next_path);
        }
    }
}

/// Parse local markdown file links and anchor links from content, skipping code blocks.
/// Returns a list of link destinations that should be handled internally (not opened in browser).
fn parse_local_links(content: &str) -> Vec<String> {
    // Match markdown links: [text](destination)
    let link_re = Regex::new(r"\[([^\]]*)\]\(([^)]+)\)").unwrap();
    let mut links = Vec::new();
    let mut in_code_block = false;

    for line in content.lines() {
        // Toggle code block state on fence lines
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        // Skip lines inside code blocks
        if in_code_block {
            continue;
        }

        // Find all links in the line
        for cap in link_re.captures_iter(line) {
            let destination = &cap[2];
            // Check if it's a local file link or anchor-only link
            if is_local_markdown_link(destination) || destination.starts_with('#') {
                links.push(destination.to_string());
            }
        }
    }

    links
}

/// Check if a link destination points to a local markdown file
fn is_local_markdown_link(destination: &str) -> bool {
    // Skip external links (http, https, mailto, etc.)
    if destination.starts_with("http://")
        || destination.starts_with("https://")
        || destination.starts_with("mailto:")
        || destination.starts_with("tel:")
        || destination.starts_with("ftp://")
        || destination.starts_with('#')  // Skip anchor-only links
    {
        return false;
    }

    // Remove anchor part if present (e.g., "file.md#heading" -> "file.md")
    let path_part = destination.split('#').next().unwrap_or(destination);

    // Check if it ends with a markdown extension
    let path = std::path::Path::new(path_part);
    path.extension()
        .map(|ext| {
            let ext = ext.to_string_lossy().to_lowercase();
            ext == "md" || ext == "markdown" || ext == "txt"
        })
        .unwrap_or(false)
}

/// Parse markdown content into blocks for minimap rendering.
/// Groups consecutive lines of the same type for efficient rendering.
fn parse_minimap_blocks(content: &str) -> Vec<MinimapBlock> {
    let header_re = Regex::new(r"^(#{1,6})\s+").unwrap();
    let mut blocks = Vec::new();
    let mut in_code_block = false;
    let mut current_type: Option<BlockType> = None;
    let mut current_start = 0;
    let mut current_count = 0;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();

        // Detect code block boundaries
        if trimmed.starts_with("```") {
            // Finish current block before code fence
            if let Some(bt) = current_type.take() {
                if current_count > 0 {
                    blocks.push(MinimapBlock {
                        block_type: bt,
                        start_line: current_start,
                        line_count: current_count,
                    });
                }
            }

            if in_code_block {
                // End of code block - this line is part of code
                in_code_block = false;
                current_type = None;
                current_count = 0;
            } else {
                // Start of code block
                in_code_block = true;
                current_type = Some(BlockType::CodeBlock);
                current_start = line_num;
                current_count = 1;
            }
            continue;
        }

        // Determine line type
        let line_type = if in_code_block {
            BlockType::CodeBlock
        } else if line.is_empty() {
            BlockType::Blank
        } else if let Some(caps) = header_re.captures(line) {
            BlockType::Header(caps[1].len() as u8)
        } else if trimmed.starts_with("- ")
            || trimmed.starts_with("* ")
            || trimmed.starts_with("+ ")
            || trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
                && trimmed.contains(". ")
        {
            BlockType::ListItem
        } else if trimmed.starts_with('>') {
            BlockType::Blockquote
        } else {
            BlockType::Text
        };

        // Accumulate same-type lines or start new block
        match current_type {
            Some(bt) if std::mem::discriminant(&bt) == std::mem::discriminant(&line_type) => {
                current_count += 1;
            }
            _ => {
                // Finish previous block
                if let Some(bt) = current_type.take() {
                    if current_count > 0 {
                        blocks.push(MinimapBlock {
                            block_type: bt,
                            start_line: current_start,
                            line_count: current_count,
                        });
                    }
                }
                // Start new block
                current_type = Some(line_type);
                current_start = line_num;
                current_count = 1;
            }
        }
    }

    // Finish last block
    if let Some(bt) = current_type {
        if current_count > 0 {
            blocks.push(MinimapBlock {
                block_type: bt,
                start_line: current_start,
                line_count: current_count,
            });
        }
    }

    blocks
}

/// Parse markdown headers from content, skipping code blocks.
/// Extracts the first h1 as document title and returns remaining headers for outline.
fn parse_headers(content: &str) -> ParsedHeaders {
    let re = Regex::new(r"^(#{1,6})\s+(.+)$").unwrap();
    let mut all_headers = Vec::new();
    let mut in_code_block = false;

    for (line_number, line) in content.lines().enumerate() {
        // Toggle code block state on fence lines
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        // Skip lines inside code blocks
        if in_code_block {
            continue;
        }

        // Check for header
        if let Some(caps) = re.captures(line) {
            all_headers.push(Header {
                level: caps[1].len() as u8,
                title: caps[2].trim().to_string(),
                line_number,
            });
        }
    }

    // Extract first h1 as document title, keep rest for outline
    let first_h1_idx = all_headers.iter().position(|h| h.level == 1);
    let document_title = first_h1_idx.map(|idx| all_headers[idx].title.clone());

    let outline_headers = all_headers
        .into_iter()
        .enumerate()
        .filter(|(idx, _)| Some(*idx) != first_h1_idx)
        .map(|(_, h)| h)
        .collect();

    ParsedHeaders {
        document_title,
        outline_headers,
    }
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
    // Zoom level (1.0 = 100%, range: 0.5 to 3.0)
    zoom_level: f32,
    // Outline state
    document_title: Option<String>,
    outline_headers: Vec<Header>,
    show_outline: bool,
    pending_scroll_offset: Option<f32>,
    last_content_height: f32,
    last_viewport_height: f32,
    // Navigation history for link following
    history_back: Vec<PathBuf>,
    history_forward: Vec<PathBuf>,
    // Cached local links for the current document
    local_links: Vec<String>,
    // Minimap state
    show_minimap: bool,
    minimap_blocks: Vec<MinimapBlock>,
    minimap_pending_scroll: Option<f32>,
    // Child windows for multi-window support
    child_windows: Vec<ChildWindow>,
    next_child_id: u64,
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

        // Use persisted zoom_level, or default to 1.0 (100%)
        let zoom_level = persisted.zoom_level.unwrap_or(1.0).clamp(0.5, 3.0);

        // Use persisted show_outline, default to true (visible by default)
        let show_outline = persisted.show_outline.unwrap_or(true);

        // Use persisted show_minimap, default to true (visible by default)
        let show_minimap = persisted.show_minimap.unwrap_or(true);

        let parsed = parse_headers(SAMPLE_MARKDOWN);
        let local_links = parse_local_links(SAMPLE_MARKDOWN);
        let minimap_blocks = parse_minimap_blocks(SAMPLE_MARKDOWN);
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
            zoom_level,
            document_title: parsed.document_title,
            outline_headers: parsed.outline_headers,
            show_outline,
            pending_scroll_offset: None,
            last_content_height: 0.0,
            last_viewport_height: 0.0,
            history_back: Vec::new(),
            history_forward: Vec::new(),
            local_links,
            show_minimap,
            minimap_blocks,
            minimap_pending_scroll: None,
            child_windows: Vec::new(),
            next_child_id: 0,
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
                let parsed = parse_headers(&self.content);
                self.document_title = parsed.document_title;
                self.outline_headers = parsed.outline_headers;

                // Parse and register local links for link hook handling
                self.local_links = parse_local_links(&self.content);
                for link in &self.local_links {
                    self.cache.add_link_hook(link);
                }

                // Parse minimap blocks for document overview
                self.minimap_blocks = parse_minimap_blocks(&self.content);

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

    /// Navigate to a local link, resolving it relative to the current file's directory
    fn navigate_to_link(&mut self, link: &str) {
        // Ignore anchor-only links (e.g., "#section") - just prevent browser from opening
        if link.starts_with('#') {
            log::debug!("Ignoring anchor-only link: {}", link);
            return;
        }

        let Some(current_file) = &self.current_file else {
            log::warn!("Cannot navigate: no current file");
            return;
        };

        let Some(current_dir) = current_file.parent() else {
            log::warn!("Cannot navigate: current file has no parent directory");
            return;
        };

        // Remove anchor part if present (e.g., "file.md#heading" -> "file.md")
        let path_part = link.split('#').next().unwrap_or(link);

        // Resolve the link relative to the current file's directory
        let target_path = current_dir.join(path_part);

        // Canonicalize to resolve .. and . components
        let target_path = match target_path.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                self.error_message = Some(format!("Cannot navigate to '{}': {}", link, e));
                log::error!("Failed to canonicalize path {:?}: {}", target_path, e);
                return;
            }
        };

        // Save current file to history before navigating
        self.history_back.push(current_file.clone());
        // Clear forward history on new navigation
        self.history_forward.clear();

        log::info!("Navigating to link: {:?}", target_path);
        self.load_file(&target_path);
    }

    /// Navigate back in history
    fn navigate_back(&mut self) {
        if let Some(prev_path) = self.history_back.pop() {
            // Save current file to forward history
            if let Some(current) = self.current_file.clone() {
                self.history_forward.push(current);
            }
            log::info!("Navigating back to: {:?}", prev_path);
            // Load without adding to history
            self.load_file_no_history(&prev_path);
        }
    }

    /// Navigate forward in history
    fn navigate_forward(&mut self) {
        if let Some(next_path) = self.history_forward.pop() {
            // Save current file to back history
            if let Some(current) = self.current_file.clone() {
                self.history_back.push(current);
            }
            log::info!("Navigating forward to: {:?}", next_path);
            // Load without adding to history
            self.load_file_no_history(&next_path);
        }
    }

    /// Load a file without modifying navigation history
    fn load_file_no_history(&mut self, path: &PathBuf) {
        // Store history temporarily
        let back = std::mem::take(&mut self.history_back);
        let forward = std::mem::take(&mut self.history_forward);

        self.load_file(path);

        // Restore history
        self.history_back = back;
        self.history_forward = forward;
    }

    /// Check link hooks after rendering and navigate if a link was clicked
    fn check_link_hooks(&mut self) -> Option<String> {
        for link in &self.local_links {
            if let Some(true) = self.cache.get_link_hook(link) {
                return Some(link.clone());
            }
        }
        None
    }

    fn can_go_back(&self) -> bool {
        !self.history_back.is_empty()
    }

    fn can_go_forward(&self) -> bool {
        !self.history_forward.is_empty()
    }

    /// Get the color for a minimap block based on its type and theme
    /// Colors are muted and subtle - headers stand out as navigation landmarks
    fn block_color(block_type: BlockType, dark_mode: bool) -> egui::Color32 {
        if dark_mode {
            match block_type {
                // Headers: amber/orange tones - primary landmarks
                BlockType::Header(1) => egui::Color32::from_rgb(0xF5, 0xA6, 0x23), // Bright amber
                BlockType::Header(2) => egui::Color32::from_rgb(0xD9, 0x8C, 0x2E), // Medium amber
                BlockType::Header(_) => egui::Color32::from_rgb(0xA8, 0x83, 0x4A), // Muted amber
                // Code: blue-tinted gray - distinct but not distracting
                BlockType::CodeBlock => egui::Color32::from_rgb(0x4A, 0x55, 0x68), // Slate blue-gray
                // Lists: subtle cool gray
                BlockType::ListItem => egui::Color32::from_rgb(0x64, 0x6E, 0x7A), // Cool gray
                // Blockquotes: muted teal
                BlockType::Blockquote => egui::Color32::from_rgb(0x5A, 0x7A, 0x7A), // Muted teal
                // Text: very subtle, almost background
                BlockType::Text => egui::Color32::from_rgb(0x45, 0x48, 0x4D), // Dark gray
                BlockType::Blank => egui::Color32::TRANSPARENT,
            }
        } else {
            match block_type {
                // Headers: amber/orange tones - primary landmarks
                BlockType::Header(1) => egui::Color32::from_rgb(0xD9, 0x73, 0x06), // Bright amber
                BlockType::Header(2) => egui::Color32::from_rgb(0xB4, 0x6A, 0x1C), // Medium amber
                BlockType::Header(_) => egui::Color32::from_rgb(0x92, 0x72, 0x3A), // Muted amber
                // Code: blue-tinted gray
                BlockType::CodeBlock => egui::Color32::from_rgb(0x64, 0x74, 0x8B), // Slate blue-gray
                // Lists: subtle warm gray
                BlockType::ListItem => egui::Color32::from_rgb(0x9C, 0xA3, 0xAF), // Warm gray
                // Blockquotes: muted sage
                BlockType::Blockquote => egui::Color32::from_rgb(0x7C, 0x9A, 0x8E), // Sage green
                // Text: subtle light gray
                BlockType::Text => egui::Color32::from_rgb(0xC8, 0xCC, 0xD0), // Light gray
                BlockType::Blank => egui::Color32::TRANSPARENT,
            }
        }
    }

    /// Resolve a link relative to the current file
    fn resolve_link(&self, link: &str) -> Option<PathBuf> {
        if link.starts_with('#') {
            return None;
        }

        let current_file = self.current_file.as_ref()?;
        let current_dir = current_file.parent()?;
        let path_part = link.split('#').next().unwrap_or(link);
        let target_path = current_dir.join(path_part);
        target_path.canonicalize().ok()
    }

    /// Open a link in a new window
    fn open_in_new_window(&mut self, link: &str) {
        let Some(target_path) = self.resolve_link(link) else {
            log::warn!("Could not resolve link for new window: {}", link);
            return;
        };

        // Skip if same as main window
        if self.current_file.as_ref() == Some(&target_path) {
            log::debug!("Link is same as main window, not opening new window");
            return;
        }

        // Check if already open in a child window - reactivate if so
        for window in &mut self.child_windows {
            if window.path == target_path {
                log::debug!("File already open in child window, reactivating");
                window.open = true;
                return;
            }
        }

        // Create new child window
        let id = egui::ViewportId::from_hash_of(format!("child_{}", self.next_child_id));
        self.next_child_id += 1;

        log::info!("Opening new window for: {:?}", target_path);
        self.child_windows.push(ChildWindow::new(id, target_path));
    }
}

impl eframe::App for MarkdownApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let state = PersistedState {
            dark_mode: Some(self.dark_mode),
            last_file: self.current_file.clone(),
            zoom_level: Some(self.zoom_level),
            show_outline: Some(self.show_outline),
            show_minimap: Some(self.show_minimap),
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

        // Apply evidence-based theme settings
        // Research: Off-white backgrounds (#F8F8F8) and dark gray text (#333333) for light mode
        // Material Design: #121212 background and 87% white (#E0E0E0) for dark mode
        // Avoids pure black on white (21:1 contrast causes halation for 47% with astigmatism)
        let visuals = if self.dark_mode {
            let mut v = egui::Visuals::dark();
            // Material Design dark mode: #121212 background, #E0E0E0 text (87% white)
            v.panel_fill = egui::Color32::from_rgb(0x12, 0x12, 0x12);
            v.window_fill = egui::Color32::from_rgb(0x12, 0x12, 0x12);
            v.extreme_bg_color = egui::Color32::from_rgb(0x1E, 0x1E, 0x1E); // Code blocks
            v.override_text_color = Some(egui::Color32::from_rgb(0xE0, 0xE0, 0xE0));
            v
        } else {
            let mut v = egui::Visuals::light();
            // Evidence-based light mode: #F8F8F8 background, #333333 text (~12:1 contrast)
            v.panel_fill = egui::Color32::from_rgb(0xF8, 0xF8, 0xF8);
            v.window_fill = egui::Color32::from_rgb(0xF8, 0xF8, 0xF8);
            v.extreme_bg_color = egui::Color32::from_rgb(0xF0, 0xF0, 0xF0); // Code blocks
            v.override_text_color = Some(egui::Color32::from_rgb(0x33, 0x33, 0x33));
            v
        };
        ctx.set_visuals(visuals);
        ctx.style_mut(|style| {
            style.url_in_tooltip = true;
            // Evidence-based font sizing: 16px minimum recommended (Rello et al., WCAG)
            // Heading max at 32px (2× base) per Major Third scale
            use egui::{FontId, TextStyle};
            style.text_styles.insert(TextStyle::Body, FontId::proportional(16.0));
            style.text_styles.insert(TextStyle::Heading, FontId::proportional(32.0));
            style.text_styles.insert(TextStyle::Small, FontId::proportional(13.0));
            style.text_styles.insert(TextStyle::Monospace, FontId::monospace(14.0));
        });

        // Apply zoom level
        ctx.set_zoom_factor(self.zoom_level);

        // Update window title
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.window_title()));

        // Handle keyboard shortcuts
        let mut open_dialog = false;
        let mut toggle_watch = false;
        let mut toggle_dark = false;
        let mut toggle_outline = false;
        let mut toggle_minimap = false;
        let mut quit_app = false;
        let mut zoom_delta: f32 = 0.0;
        let mut go_back = false;
        let mut go_forward = false;

        ctx.input(|i| {
            // Ctrl+O: Open file
            if i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::O) {
                open_dialog = true;
            }
            // Ctrl+Shift+O: Toggle outline
            if i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::O) {
                toggle_outline = true;
            }
            // Ctrl+M: Toggle minimap
            if i.modifiers.ctrl && i.key_pressed(egui::Key::M) {
                toggle_minimap = true;
            }
            // Ctrl+W: Toggle watch
            if i.modifiers.ctrl && i.key_pressed(egui::Key::W) {
                toggle_watch = true;
            }
            // Alt+Left: Go back in history
            if i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft) {
                go_back = true;
            }
            // Alt+Right: Go forward in history
            if i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight) {
                go_forward = true;
            }
            // Ctrl+D: Toggle dark mode
            if i.modifiers.ctrl && i.key_pressed(egui::Key::D) {
                toggle_dark = true;
            }
            // Ctrl+Q: Quit
            if i.modifiers.ctrl && i.key_pressed(egui::Key::Q) {
                quit_app = true;
            }
            // Ctrl+Plus or Ctrl+=: Zoom in
            if i.modifiers.ctrl && (i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals)) {
                zoom_delta = 0.1;
            }
            // Ctrl+Minus: Zoom out
            if i.modifiers.ctrl && i.key_pressed(egui::Key::Minus) {
                zoom_delta = -0.1;
            }
            // Ctrl+0: Reset zoom
            if i.modifiers.ctrl && i.key_pressed(egui::Key::Num0) {
                zoom_delta = 1.0 - self.zoom_level; // Reset to 1.0
            }
            // Ctrl + scroll wheel for zoom
            if i.modifiers.ctrl && i.raw_scroll_delta.y != 0.0 {
                zoom_delta = if i.raw_scroll_delta.y > 0.0 { 0.1 } else { -0.1 };
            }
        });

        // Apply zoom changes
        if zoom_delta != 0.0 {
            self.zoom_level = (self.zoom_level + zoom_delta).clamp(0.5, 3.0);
        }

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
        if toggle_outline {
            self.show_outline = !self.show_outline;
        }
        if toggle_minimap {
            self.show_minimap = !self.show_minimap;
        }
        if quit_app {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if go_back {
            self.navigate_back();
        }
        if go_forward {
            self.navigate_forward();
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

                ui.menu_button("Navigate", |ui| {
                    let can_back = self.can_go_back();
                    if ui.add_enabled(can_back, egui::Button::new("← Back").shortcut_text("Alt+←")).clicked() {
                        self.navigate_back();
                        ui.close();
                    }

                    let can_forward = self.can_go_forward();
                    if ui.add_enabled(can_forward, egui::Button::new("→ Forward").shortcut_text("Alt+→")).clicked() {
                        self.navigate_forward();
                        ui.close();
                    }
                });

                ui.menu_button("View", |ui| {
                    let theme_text = if self.dark_mode { "☀ Light Mode" } else { "🌙 Dark Mode" };
                    if ui.add(egui::Button::new(theme_text).shortcut_text("Ctrl+D")).clicked() {
                        self.dark_mode = !self.dark_mode;
                        ui.close();
                    }

                    let outline_text = if self.show_outline { "✓ Show Outline" } else { "Show Outline" };
                    if ui.add(egui::Button::new(outline_text).shortcut_text("Ctrl+Shift+O")).clicked() {
                        self.show_outline = !self.show_outline;
                        ui.close();
                    }

                    let minimap_text = if self.show_minimap { "✓ Show Minimap" } else { "Show Minimap" };
                    if ui.add(egui::Button::new(minimap_text).shortcut_text("Ctrl+M")).clicked() {
                        self.show_minimap = !self.show_minimap;
                        ui.close();
                    }

                    ui.separator();

                    if ui.add(egui::Button::new("Zoom In").shortcut_text("Ctrl++")).clicked() {
                        self.zoom_level = (self.zoom_level + 0.1).min(3.0);
                        ui.close();
                    }
                    if ui.add(egui::Button::new("Zoom Out").shortcut_text("Ctrl+-")).clicked() {
                        self.zoom_level = (self.zoom_level - 0.1).max(0.5);
                        ui.close();
                    }
                    if ui.add(egui::Button::new("Reset Zoom").shortcut_text("Ctrl+0")).clicked() {
                        self.zoom_level = 1.0;
                        ui.close();
                    }
                });

                // Show current file path on the right
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Show zoom level if not at 100%
                    if (self.zoom_level - 1.0).abs() > 0.01 {
                        ui.label(
                            egui::RichText::new(format!("{}%", (self.zoom_level * 100.0).round() as i32))
                                .small()
                                .color(ui.visuals().weak_text_color())
                        );
                        ui.separator();
                    }

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

        // Outline sidebar (right side)
        let mut clicked_header_line: Option<usize> = None;
        if self.show_outline && !self.outline_headers.is_empty() {
            // Check if any pointer is down (potential resize in progress)
            let is_dragging = ctx.input(|i| i.pointer.any_down());

            // Use document title if available, otherwise "Outline"
            let sidebar_title = self.document_title.as_deref().unwrap_or("Outline");

            egui::SidePanel::right("outline")
                .resizable(true)
                .default_width(200.0)
                .min_width(120.0)
                .max_width(400.0)
                .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin { left: 8, right: 8, top: 8, bottom: 8 }))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.set_max_width(ui.available_width());
                        ui.add(egui::Label::new(egui::RichText::new(sidebar_title).heading()).truncate());
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR | egui::scroll_area::ScrollSource::MOUSE_WHEEL)
                        .show(ui, |ui| {
                            for header in &self.outline_headers {
                                // Indent based on header level (h2 = 0, h3 = 1 indent, etc.)
                                let indent = (header.level.saturating_sub(2) as usize) * 12;
                                let prefix = " ".repeat(indent / 4); // Use spaces for indent

                                let display_text = if header.title.len() > 40 {
                                    format!("{}{}...", prefix, &header.title[..37])
                                } else {
                                    format!("{}{}", prefix, &header.title)
                                };

                                // Always use selectable_label for consistent spacing
                                // Only handle clicks when not dragging (to avoid accidental clicks during resize)
                                let response = ui.selectable_label(false, &display_text);
                                if !is_dragging && response.clicked() {
                                    clicked_header_line = Some(header.line_number);
                                }
                                ui.add_space(4.0);
                            }
                        });
                });
        }

        // Calculate scroll target if header was clicked
        if let Some(line_number) = clicked_header_line {
            if self.content_lines > 0 && self.last_content_height > 0.0 {
                // Calculate approximate scroll position based on line number ratio
                let ratio = line_number as f32 / self.content_lines as f32;
                self.pending_scroll_offset = Some(ratio * self.last_content_height);
            }
        }

        // Minimap sidebar (right side)
        if self.show_minimap && self.content_lines > 0 {
            let content_height = self.last_content_height;
            let scroll_offset = self.scroll_offset;
            let viewport_height = self.last_viewport_height;
            let content_lines = self.content_lines;
            let dark_mode = self.dark_mode;

            egui::SidePanel::right("minimap")
                .resizable(false)
                .exact_width(80.0)
                .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::same(2)))
                .show(ctx, |ui| {
                    let available_height = ui.available_height();

                    // Allocate painter for the minimap
                    let (response, painter) = ui.allocate_painter(
                        egui::vec2(76.0, available_height),
                        egui::Sense::click_and_drag(),
                    );

                    let minimap_rect = response.rect;

                    // Calculate scale: how many pixels per line
                    let scale = if content_height > 0.0 {
                        minimap_rect.height() / content_height
                    } else {
                        1.0
                    };

                    // Draw content blocks as colored rectangles
                    for block in &self.minimap_blocks {
                        let color = Self::block_color(block.block_type, dark_mode);

                        // Calculate position based on line number ratio
                        let y_ratio = block.start_line as f32 / content_lines as f32;
                        let height_ratio = block.line_count as f32 / content_lines as f32;

                        let block_y = minimap_rect.top() + y_ratio * minimap_rect.height();
                        let block_height = (height_ratio * minimap_rect.height()).max(2.0);

                        // Vary width based on block type for visual interest
                        let (x_offset, width) = match block.block_type {
                            BlockType::Header(_) => (2.0, 72.0),
                            BlockType::CodeBlock => (8.0, 60.0),
                            BlockType::ListItem => (12.0, 56.0),
                            BlockType::Blockquote => (6.0, 64.0),
                            BlockType::Text => (4.0, 68.0),
                            BlockType::Blank => continue, // Skip blank lines
                        };

                        let block_rect = egui::Rect::from_min_size(
                            egui::pos2(minimap_rect.left() + x_offset, block_y),
                            egui::vec2(width, block_height),
                        );
                        painter.rect_filled(block_rect, 1.0, color);
                    }

                    // Draw viewport indicator (semi-transparent rectangle showing visible area)
                    if content_height > 0.0 && viewport_height > 0.0 {
                        // Calculate viewport position and size in minimap coordinates
                        let viewport_y = minimap_rect.top() + scroll_offset * scale;
                        let viewport_h = (viewport_height * scale).min(minimap_rect.height());

                        let viewport_rect = egui::Rect::from_min_size(
                            egui::pos2(minimap_rect.left(), viewport_y),
                            egui::vec2(minimap_rect.width(), viewport_h),
                        );

                        // Draw viewport indicator
                        let indicator_color = if dark_mode {
                            egui::Color32::from_white_alpha(30)
                        } else {
                            egui::Color32::from_black_alpha(20)
                        };
                        painter.rect_filled(viewport_rect, 0.0, indicator_color);

                        // Draw border
                        let border_color = if dark_mode {
                            egui::Color32::from_white_alpha(60)
                        } else {
                            egui::Color32::from_black_alpha(40)
                        };
                        painter.rect_stroke(viewport_rect, 0.0, egui::Stroke::new(1.0, border_color), egui::StrokeKind::Outside);
                    }

                    // Handle click-to-navigate
                    if response.clicked() || response.dragged() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            let local_y = pos.y - minimap_rect.top();
                            let ratio = local_y / minimap_rect.height();

                            // Calculate target scroll position (center viewport on click)
                            let target_scroll = (ratio * content_height - viewport_height / 2.0)
                                .clamp(0.0, (content_height - viewport_height).max(0.0));

                            self.minimap_pending_scroll = Some(target_scroll);
                        }
                    }
                });
        }

        // Apply minimap scroll if set
        if let Some(offset) = self.minimap_pending_scroll.take() {
            self.pending_scroll_offset = Some(offset);
        }

        // Main content panel
        let mut clear_error = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(egui::Margin { left: 4, right: 8, top: 4, bottom: 4 }))
            .show(ctx, |ui| {
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
            let mut scroll_area = egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR | egui::scroll_area::ScrollSource::MOUSE_WHEEL);

            // Apply pending scroll offset if set
            if let Some(offset) = self.pending_scroll_offset.take() {
                scroll_area = scroll_area.vertical_scroll_offset(offset);
            }

            let scroll_output = scroll_area.show_viewport(ui, |ui, viewport| {
                // Track scroll position and viewport height for minimap sync
                self.scroll_offset = viewport.min.y;
                self.last_viewport_height = viewport.height();

                CommonMarkViewer::new()
                    .max_image_width(Some(800))
                    .default_width(Some(600)) // ~55-75 CPL at 16px (Dyson & Haselgrove 2001)
                    .indentation_spaces(2)
                    .show_alt_text_on_hover(true)
                    .syntax_theme_dark("base16-ocean.dark")
                    .syntax_theme_light("base16-ocean.light")
                    // Evidence-based typography (WCAG 2.1, peer-reviewed HCI research)
                    .line_height(1.5) // 1.5× line height per WCAG 2.1 SC 1.4.12
                    .code_line_height(1.3) // 1.3× for code (tighter, PPIG research)
                    .paragraph_spacing(2.0) // 2× font size per WCAG 1.4.12
                    .heading_spacing_above(2.0) // 2× font size before headings
                    .heading_spacing_below(0.75) // 0.75× (line-height × 0.5) after headings
                    .show(ui, &mut self.cache, &self.content);
            });

            // Store content height for scroll calculations
            self.last_content_height = scroll_output.content_size.y;

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

        // Check if any local link was clicked and navigate to it
        if let Some(clicked_link) = self.check_link_hooks() {
            let ctrl_held = ctx.input(|i| i.modifiers.ctrl || i.modifiers.command);
            if ctrl_held {
                self.open_in_new_window(&clicked_link);
            } else {
                self.navigate_to_link(&clicked_link);
            }
        }

        // Render child windows using immediate viewports
        let dark_mode = self.dark_mode;
        let zoom_level = self.zoom_level;
        let show_outline = self.show_outline;

        for child in &mut self.child_windows {
            if !child.open {
                continue;
            }

            let viewport_builder = egui::ViewportBuilder::default()
                .with_inner_size([900.0, 700.0])
                .with_min_inner_size([400.0, 300.0])
                .with_title(child.window_title());

            ctx.show_viewport_immediate(
                child.id,
                viewport_builder,
                |ctx, _class| {
                    // Apply theme settings (shared with main window)
                    let visuals = if dark_mode {
                        let mut v = egui::Visuals::dark();
                        v.panel_fill = egui::Color32::from_rgb(0x12, 0x12, 0x12);
                        v.window_fill = egui::Color32::from_rgb(0x12, 0x12, 0x12);
                        v.extreme_bg_color = egui::Color32::from_rgb(0x1E, 0x1E, 0x1E);
                        v.override_text_color = Some(egui::Color32::from_rgb(0xE0, 0xE0, 0xE0));
                        v
                    } else {
                        let mut v = egui::Visuals::light();
                        v.panel_fill = egui::Color32::from_rgb(0xF8, 0xF8, 0xF8);
                        v.window_fill = egui::Color32::from_rgb(0xF8, 0xF8, 0xF8);
                        v.extreme_bg_color = egui::Color32::from_rgb(0xF0, 0xF0, 0xF0);
                        v.override_text_color = Some(egui::Color32::from_rgb(0x33, 0x33, 0x33));
                        v
                    };
                    ctx.set_visuals(visuals);
                    ctx.style_mut(|style| {
                        style.url_in_tooltip = true;
                        use egui::{FontId, TextStyle};
                        style.text_styles.insert(TextStyle::Body, FontId::proportional(16.0));
                        style.text_styles.insert(TextStyle::Heading, FontId::proportional(32.0));
                        style.text_styles.insert(TextStyle::Small, FontId::proportional(13.0));
                        style.text_styles.insert(TextStyle::Monospace, FontId::monospace(14.0));
                    });
                    ctx.set_zoom_factor(zoom_level);

                    // Check for close request
                    if ctx.input(|i| i.viewport().close_requested()) {
                        child.open = false;
                    }

                    // Handle keyboard shortcuts for this window
                    let mut go_back = false;
                    let mut go_forward = false;
                    ctx.input(|i| {
                        if i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft) {
                            go_back = true;
                        }
                        if i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight) {
                            go_forward = true;
                        }
                    });
                    if go_back {
                        child.navigate_back();
                    }
                    if go_forward {
                        child.navigate_forward();
                    }

                    // Menu bar with navigation
                    egui::TopBottomPanel::top("child_menu_bar").show(ctx, |ui| {
                        egui::MenuBar::new().ui(ui, |ui| {
                            // Navigation buttons
                            let can_back = child.can_go_back();
                            if ui.add_enabled(can_back, egui::Button::new("←")).on_hover_text("Back (Alt+←)").clicked() {
                                child.navigate_back();
                            }

                            let can_forward = child.can_go_forward();
                            if ui.add_enabled(can_forward, egui::Button::new("→")).on_hover_text("Forward (Alt+→)").clicked() {
                                child.navigate_forward();
                            }

                            ui.separator();

                            // Show file path
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    egui::RichText::new(child.path.display().to_string())
                                        .small()
                                        .color(ui.visuals().weak_text_color())
                                );
                            });
                        });
                    });

                    // Outline sidebar (right side, if enabled)
                    let mut clicked_header_line: Option<usize> = None;
                    if show_outline && !child.outline_headers.is_empty() {
                        let is_dragging = ctx.input(|i| i.pointer.any_down());
                        let sidebar_title = child.document_title.as_deref().unwrap_or("Outline");

                        egui::SidePanel::right("child_outline")
                            .resizable(true)
                            .default_width(200.0)
                            .min_width(120.0)
                            .max_width(400.0)
                            .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin { left: 8, right: 8, top: 8, bottom: 8 }))
                            .show(ctx, |ui| {
                                ui.horizontal(|ui| {
                                    ui.set_max_width(ui.available_width());
                                    ui.add(egui::Label::new(egui::RichText::new(sidebar_title).heading()).truncate());
                                });
                                ui.separator();
                                egui::ScrollArea::vertical()
                                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                    .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR | egui::scroll_area::ScrollSource::MOUSE_WHEEL)
                                    .show(ui, |ui| {
                                        for header in &child.outline_headers {
                                            let indent = (header.level.saturating_sub(2) as usize) * 12;
                                            let prefix = " ".repeat(indent / 4);
                                            let display_text = if header.title.len() > 40 {
                                                format!("{}{}...", prefix, &header.title[..37])
                                            } else {
                                                format!("{}{}", prefix, &header.title)
                                            };
                                            let response = ui.selectable_label(false, &display_text);
                                            if !is_dragging && response.clicked() {
                                                clicked_header_line = Some(header.line_number);
                                            }
                                            ui.add_space(4.0);
                                        }
                                    });
                            });
                    }

                    // Calculate scroll target if header was clicked
                    if let Some(line_number) = clicked_header_line {
                        if child.content_lines > 0 && child.last_content_height > 0.0 {
                            let ratio = line_number as f32 / child.content_lines as f32;
                            child.pending_scroll_offset = Some(ratio * child.last_content_height);
                        }
                    }

                    // Main content panel
                    egui::CentralPanel::default()
                        .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(egui::Margin { left: 4, right: 8, top: 4, bottom: 4 }))
                        .show(ctx, |ui| {
                            let mut scroll_area = egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR | egui::scroll_area::ScrollSource::MOUSE_WHEEL);

                            if let Some(offset) = child.pending_scroll_offset.take() {
                                scroll_area = scroll_area.vertical_scroll_offset(offset);
                            }

                            let scroll_output = scroll_area.show_viewport(ui, |ui, viewport| {
                                child.scroll_offset = viewport.min.y;

                                CommonMarkViewer::new()
                                    .max_image_width(Some(800))
                                    .default_width(Some(600))
                                    .indentation_spaces(2)
                                    .show_alt_text_on_hover(true)
                                    .syntax_theme_dark("base16-ocean.dark")
                                    .syntax_theme_light("base16-ocean.light")
                                    .line_height(1.5)
                                    .code_line_height(1.3)
                                    .paragraph_spacing(2.0)
                                    .heading_spacing_above(2.0)
                                    .heading_spacing_below(0.75)
                                    .show(ui, &mut child.cache, &child.content);
                            });

                            child.last_content_height = scroll_output.content_size.y;
                        });

                    // Check if any local link was clicked (in-place navigation only)
                    if let Some(clicked_link) = child.check_link_hooks() {
                        child.navigate_to_link(&clicked_link);
                    }
                },
            );
        }

        // Clean up closed child windows
        self.child_windows.retain(|w| w.open);

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
