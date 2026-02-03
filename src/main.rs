#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use clap::Parser;
use eframe::egui;
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};
use notify::RecommendedWatcher;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind, Debouncer};
use regex::Regex;
use serde::{Deserialize, Serialize};

#[cfg(feature = "mcp")]
use egui_mcp_bridge::{McpBridge, McpUiExt};

const APP_KEY: &str = "md-viewer-state";

/// System font paths for fallback (Arch Linux / common Linux paths)
const SYSTEM_FONT_PATHS: &[(&str, &str)] = &[
    // Noto Sans for extended Latin, Greek, Cyrillic
    ("NotoSans", "/usr/share/fonts/noto/NotoSans-Regular.ttf"),
    ("NotoSans", "/usr/share/fonts/TTF/NotoSans-Regular.ttf"),
    (
        "NotoSans",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    ),
    // CJK fonts (Chinese, Japanese, Korean)
    (
        "NotoSansCJK",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    ),
    (
        "NotoSansCJK",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    ),
    (
        "NotoSansCJK",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    ),
    // Emoji
    ("NotoEmoji", "/usr/share/fonts/noto/NotoColorEmoji.ttf"),
    (
        "NotoEmoji",
        "/usr/share/fonts/noto-emoji/NotoColorEmoji.ttf",
    ),
    (
        "NotoEmoji",
        "/usr/share/fonts/google-noto-emoji/NotoColorEmoji.ttf",
    ),
    (
        "NotoEmoji",
        "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
    ),
    // Arabic
    (
        "NotoSansArabic",
        "/usr/share/fonts/noto/NotoSansArabic-Regular.ttf",
    ),
    (
        "NotoSansArabic",
        "/usr/share/fonts/TTF/NotoSansArabic-Regular.ttf",
    ),
    // Hebrew
    (
        "NotoSansHebrew",
        "/usr/share/fonts/noto/NotoSansHebrew-Regular.ttf",
    ),
    (
        "NotoSansHebrew",
        "/usr/share/fonts/TTF/NotoSansHebrew-Regular.ttf",
    ),
    // Devanagari (Hindi, Sanskrit)
    (
        "NotoSansDevanagari",
        "/usr/share/fonts/noto/NotoSansDevanagari-Regular.ttf",
    ),
    (
        "NotoSansDevanagari",
        "/usr/share/fonts/TTF/NotoSansDevanagari-Regular.ttf",
    ),
    // Thai
    (
        "NotoSansThai",
        "/usr/share/fonts/noto/NotoSansThai-Regular.ttf",
    ),
    (
        "NotoSansThai",
        "/usr/share/fonts/TTF/NotoSansThai-Regular.ttf",
    ),
    // Symbols (math, arrows, etc.)
    (
        "NotoSansSymbols",
        "/usr/share/fonts/noto/NotoSansSymbols-Regular.ttf",
    ),
    (
        "NotoSansSymbols",
        "/usr/share/fonts/TTF/NotoSansSymbols-Regular.ttf",
    ),
    (
        "NotoSansSymbols2",
        "/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf",
    ),
    (
        "NotoSansSymbols2",
        "/usr/share/fonts/TTF/NotoSansSymbols2-Regular.ttf",
    ),
    // DejaVu Sans - covers warning sign (U+26A0) and other misc symbols
    ("DejaVuSans", "/usr/share/fonts/TTF/DejaVuSans.ttf"),
    ("DejaVuSans", "/usr/share/fonts/dejavu/DejaVuSans.ttf"),
    (
        "DejaVuSans",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ),
];
const MAX_WATCHER_RETRIES: u32 = 3;
const FLASH_DURATION_MS: u64 = 600;

// Optimal widths for initial window sizing (based on typography research)
// Content: 600px optimal for 55-75 CPL readability
// Explorer: 200px default with 16px inner margins
// Outline: 200px default with 8px inner margins
// Plus ~16px for panel separators
const CONTENT_OPTIMAL_WIDTH: f32 = 600.0;
const EXPLORER_DEFAULT_WIDTH: f32 = 216.0; // 200 + 16 margins
const OUTLINE_DEFAULT_WIDTH: f32 = 208.0; // 200 + 8 margins
const PANEL_SEPARATORS: f32 = 16.0;
const OPTIMAL_WINDOW_HEIGHT: f32 = 750.0;

/// Persisted state saved between sessions
#[derive(Serialize, Deserialize, Default)]
struct PersistedState {
    dark_mode: Option<bool>,
    zoom_level: Option<f32>,
    show_outline: Option<bool>,
    open_tabs: Option<Vec<PathBuf>>,
    active_tab: Option<usize>,
    // File explorer state
    show_explorer: Option<bool>,
    explorer_root: Option<PathBuf>,
    expanded_dirs: Option<Vec<PathBuf>>,
    explorer_sort_order: Option<SortOrder>,
}

/// Represents a markdown header for the outline
#[derive(Clone)]
struct Header {
    level: u8,
    title: String,
    line_number: usize,
}

/// Result of parsing markdown headers
struct ParsedHeaders {
    /// Document title (first h1, if any)
    document_title: Option<String>,
    /// Outline headers (excludes the first h1)
    outline_headers: Vec<Header>,
}

/// Sort order for file explorer
#[derive(Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
enum SortOrder {
    #[default]
    NameAsc,
    NameDesc,
    DateAsc,
    DateDesc,
}

impl SortOrder {
    fn label(&self) -> &'static str {
        match self {
            SortOrder::NameAsc => "Name A-Z",
            SortOrder::NameDesc => "Name Z-A",
            SortOrder::DateAsc => "Oldest First",
            SortOrder::DateDesc => "Newest First",
        }
    }
}

/// A node in the file explorer tree
#[derive(Clone)]
enum FileTreeNode {
    File {
        path: PathBuf,
        name: String,
        modified: Option<std::time::SystemTime>,
    },
    Directory {
        path: PathBuf,
        name: String,
        modified: Option<std::time::SystemTime>,
        /// None = not yet loaded, Some = loaded (may be empty)
        children: Option<Vec<FileTreeNode>>,
    },
}

impl FileTreeNode {
    fn name(&self) -> &str {
        match self {
            FileTreeNode::File { name, .. } => name,
            FileTreeNode::Directory { name, .. } => name,
        }
    }

    fn modified(&self) -> Option<std::time::SystemTime> {
        match self {
            FileTreeNode::File { modified, .. } => *modified,
            FileTreeNode::Directory { modified, .. } => *modified,
        }
    }

    fn is_directory(&self) -> bool {
        matches!(self, FileTreeNode::Directory { .. })
    }
}

/// File explorer state
#[derive(Default)]
struct FileExplorer {
    root: Option<PathBuf>,
    tree: Vec<FileTreeNode>,
    expanded_dirs: HashSet<PathBuf>,
    sort_order: SortOrder,
}

impl FileExplorer {
    /// Scan a directory shallowly - only one level, subdirectories marked as unloaded
    fn scan_directory_shallow(path: &PathBuf, sort_order: SortOrder) -> Vec<FileTreeNode> {
        let Ok(entries) = fs::read_dir(path) else {
            return Vec::new();
        };

        let mut nodes: Vec<FileTreeNode> = Vec::new();

        for entry in entries.flatten() {
            let entry_path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden files
            if name.starts_with('.') {
                continue;
            }

            let modified = entry.metadata().ok().and_then(|m| m.modified().ok());

            if entry_path.is_dir() {
                // Check if directory might contain markdown files (has any non-hidden entries)
                // We don't recurse - just check if it's worth showing
                if Self::dir_might_have_markdown(&entry_path) {
                    nodes.push(FileTreeNode::Directory {
                        path: entry_path,
                        name,
                        modified,
                        children: None, // Lazy - not loaded yet
                    });
                }
            } else if Self::is_markdown_file(&entry_path) {
                nodes.push(FileTreeNode::File {
                    path: entry_path,
                    name,
                    modified,
                });
            }
        }

        Self::sort_nodes(&mut nodes, sort_order);
        nodes
    }

    /// Sort nodes according to the given sort order (directories always on top)
    fn sort_nodes(nodes: &mut [FileTreeNode], sort_order: SortOrder) {
        nodes.sort_by(|a, b| {
            // Directories always come first
            match (a.is_directory(), b.is_directory()) {
                (true, false) => return std::cmp::Ordering::Less,
                (false, true) => return std::cmp::Ordering::Greater,
                _ => {}
            }

            // Within the same type, sort by the selected criteria
            match sort_order {
                SortOrder::NameAsc => a.name().cmp(b.name()),
                SortOrder::NameDesc => b.name().cmp(a.name()),
                SortOrder::DateAsc => {
                    // Oldest first: None (unknown) sorts last
                    match (a.modified(), b.modified()) {
                        (Some(a_time), Some(b_time)) => a_time.cmp(&b_time),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => a.name().cmp(b.name()),
                    }
                }
                SortOrder::DateDesc => {
                    // Newest first: None (unknown) sorts last
                    match (a.modified(), b.modified()) {
                        (Some(a_time), Some(b_time)) => b_time.cmp(&a_time),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => a.name().cmp(b.name()),
                    }
                }
            }
        });
    }

    /// Quick check if a directory might contain markdown files
    /// Returns true if directory has any non-hidden entries (optimistic)
    fn dir_might_have_markdown(path: &PathBuf) -> bool {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with('.') {
                    return true;
                }
            }
        }
        false
    }

    fn is_markdown_file(path: &Path) -> bool {
        path.extension()
            .map(|ext| {
                let ext = ext.to_string_lossy().to_lowercase();
                ext == "md" || ext == "markdown" || ext == "txt"
            })
            .unwrap_or(false)
    }

    /// Set root directory and rescan (shallow)
    fn set_root(&mut self, path: PathBuf) {
        // Convert empty path to current directory
        let path = if path.as_os_str().is_empty() {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else {
            path
        };
        self.root = Some(path.clone());
        self.tree = Self::scan_directory_shallow(&path, self.sort_order);
    }

    /// Refresh the file tree (clears loaded state, rescans shallowly)
    fn refresh(&mut self) {
        if let Some(root) = &self.root.clone() {
            self.tree = Self::scan_directory_shallow(root, self.sort_order);
            // Re-load children for currently expanded directories
            let expanded: Vec<PathBuf> = self.expanded_dirs.iter().cloned().collect();
            for dir_path in expanded {
                self.load_children(&dir_path);
            }
        }
    }

    /// Load children for a specific directory (lazy loading)
    fn load_children(&mut self, dir_path: &PathBuf) {
        Self::load_children_in_tree(&mut self.tree, dir_path, self.sort_order);
    }

    /// Recursively find and load children for a directory in the tree
    fn load_children_in_tree(
        nodes: &mut [FileTreeNode],
        target_path: &PathBuf,
        sort_order: SortOrder,
    ) -> bool {
        for node in nodes.iter_mut() {
            if let FileTreeNode::Directory { path, children, .. } = node {
                if path == target_path {
                    // Found the target directory - load its children if not loaded
                    if children.is_none() {
                        *children = Some(Self::scan_directory_shallow(path, sort_order));
                    }
                    return true;
                }
                // Recurse into loaded children
                if let Some(ref mut child_nodes) = children {
                    if Self::load_children_in_tree(child_nodes, target_path, sort_order) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Toggle directory expansion (loads children if not yet loaded)
    fn toggle_expanded(&mut self, path: &PathBuf) {
        if self.expanded_dirs.contains(path) {
            self.expanded_dirs.remove(path);
        } else {
            // Load children before expanding if not yet loaded
            self.load_children(path);
            self.expanded_dirs.insert(path.clone());
        }
    }

    /// Check if a directory is expanded
    fn is_expanded(&self, path: &PathBuf) -> bool {
        self.expanded_dirs.contains(path)
    }

    /// Set sort order and re-sort the tree in place
    fn set_sort_order(&mut self, order: SortOrder) {
        if self.sort_order != order {
            self.sort_order = order;
            Self::resort_tree_recursive(&mut self.tree, order);
        }
    }

    /// Recursively re-sort all nodes in the tree
    fn resort_tree_recursive(nodes: &mut [FileTreeNode], sort_order: SortOrder) {
        Self::sort_nodes(nodes, sort_order);
        for node in nodes.iter_mut() {
            if let FileTreeNode::Directory {
                children: Some(ref mut child_nodes),
                ..
            } = node
            {
                Self::resort_tree_recursive(child_nodes, sort_order);
            }
        }
    }

    /// Get children for a directory by path (looks up in original tree, not clone)
    fn get_children(&self, target_path: &PathBuf) -> Option<&Vec<FileTreeNode>> {
        Self::find_children_in_tree(&self.tree, target_path)
    }

    /// Recursively find children for a directory in the tree
    fn find_children_in_tree<'a>(
        nodes: &'a [FileTreeNode],
        target_path: &PathBuf,
    ) -> Option<&'a Vec<FileTreeNode>> {
        for node in nodes {
            if let FileTreeNode::Directory { path, children, .. } = node {
                if path == target_path {
                    return children.as_ref();
                }
                // Recurse into loaded children
                if let Some(child_nodes) = children {
                    if let Some(found) = Self::find_children_in_tree(child_nodes, target_path) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    /// Expand all directories in the tree (loads all children recursively)
    fn expand_all(&mut self) {
        // First, recursively load all directories
        Self::load_all_children(&mut self.tree, self.sort_order);
        // Then collect all directory paths
        self.expanded_dirs = Self::collect_all_dirs(&self.tree);
    }

    /// Recursively load all unloaded directories
    fn load_all_children(nodes: &mut [FileTreeNode], sort_order: SortOrder) {
        for node in nodes.iter_mut() {
            if let FileTreeNode::Directory { path, children, .. } = node {
                // Load children if not yet loaded
                if children.is_none() {
                    *children = Some(Self::scan_directory_shallow(path, sort_order));
                }
                // Recurse into children
                if let Some(ref mut child_nodes) = children {
                    Self::load_all_children(child_nodes, sort_order);
                }
            }
        }
    }

    /// Collapse all directories in the tree
    fn collapse_all(&mut self) {
        self.expanded_dirs.clear();
    }

    /// Collect all directory paths from a tree recursively (only loaded directories)
    fn collect_all_dirs(nodes: &[FileTreeNode]) -> HashSet<PathBuf> {
        let mut dirs = HashSet::new();
        for node in nodes {
            if let FileTreeNode::Directory { path, children, .. } = node {
                dirs.insert(path.clone());
                // Only recurse into loaded children
                if let Some(child_nodes) = children {
                    dirs.extend(Self::collect_all_dirs(child_nodes));
                }
            }
        }
        dirs
    }
}

/// Per-tab state for a document
struct Tab {
    id: egui::Id,
    path: PathBuf,
    content: String,
    cache: CommonMarkCache,
    document_title: Option<String>,
    outline_headers: Vec<Header>,
    /// Set of header indices that are collapsed in the outline
    collapsed_headers: HashSet<usize>,
    scroll_offset: f32,
    pending_scroll_offset: Option<f32>,
    last_content_height: f32,
    content_lines: usize,
    local_links: Vec<String>,
    history_back: Vec<PathBuf>,
    history_forward: Vec<PathBuf>,
}

impl Tab {
    fn new(path: PathBuf) -> Self {
        // Canonicalize path for consistent comparison with watcher events
        let path = path.canonicalize().unwrap_or(path);
        let content = fs::read_to_string(&path).unwrap_or_default();
        let parsed = parse_headers(&content);
        let local_links = parse_local_links(&content);
        let content_lines = content.lines().count();

        let mut cache = CommonMarkCache::default();
        for link in &local_links {
            cache.add_link_hook(link);
        }

        Self {
            id: egui::Id::new(&path),
            path,
            content,
            cache,
            document_title: parsed.document_title,
            outline_headers: parsed.outline_headers,
            collapsed_headers: HashSet::new(),
            scroll_offset: 0.0,
            pending_scroll_offset: None,
            last_content_height: 0.0,
            content_lines,
            local_links,
            history_back: Vec::new(),
            history_forward: Vec::new(),
        }
    }

    fn from_sample() -> Self {
        let content = SAMPLE_MARKDOWN.to_string();
        let parsed = parse_headers(&content);
        let local_links = parse_local_links(&content);
        let content_lines = content.lines().count();

        let mut cache = CommonMarkCache::default();
        for link in &local_links {
            cache.add_link_hook(link);
        }

        Self {
            id: egui::Id::new("sample"),
            path: PathBuf::from("Welcome"),
            content,
            cache,
            document_title: parsed.document_title,
            outline_headers: parsed.outline_headers,
            collapsed_headers: HashSet::new(),
            scroll_offset: 0.0,
            pending_scroll_offset: None,
            last_content_height: 0.0,
            content_lines,
            local_links,
            history_back: Vec::new(),
            history_forward: Vec::new(),
        }
    }

    fn title(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string())
    }

    fn reload(&mut self) {
        if !self.path.exists() {
            return;
        }

        if let Ok(bytes) = fs::read(&self.path) {
            let content = String::from_utf8_lossy(&bytes);
            self.content_lines = content.lines().count();
            self.content = content.into_owned();
            self.cache = CommonMarkCache::default();

            let parsed = parse_headers(&self.content);
            self.document_title = parsed.document_title;
            self.outline_headers = parsed.outline_headers;
            self.collapsed_headers.clear();

            self.local_links = parse_local_links(&self.content);
            for link in &self.local_links {
                self.cache.add_link_hook(link);
            }
        }
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
            self.id = egui::Id::new(path);
            self.cache = CommonMarkCache::default();
            self.scroll_offset = 0.0;
            self.pending_scroll_offset = None;

            let parsed = parse_headers(&self.content);
            self.document_title = parsed.document_title;
            self.outline_headers = parsed.outline_headers;
            self.collapsed_headers.clear();

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

    fn check_link_hooks(&self) -> Option<String> {
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

    fn resolve_link(&self, link: &str) -> Option<PathBuf> {
        if link.starts_with('#') {
            return None;
        }

        let current_dir = self.path.parent()?;
        let path_part = link.split('#').next().unwrap_or(link);
        let target_path = current_dir.join(path_part);
        target_path.canonicalize().ok()
    }
}

/// Parse local markdown file links and anchor links from content, skipping code blocks.
fn parse_local_links(content: &str) -> Vec<String> {
    let link_re = Regex::new(r"\[([^\]]*)\]\(([^)]+)\)").unwrap();
    let mut links = Vec::new();
    let mut in_code_block = false;

    for line in content.lines() {
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            continue;
        }

        for cap in link_re.captures_iter(line) {
            let destination = &cap[2];
            if is_local_markdown_link(destination) || destination.starts_with('#') {
                links.push(destination.to_string());
            }
        }
    }

    links
}

/// Check if a link destination points to a local markdown file
fn is_local_markdown_link(destination: &str) -> bool {
    if destination.starts_with("http://")
        || destination.starts_with("https://")
        || destination.starts_with("mailto:")
        || destination.starts_with("tel:")
        || destination.starts_with("ftp://")
        || destination.starts_with('#')
    {
        return false;
    }

    let path_part = destination.split('#').next().unwrap_or(destination);
    let path = std::path::Path::new(path_part);
    path.extension()
        .map(|ext| {
            let ext = ext.to_string_lossy().to_lowercase();
            ext == "md" || ext == "markdown" || ext == "txt"
        })
        .unwrap_or(false)
}

/// Parse markdown headers from content, skipping code blocks.
fn parse_headers(content: &str) -> ParsedHeaders {
    let re = Regex::new(r"^(#{1,6})\s+(.+)$").unwrap();
    let mut all_headers = Vec::new();
    let mut in_code_block = false;

    for (line_number, line) in content.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            continue;
        }

        if let Some(caps) = re.captures(line) {
            all_headers.push(Header {
                level: caps[1].len() as u8,
                title: caps[2].trim().to_string(),
                line_number,
            });
        }
    }

    let document_title = all_headers
        .iter()
        .find(|h| h.level == 1)
        .map(|h| h.title.clone());
    let outline_headers = all_headers;

    ParsedHeaders {
        document_title,
        outline_headers,
    }
}

/// Check if header at `index` should be hidden because an ancestor is collapsed
fn header_is_hidden(headers: &[Header], index: usize, collapsed: &HashSet<usize>) -> bool {
    if index == 0 || index >= headers.len() {
        return false;
    }
    let mut search_level = headers[index].level;
    // Walk backwards to find ancestors
    for i in (0..index).rev() {
        let h = &headers[i];
        // Only consider headers with lower level than what we're searching for
        if h.level < search_level {
            // Found an ancestor
            if collapsed.contains(&i) {
                return true;
            }
            // This ancestor is not collapsed, but check its ancestors too
            // Update search_level to only look for even lower level headers
            search_level = h.level;
        }
        // Headers at same or higher level are siblings/cousins, skip them
    }
    false
}

/// Check if a header has any children (headers with higher level immediately following)
fn header_has_children(headers: &[Header], index: usize) -> bool {
    if index >= headers.len() {
        return false;
    }
    let current_level = headers[index].level;
    // Look at the next header
    if let Some(next) = headers.get(index + 1) {
        // A child has a higher level number (e.g., h3 is child of h2)
        next.level > current_level
    } else {
        false
    }
}

/// Check if any header in the list has children
fn any_header_has_children(headers: &[Header]) -> bool {
    for i in 0..headers.len() {
        if header_has_children(headers, i) {
            return true;
        }
    }
    false
}

/// Setup custom fonts with system font fallbacks for Unicode support.
/// Loads Noto fonts from system for extended character coverage.
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut loaded_fonts: HashSet<String> = HashSet::new();

    // Try to load each font from its possible paths
    for (font_name, font_path) in SYSTEM_FONT_PATHS {
        // Skip if we already loaded this font
        if loaded_fonts.contains(*font_name) {
            continue;
        }

        let path = Path::new(font_path);
        if path.exists() {
            match fs::read(path) {
                Ok(font_data) => {
                    log::info!("Loaded font fallback: {} from {}", font_name, font_path);

                    fonts.font_data.insert(
                        font_name.to_string(),
                        egui::FontData::from_owned(font_data).into(),
                    );

                    // Add to proportional family as fallback (after default fonts)
                    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                        family.push(font_name.to_string());
                    }

                    // Also add text fonts to monospace for code blocks with Unicode
                    // (but not emoji - that would look weird in code)
                    if !font_name.contains("Emoji") {
                        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                            family.push(font_name.to_string());
                        }
                    }

                    loaded_fonts.insert(font_name.to_string());
                }
                Err(e) => {
                    log::debug!("Failed to read font {}: {}", font_path, e);
                }
            }
        }
    }

    if loaded_fonts.is_empty() {
        log::warn!("No system fonts loaded. Unicode characters may show as red triangles.");
        log::warn!(
            "Install noto-fonts, noto-fonts-cjk, and noto-fonts-emoji for full Unicode support."
        );
    } else {
        log::info!(
            "Loaded {} font fallbacks for Unicode support",
            loaded_fonts.len()
        );
    }

    ctx.set_fonts(fonts);
}

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Parser, Debug)]
#[command(name = "md-viewer")]
#[command(about = "A lightweight markdown viewer", long_about = None)]
struct Args {
    /// Markdown file to open
    file: Option<PathBuf>,

    /// Disable live reload (watching is enabled by default)
    #[arg(long)]
    no_watch: bool,
}

fn main() -> eframe::Result<()> {
    env_logger::init();

    let args = Args::parse();

    // Calculate optimal window width assuming both sidebars are shown (the default)
    let optimal_width =
        CONTENT_OPTIMAL_WIDTH + EXPLORER_DEFAULT_WIDTH + OUTLINE_DEFAULT_WIDTH + PANEL_SEPARATORS;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([optimal_width, OPTIMAL_WINDOW_HEIGHT])
            .with_min_inner_size([400.0, 300.0])
            .with_title("Markdown Viewer")
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "md-viewer",
        options,
        Box::new(move |cc| Ok(Box::new(MarkdownApp::new(cc, args.file, !args.no_watch)))),
    )
}

struct MarkdownApp {
    tabs: Vec<Tab>,
    active_tab: usize,
    dark_mode: bool,
    zoom_level: f32,
    show_outline: bool,
    watch_enabled: bool,
    error_message: Option<String>,
    is_dragging: bool,
    // File watcher state
    watcher: Option<Debouncer<RecommendedWatcher>>,
    watcher_rx: Option<Receiver<Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>>>,
    watcher_retry_count: u32,
    // Set of paths being watched (individual tab files)
    watched_paths: HashSet<PathBuf>,
    // Explorer root being watched (uses recursive mode for directory tree updates)
    watched_explorer_root: Option<PathBuf>,
    // Tab being hovered for close button
    hovered_tab: Option<usize>,
    // File explorer state
    file_explorer: FileExplorer,
    show_explorer: bool,
    // Flash effect for updated files (path -> start time)
    flashing_paths: HashMap<PathBuf, Instant>,
    // True if running on virtual display (e.g., Xvfb :99) - limits frame rate
    is_virtual_display: bool,
    // MCP bridge for E2E testing
    #[cfg(feature = "mcp")]
    mcp_bridge: McpBridge,
}

impl MarkdownApp {
    fn new(cc: &eframe::CreationContext<'_>, file: Option<PathBuf>, watch: bool) -> Self {
        // Setup fonts with system font fallbacks for Unicode support
        setup_fonts(&cc.egui_ctx);

        // Load persisted state
        let persisted: PersistedState = cc
            .storage
            .and_then(|s| eframe::get_value(s, APP_KEY))
            .unwrap_or_default();

        let dark_mode = persisted
            .dark_mode
            .unwrap_or_else(|| cc.egui_ctx.style().visuals.dark_mode);
        let zoom_level = persisted.zoom_level.unwrap_or(1.0).clamp(0.5, 3.0);
        let show_outline = persisted.show_outline.unwrap_or(true);
        let show_explorer = persisted.show_explorer.unwrap_or(true);

        // Determine initial tabs
        let initial_tabs: Vec<Tab> = if let Some(ref path) = file {
            // CLI argument takes priority
            vec![Tab::new(path.clone())]
        } else if let Some(paths) = persisted.open_tabs {
            // Restore previous session tabs
            paths
                .into_iter()
                .filter(|p| p.exists())
                .map(Tab::new)
                .collect()
        } else {
            // Show sample content
            vec![Tab::from_sample()]
        };

        let tabs = if initial_tabs.is_empty() {
            vec![Tab::from_sample()]
        } else {
            initial_tabs
        };

        let active_tab = persisted
            .active_tab
            .unwrap_or(0)
            .min(tabs.len().saturating_sub(1));

        // Initialize file explorer
        let mut file_explorer = FileExplorer::default();

        // Restore sort order before scanning (so initial scan uses correct order)
        if let Some(sort_order) = persisted.explorer_sort_order {
            file_explorer.sort_order = sort_order;
        }

        // Determine explorer root:
        // 1. From CLI file path
        // 2. From persisted state
        // 3. From first open tab
        // 4. Current working directory as fallback
        let explorer_root = file
            .as_ref()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .or(persisted.explorer_root.filter(|p| p.exists()))
            .or_else(|| {
                tabs.first()
                    .and_then(|t| t.path.parent().map(|p| p.to_path_buf()))
            })
            .or_else(|| std::env::current_dir().ok());

        if let Some(root) = explorer_root {
            file_explorer.set_root(root);
        }

        // Restore expanded directories and load their children
        if let Some(expanded) = persisted.expanded_dirs {
            for dir_path in &expanded {
                file_explorer.load_children(dir_path);
            }
            file_explorer.expanded_dirs = expanded.into_iter().collect();
        }

        #[cfg(feature = "mcp")]
        let mcp_bridge = McpBridge::builder().port(9877).build();
        #[cfg(feature = "mcp")]
        log::info!("MCP bridge listening on port {}", mcp_bridge.port());

        // Detect virtual display (e.g., Xvfb :99) to limit frame rate
        // Virtual displays lack vsync, causing unlimited FPS and high CPU
        let is_virtual_display = std::env::var("DISPLAY")
            .map(|d| d != ":0" && d != ":0.0" && !d.is_empty())
            .unwrap_or(false);

        let mut app = Self {
            tabs,
            active_tab,
            dark_mode,
            zoom_level,
            show_outline,
            watch_enabled: watch,
            error_message: None,
            is_dragging: false,
            watcher: None,
            watcher_rx: None,
            watcher_retry_count: 0,
            watched_paths: HashSet::new(),
            watched_explorer_root: None,
            hovered_tab: None,
            file_explorer,
            show_explorer,
            flashing_paths: HashMap::new(),
            is_virtual_display,
            #[cfg(feature = "mcp")]
            mcp_bridge,
        };

        if watch {
            app.start_watching();
        }

        app
    }

    fn window_title(&self) -> String {
        if let Some(tab) = self.tabs.get(self.active_tab) {
            format!("{} - Markdown Viewer", tab.title())
        } else {
            "Markdown Viewer".to_string()
        }
    }

    fn is_markdown_file(path: &std::path::Path) -> bool {
        path.extension()
            .map(|ext| {
                let ext = ext.to_string_lossy().to_lowercase();
                ext == "md" || ext == "markdown" || ext == "txt"
            })
            .unwrap_or(false)
    }

    fn open_file_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Markdown", &["md", "markdown"])
            .add_filter("Text", &["txt"])
            .add_filter("All Files", &["*"])
            .pick_file()
        {
            self.open_in_new_tab(path);
        }
    }

    fn open_in_new_tab(&mut self, path: PathBuf) {
        // Canonicalize for consistent comparison with existing tabs
        let path = path.canonicalize().unwrap_or(path);
        // Check if already open
        if let Some(idx) = self.tabs.iter().position(|t| t.path == path) {
            self.active_tab = idx;
            return;
        }

        // Add new tab
        let tab = Tab::new(path);
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;

        // Update watcher if enabled
        if self.watch_enabled {
            self.update_watched_paths();
        }
    }

    fn close_tab(&mut self, idx: usize) {
        if self.tabs.len() <= 1 {
            // Don't close the last tab
            return;
        }

        self.tabs.remove(idx);

        // Adjust active tab index
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if self.active_tab > idx {
            self.active_tab -= 1;
        }

        // Update watcher
        if self.watch_enabled {
            self.update_watched_paths();
        }
    }

    fn close_active_tab(&mut self) {
        self.close_tab(self.active_tab);
    }

    fn next_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.active_tab = (self.active_tab + 1) % self.tabs.len();
        }
    }

    fn prev_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.active_tab = if self.active_tab == 0 {
                self.tabs.len() - 1
            } else {
                self.active_tab - 1
            };
        }
    }

    fn focus_tab(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.active_tab = idx;
        }
    }

    fn get_open_tab_paths(&self) -> Vec<PathBuf> {
        self.tabs
            .iter()
            .filter(|t| t.path.exists())
            .map(|t| t.path.clone())
            .collect()
    }

    fn start_watching(&mut self) {
        self.stop_watching();

        let tab_paths = self.get_open_tab_paths();
        let explorer_root = self.file_explorer.root.clone();

        // Need something to watch
        if tab_paths.is_empty() && explorer_root.is_none() {
            return;
        }

        let (tx, rx) = mpsc::channel();

        match new_debouncer(Duration::from_millis(200), tx) {
            Ok(mut debouncer) => {
                // Watch individual tab files (non-recursive)
                for path in &tab_paths {
                    if let Err(e) = debouncer
                        .watcher()
                        .watch(path, notify::RecursiveMode::NonRecursive)
                    {
                        log::error!("Failed to watch file {:?}: {}", path, e);
                    } else {
                        self.watched_paths.insert(path.clone());
                    }
                }

                // Watch explorer root directory (recursive) for tree updates
                if let Some(ref root) = explorer_root {
                    if let Err(e) = debouncer
                        .watcher()
                        .watch(root, notify::RecursiveMode::Recursive)
                    {
                        log::error!("Failed to watch explorer root {:?}: {}", root, e);
                    } else {
                        self.watched_explorer_root = Some(root.clone());
                        log::info!("Started watching explorer root: {:?}", root);
                    }
                }

                if !self.watched_paths.is_empty() || self.watched_explorer_root.is_some() {
                    log::info!(
                        "Started watching {} files + explorer root: {}",
                        self.watched_paths.len(),
                        self.watched_explorer_root.is_some()
                    );
                    self.watcher = Some(debouncer);
                    self.watcher_rx = Some(rx);
                    self.watch_enabled = true;
                    self.watcher_retry_count = 0;
                }
            }
            Err(e) => {
                log::error!("Failed to create file watcher: {}", e);
                self.error_message = Some(format!("Failed to create file watcher: {}", e));
            }
        }
    }

    fn stop_watching(&mut self) {
        if self.watcher.is_some() {
            log::info!("Stopped watching files");
        }
        self.watcher = None;
        self.watcher_rx = None;
        self.watched_paths.clear();
        self.watched_explorer_root = None;
    }

    fn update_watched_paths(&mut self) {
        if !self.watch_enabled {
            return;
        }

        let current_paths: HashSet<PathBuf> = self.get_open_tab_paths().into_iter().collect();

        // Add new paths
        if let Some(debouncer) = &mut self.watcher {
            for path in current_paths.difference(&self.watched_paths) {
                if let Err(e) = debouncer
                    .watcher()
                    .watch(path, notify::RecursiveMode::NonRecursive)
                {
                    log::error!("Failed to watch file {:?}: {}", path, e);
                }
            }

            // Remove old paths
            for path in self.watched_paths.difference(&current_paths) {
                let _ = debouncer.watcher().unwatch(path);
            }
        }

        self.watched_paths = current_paths;
    }

    fn check_file_changes(&mut self) -> Vec<PathBuf> {
        let Some(rx) = &self.watcher_rx else {
            // Attempt recovery if watching is enabled and there's something to watch
            // Check actual tabs and explorer root, not watched_paths (which may be empty after failure)
            let has_watchable = !self.tabs.is_empty() || self.file_explorer.root.is_some();
            if self.watch_enabled && has_watchable && self.watcher_retry_count < MAX_WATCHER_RETRIES
            {
                log::info!(
                    "Attempting to recover file watcher (attempt {})",
                    self.watcher_retry_count + 1
                );
                self.watcher_retry_count += 1;
                self.start_watching();
            }
            return Vec::new();
        };

        let mut changed_paths = Vec::new();

        while let Ok(result) = rx.try_recv() {
            match result {
                Ok(events) => {
                    self.watcher_retry_count = 0;
                    for event in events {
                        if event.kind == DebouncedEventKind::Any {
                            log::debug!("File change detected: {:?}", event.path);
                            changed_paths.push(event.path);
                        }
                    }
                }
                Err(e) => {
                    log::error!("File watcher error: {}", e);
                    self.watcher = None;
                    self.watcher_rx = None;

                    if self.watcher_retry_count < MAX_WATCHER_RETRIES {
                        self.watcher_retry_count += 1;
                        log::info!(
                            "Attempting watcher recovery (attempt {})",
                            self.watcher_retry_count
                        );
                        self.start_watching();
                    } else {
                        self.error_message = Some(format!(
                            "File watcher failed after {} retries: {}",
                            MAX_WATCHER_RETRIES, e
                        ));
                        self.watch_enabled = false;
                    }
                    return Vec::new();
                }
            }
        }

        changed_paths
    }

    fn reload_changed_tabs(&mut self, changed_paths: Vec<PathBuf>) {
        let now = Instant::now();
        let mut refresh_tree = false;

        for path in changed_paths {
            // Trigger flash effect for the changed file
            self.flashing_paths.insert(path.clone(), now);

            // Also flash parent directories up to the explorer root
            if let Some(root) = &self.file_explorer.root {
                // Check if the changed path is within the explorer root
                if path.starts_with(root) {
                    refresh_tree = true;
                }

                let mut current = path.parent();
                while let Some(parent) = current {
                    if parent.starts_with(root) || parent == root {
                        self.flashing_paths.insert(parent.to_path_buf(), now);
                    }
                    if parent == root {
                        break;
                    }
                    current = parent.parent();
                }
            }

            // Reload the tab content
            for tab in &mut self.tabs {
                if tab.path == path {
                    log::info!("Reloading tab: {:?}", path);
                    tab.reload();
                }
            }
        }

        // Refresh the file explorer tree if any changes were within the explorer root
        if refresh_tree {
            log::info!("Refreshing file explorer tree");
            self.file_explorer.refresh();
        }
    }

    /// Render the custom tab bar
    fn render_tab_bar(&mut self, ui: &mut egui::Ui) -> Option<usize> {
        let mut tab_to_close: Option<usize> = None;
        let mut new_active: Option<usize> = None;
        let mut close_others: Option<usize> = None;

        // Collect tab info first to avoid borrow issues
        let tab_info: Vec<(String, bool)> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(idx, tab)| (tab.title(), idx == self.active_tab))
            .collect();

        let tab_count = tab_info.len();
        let hovered_tab = self.hovered_tab;

        // Collect widget data for MCP registration (name, widget_type, rect, value)
        #[cfg(feature = "mcp")]
        let mut widget_data: Vec<(String, &'static str, egui::Rect, Option<String>)> = Vec::new();

        ui.horizontal(|ui| {
            // Scrollable tab area
            egui::ScrollArea::horizontal()
                .max_width(ui.available_width() - 30.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (idx, (title, is_active)) in tab_info.iter().enumerate() {
                            let is_hovered = hovered_tab == Some(idx);

                            // Tab frame
                            let tab_response = ui.horizontal(|ui| {
                                // Tab button
                                let text = egui::RichText::new(title);
                                let text = if *is_active { text.strong() } else { text };

                                let response = ui.selectable_label(*is_active, text);

                                // Collect tab widget data for MCP
                                #[cfg(feature = "mcp")]
                                widget_data.push((
                                    format!("Tab: {}", title),
                                    "tab",
                                    response.rect,
                                    Some(if *is_active {
                                        "active".to_string()
                                    } else {
                                        "".to_string()
                                    }),
                                ));

                                if response.clicked() {
                                    new_active = Some(idx);
                                }

                                // Middle-click to close
                                if response.middle_clicked() {
                                    tab_to_close = Some(idx);
                                }

                                // Close button (show on hover or active)
                                if *is_active || is_hovered {
                                    let close_btn = ui.small_button("×");

                                    // Collect close button widget data for MCP
                                    #[cfg(feature = "mcp")]
                                    widget_data.push((
                                        format!("Close Tab: {}", title),
                                        "button",
                                        close_btn.rect,
                                        None,
                                    ));

                                    if close_btn.clicked() {
                                        tab_to_close = Some(idx);
                                    }
                                }

                                // Context menu
                                response.context_menu(|ui| {
                                    if ui.button("Close").clicked() {
                                        tab_to_close = Some(idx);
                                        ui.close();
                                    }
                                    if tab_count > 1 && ui.button("Close Others").clicked() {
                                        close_others = Some(idx);
                                        ui.close();
                                    }
                                });

                                response
                            });

                            // Track hover state
                            if tab_response.response.hovered() {
                                self.hovered_tab = Some(idx);
                            }

                            ui.separator();
                        }
                    });
                });

            // New tab button
            let new_tab_btn = ui.button("+").on_hover_text("New Tab (Ctrl+T)");

            // Collect new tab button widget data for MCP
            #[cfg(feature = "mcp")]
            widget_data.push(("New Tab".to_string(), "button", new_tab_btn.rect, None));

            if new_tab_btn.clicked() {
                self.open_file_dialog();
            }
        });

        // Register all collected widgets with MCP bridge
        #[cfg(feature = "mcp")]
        for (name, widget_type, rect, value) in widget_data {
            self.mcp_bridge
                .register_widget_rect(&name, widget_type, rect, value.as_deref());
        }

        // Apply new active tab
        if let Some(idx) = new_active {
            self.active_tab = idx;
        }

        // Handle close others
        if let Some(keep_idx) = close_others {
            let kept = self.tabs.remove(keep_idx);
            self.tabs.clear();
            self.tabs.push(kept);
            self.active_tab = 0;
            if self.watch_enabled {
                self.update_watched_paths();
            }
            return None; // Don't close any tab, we already handled it
        }

        tab_to_close
    }

    /// Render the outline sidebar (right panel)
    /// Rendered at top level for proper layout space allocation
    fn render_outline(&mut self, ctx: &egui::Context) {
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };

        if !self.show_outline || tab.outline_headers.is_empty() {
            return;
        }

        // Handle outline header click (store index to access both title and line_number)
        let mut clicked_header_index: Option<usize> = None;

        // Collect widget data for MCP registration (name, widget_type, rect, value)
        #[cfg(feature = "mcp")]
        let mut widget_data: Vec<(String, &'static str, egui::Rect, Option<String>)> = Vec::new();

        let is_dragging = ctx.input(|i| i.pointer.any_down());

        egui::SidePanel::right("outline")
            .resizable(true)
            .default_width(200.0)
            .min_width(120.0)
            .max_width(400.0)
            .frame(
                egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin {
                    left: 8,
                    right: 0,
                    top: 8,
                    bottom: 0,
                }),
            )
            .show(ctx, |ui| {
                // Expand/Collapse All buttons (only if there are nested headers)
                let has_nested = any_header_has_children(&tab.outline_headers);
                if has_nested {
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        let expand_btn = ui.small_button("Expand All");

                        // Collect Expand All button for MCP
                        #[cfg(feature = "mcp")]
                        widget_data.push((
                            "Outline: Expand All".to_string(),
                            "button",
                            expand_btn.rect,
                            None,
                        ));

                        if expand_btn.clicked() {
                            tab.collapsed_headers.clear();
                        }

                        let collapse_btn = ui.small_button("Collapse All");

                        // Collect Collapse All button for MCP
                        #[cfg(feature = "mcp")]
                        widget_data.push((
                            "Outline: Collapse All".to_string(),
                            "button",
                            collapse_btn.rect,
                            None,
                        ));

                        if collapse_btn.clicked() {
                            for i in 0..tab.outline_headers.len() {
                                if header_has_children(&tab.outline_headers, i) {
                                    tab.collapsed_headers.insert(i);
                                }
                            }
                        }
                    });
                    ui.separator();
                }
                egui::ScrollArea::vertical()
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .id_salt("outline")
                    .show(ui, |ui| {
                        let mut toggle_index: Option<usize> = None;
                        // Only reserve space for fold indicators if any header has children
                        let show_fold_indicators = any_header_has_children(&tab.outline_headers);
                        for (idx, header) in tab.outline_headers.iter().enumerate() {
                            // Skip headers hidden by collapsed ancestors
                            if header_is_hidden(&tab.outline_headers, idx, &tab.collapsed_headers) {
                                continue;
                            }

                            let has_children = header_has_children(&tab.outline_headers, idx);
                            let is_collapsed = tab.collapsed_headers.contains(&idx);

                            // Indent based on header level (h2 = 0, h3 = 1 indent, etc.)
                            let indent = (header.level.saturating_sub(2) as usize) * 12;

                            ui.horizontal(|ui| {
                                // Add base indent
                                if indent > 0 {
                                    ui.add_space(indent as f32);
                                }

                                // Fold indicator (fixed width area for alignment)
                                // Only allocate space if any header has children
                                if show_fold_indicators {
                                    let (rect, response) = ui.allocate_exact_size(
                                        egui::vec2(20.0, 20.0),
                                        egui::Sense::click(),
                                    );
                                    if has_children {
                                        let indicator = if is_collapsed { "+" } else { "-" };
                                        let text_color = if response.hovered() {
                                            ui.visuals().strong_text_color()
                                        } else {
                                            ui.visuals().text_color()
                                        };
                                        ui.painter().text(
                                            rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            indicator,
                                            egui::FontId::monospace(16.0),
                                            text_color,
                                        );

                                        // Collect fold indicator for MCP
                                        #[cfg(feature = "mcp")]
                                        widget_data.push((
                                            format!("Toggle: {}", header.title),
                                            "button",
                                            rect,
                                            Some(if is_collapsed {
                                                "collapsed".to_string()
                                            } else {
                                                "expanded".to_string()
                                            }),
                                        ));

                                        if !is_dragging && response.clicked() {
                                            toggle_index = Some(idx);
                                        }
                                    }
                                }

                                // Header title
                                let display_text = if header.title.len() > 35 {
                                    format!("{}...", &header.title[..32])
                                } else {
                                    header.title.clone()
                                };

                                let response = ui.selectable_label(false, &display_text);

                                // Collect header for MCP
                                #[cfg(feature = "mcp")]
                                widget_data.push((
                                    format!("Header: {}", header.title),
                                    "header",
                                    response.rect,
                                    Some(format!("h{}", header.level)),
                                ));

                                if !is_dragging && response.clicked() {
                                    clicked_header_index = Some(idx);
                                }
                            });
                        }
                        // Apply toggle after iteration to avoid borrow issues
                        if let Some(idx) = toggle_index {
                            if tab.collapsed_headers.contains(&idx) {
                                tab.collapsed_headers.remove(&idx);
                            } else {
                                tab.collapsed_headers.insert(idx);
                            }
                        }
                    });
            });

        // Register all collected widgets with MCP bridge
        #[cfg(feature = "mcp")]
        for (name, widget_type, rect, value) in widget_data {
            self.mcp_bridge
                .register_widget_rect(&name, widget_type, rect, value.as_deref());
        }

        // Calculate scroll target if header was clicked
        if let Some(idx) = clicked_header_index {
            if let Some(header) = tab.outline_headers.get(idx) {
                // Try to get actual rendered position from cache first
                if let Some(y_pos) = tab.cache.get_header_position(&header.title) {
                    // Use exact position if available (header has been rendered)
                    tab.pending_scroll_offset = Some((y_pos - 50.0).max(0.0));
                } else if tab.last_content_height > 0.0 && tab.content_lines > 0 {
                    // Fallback: estimate position based on line number ratio
                    // This works for headers that haven't been rendered yet
                    let estimated_y = (header.line_number as f32 / tab.content_lines as f32)
                        * tab.last_content_height;
                    tab.pending_scroll_offset = Some((estimated_y - 50.0).max(0.0));
                }
            }
        }
    }

    /// Render the active tab's content
    fn render_tab_content(&mut self, ui: &mut egui::Ui, ctrl_held: bool) -> Option<PathBuf> {
        let mut open_in_new_tab: Option<PathBuf> = None;

        let tab = self.tabs.get_mut(self.active_tab)?;

        // Content area (no inner CentralPanel needed - we're already in one)
        // Left margin for breathing room, right margin prevents scrollbar/resize-handle overlap jitter
        egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: 8,
                right: 3,
                ..Default::default()
            })
            .show(ui, |ui| {
                // Capture scroll input for manual handling during selection
                let raw_scroll = ui.ctx().input(|i| i.raw_scroll_delta.y);
                let content_rect = ui.available_rect_before_wrap();

                let mut scroll_area = egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .scroll_source(egui::scroll_area::ScrollSource {
                        scroll_bar: true,
                        drag: false,
                        mouse_wheel: true,
                    })
                    .id_salt(tab.id);

                // Apply pending scroll offset from header clicks
                if let Some(offset) = tab.pending_scroll_offset.take() {
                    scroll_area = scroll_area.vertical_scroll_offset(offset);
                }

                let mut scroll_output = scroll_area.show_viewport(ui, |ui, viewport| {
                    tab.scroll_offset = viewport.min.y;
                    tab.cache.set_scroll_offset(viewport.min.y);

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
                        .show(ui, &mut tab.cache, &tab.content);
                });

                tab.last_content_height = scroll_output.content_size.y;

                // Manual scroll handling for mouse wheel during text selection
                let pointer_over_content = ui.ctx().input(|i| {
                    i.pointer
                        .hover_pos()
                        .is_some_and(|pos| content_rect.contains(pos))
                });
                if raw_scroll.abs() > 0.0 && pointer_over_content {
                    let current_offset = scroll_output.state.offset.y;
                    let max_scroll = (tab.last_content_height - content_rect.height()).max(0.0);
                    let new_offset = (current_offset - raw_scroll).clamp(0.0, max_scroll);

                    // Don't store at boundaries (can break selection)
                    let would_hit_top = new_offset < 0.5;
                    let would_hit_bottom = new_offset > max_scroll - 0.5;
                    let offset_changed = (new_offset - current_offset).abs() > 0.5;

                    if offset_changed && !would_hit_top && !would_hit_bottom {
                        scroll_output.state.offset.y = new_offset;
                        scroll_output.state.store(ui.ctx(), scroll_output.id);
                        ui.ctx().request_repaint();
                    }
                }

                // Request repaint during smooth scrolling
                if ui.ctx().input(|i| i.smooth_scroll_delta.length_sq() > 0.0) {
                    ui.ctx().request_repaint();
                }
            });

        // Check for clicked links
        if let Some(clicked_link) = tab.check_link_hooks() {
            if ctrl_held {
                // Open in new tab
                if let Some(target_path) = tab.resolve_link(&clicked_link) {
                    open_in_new_tab = Some(target_path);
                }
            } else {
                // Navigate in current tab
                tab.navigate_to_link(&clicked_link);
            }
        }

        open_in_new_tab
    }

    /// Render the file explorer sidebar
    /// Returns Some(path) if a file was clicked to open
    fn render_file_explorer(&mut self, ctx: &egui::Context) -> Option<PathBuf> {
        let mut file_to_open: Option<PathBuf> = None;

        if !self.show_explorer {
            return None;
        }

        egui::SidePanel::left("file_explorer")
            .resizable(true)
            .default_width(200.0)
            .min_width(150.0)
            .max_width(300.0)
            .frame(
                egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin {
                    left: 8,
                    right: 8,
                    top: 8,
                    bottom: 8,
                }),
            )
            .show(ctx, |ui| {
                // Header with folder name - OUTSIDE ScrollArea
                if let Some(root) = &self.file_explorer.root {
                    let folder_name = root
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| root.display().to_string());
                    ui.strong(&folder_name);
                } else {
                    ui.strong("No folder");
                }

                // Expand/collapse/refresh buttons - OUTSIDE ScrollArea
                ui.horizontal(|ui| {
                    let expand_btn = ui.small_button("⊞").on_hover_text("Expand all directories");
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Explorer: Expand All",
                        "button",
                        &expand_btn,
                        None,
                    );
                    if expand_btn.clicked() {
                        self.file_explorer.expand_all();
                    }

                    let collapse_btn = ui
                        .small_button("⊟")
                        .on_hover_text("Collapse all directories");
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Explorer: Collapse All",
                        "button",
                        &collapse_btn,
                        None,
                    );
                    if collapse_btn.clicked() {
                        self.file_explorer.collapse_all();
                    }

                    if ui.small_button("↻").on_hover_text("Refresh").clicked() {
                        self.file_explorer.refresh();
                    }
                });

                // Sort order dropdown
                ui.horizontal(|ui| {
                    ui.label("Sort:");
                    #[allow(unused_variables)]
                    let current_label = self.file_explorer.sort_order.label();
                    let combo_response = egui::ComboBox::from_id_salt("explorer_sort")
                        .selected_text(self.file_explorer.sort_order.label())
                        .show_ui(ui, |ui| {
                            for order in [
                                SortOrder::NameAsc,
                                SortOrder::NameDesc,
                                SortOrder::DateAsc,
                                SortOrder::DateDesc,
                            ] {
                                let is_selected = self.file_explorer.sort_order == order;
                                if ui.selectable_label(is_selected, order.label()).clicked() {
                                    self.file_explorer.set_sort_order(order);
                                }
                            }
                        });
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Explorer: Sort Order",
                        "combobox",
                        &combo_response.response,
                        Some(current_label),
                    );
                    #[cfg(not(feature = "mcp"))]
                    let _ = combo_response;
                });

                ui.separator();

                // File tree inside ScrollArea
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .id_salt("file_explorer")
                    .show(ui, |ui| {
                        // Collect open tab paths for highlighting
                        let open_paths: HashSet<PathBuf> = self
                            .tabs
                            .iter()
                            .filter(|t| t.path.exists())
                            .map(|t| t.path.clone())
                            .collect();

                        // Clone tree to avoid borrow issues
                        let tree = self.file_explorer.tree.clone();
                        for node in &tree {
                            if let Some(path) = self.render_tree_node(ui, node, 0, &open_paths) {
                                file_to_open = Some(path);
                            }
                        }
                    });
            });

        file_to_open
    }

    /// Calculate flash intensity for a path (0.0 = no flash, 1.0 = full flash)
    fn get_flash_intensity(&self, path: &PathBuf) -> f32 {
        // Try the path directly first
        let start_time = self.flashing_paths.get(path).or_else(|| {
            // Try canonical path if direct lookup fails
            path.canonicalize()
                .ok()
                .and_then(|canonical| self.flashing_paths.get(&canonical))
        });

        if let Some(start_time) = start_time {
            let elapsed = start_time.elapsed().as_millis() as u64;
            if elapsed < FLASH_DURATION_MS {
                // Fade out: 1.0 -> 0.0 over the duration
                1.0 - (elapsed as f32 / FLASH_DURATION_MS as f32)
            } else {
                0.0
            }
        } else {
            0.0
        }
    }

    /// Render a single node in the file tree (recursive)
    fn render_tree_node(
        &mut self,
        ui: &mut egui::Ui,
        node: &FileTreeNode,
        depth: usize,
        open_paths: &HashSet<PathBuf>,
    ) -> Option<PathBuf> {
        let mut file_to_open: Option<PathBuf> = None;
        let indent = depth * 16;

        match node {
            FileTreeNode::File { path, name, .. } => {
                // Calculate flash intensity for this file
                let flash_intensity = self.get_flash_intensity(path);
                let dark_mode = self.dark_mode;

                // Render file row and get its rect
                let row_response = ui.horizontal(|ui| {
                    ui.add_space(indent as f32);

                    // File icon
                    ui.label("📄");

                    // Truncate long filenames
                    let max_len = 25;
                    let display_name = if name.len() > max_len {
                        format!("{}...", &name[..max_len])
                    } else {
                        name.clone()
                    };

                    // Highlight if file is open in a tab
                    let is_open = open_paths.contains(path);
                    let text = if is_open {
                        egui::RichText::new(&display_name).strong()
                    } else {
                        egui::RichText::new(&display_name)
                    };

                    let response = ui.selectable_label(is_open, text);
                    #[cfg(feature = "mcp")]
                    {
                        let state_value = if is_open { "open" } else { "" };
                        self.mcp_bridge.register_widget(
                            &format!("File: {}", name),
                            "button",
                            &response,
                            Some(state_value),
                        );
                    }

                    // Show full name on hover if truncated
                    if name.len() > max_len {
                        response.clone().on_hover_text(name);
                    }
                    if response.clicked() {
                        file_to_open = Some(path.clone());
                    }

                    // Context menu for file actions
                    response.context_menu(|ui| {
                        if ui.button("Copy Contents").clicked() {
                            if let Ok(contents) = fs::read_to_string(path) {
                                ui.ctx().copy_text(contents);
                            }
                            ui.close();
                        }
                        if ui.button("Copy Path").clicked() {
                            ui.ctx().copy_text(path.display().to_string());
                            ui.close();
                        }
                        if ui.button("Copy File URI").clicked() {
                            ui.ctx().copy_text(format!("file://{}", path.display()));
                            ui.close();
                        }
                    });
                });

                // Paint flash overlay on top using the row rect
                if flash_intensity > 0.0 {
                    let alpha = ((flash_intensity * 180.0) as u8).max(60);
                    let flash_color = if dark_mode {
                        egui::Color32::from_rgba_unmultiplied(60, 200, 60, alpha)
                    } else {
                        egui::Color32::from_rgba_unmultiplied(80, 200, 80, alpha)
                    };
                    let rect = row_response.response.rect;
                    // Use debug_painter which draws on top of everything
                    ui.ctx().debug_painter().rect_filled(rect, 4.0, flash_color);
                }
            }
            FileTreeNode::Directory { path, name, .. } => {
                // Calculate flash intensity for this directory
                let flash_intensity = self.get_flash_intensity(path);
                let dark_mode = self.dark_mode;

                // Track if we should toggle this frame (detected in closure, applied after)
                let mut should_toggle = false;

                // Get current expansion state
                let is_expanded = self.file_explorer.is_expanded(path);

                // Render directory row
                let row_response = ui.horizontal(|ui| {
                    ui.add_space(indent as f32);

                    // Expand/collapse indicator
                    let indicator = if is_expanded { "v" } else { ">" };

                    #[cfg(feature = "mcp")]
                    let expand_btn = ui.mcp_small_button(format!("Toggle: {}", name), indicator);
                    #[cfg(not(feature = "mcp"))]
                    let expand_btn = ui.small_button(indicator);

                    if expand_btn.clicked() {
                        should_toggle = true;
                    }

                    // Folder icon
                    let folder_icon = if is_expanded { "📂" } else { "📁" };
                    ui.label(folder_icon);

                    // Truncate long folder names
                    let max_len = 22;
                    let display_name = if name.len() > max_len {
                        format!("{}...", &name[..max_len])
                    } else {
                        name.clone()
                    };

                    let response = ui.add(
                        egui::Label::new(&display_name)
                            .selectable(false)
                            .sense(egui::Sense::click()),
                    );
                    #[cfg(feature = "mcp")]
                    {
                        let state_value = if is_expanded { "expanded" } else { "collapsed" };
                        self.mcp_bridge.register_widget(
                            &format!("Directory: {}", name),
                            "button",
                            &response,
                            Some(state_value),
                        );
                    }

                    // Show full name on hover if truncated
                    if name.len() > max_len {
                        response.clone().on_hover_text(name);
                    }

                    // Click directory name to toggle expansion
                    if response.clicked() {
                        should_toggle = true;
                    }
                });

                // Toggle AFTER rendering row but BEFORE rendering children
                if should_toggle {
                    self.file_explorer.toggle_expanded(path);
                }

                // Paint flash overlay on top using the row rect
                if flash_intensity > 0.0 {
                    let alpha = ((flash_intensity * 180.0) as u8).max(60);
                    let flash_color = if dark_mode {
                        egui::Color32::from_rgba_unmultiplied(60, 200, 60, alpha)
                    } else {
                        egui::Color32::from_rgba_unmultiplied(80, 200, 80, alpha)
                    };
                    let rect = row_response.response.rect;
                    // Use debug_painter which draws on top of everything
                    ui.ctx().debug_painter().rect_filled(rect, 4.0, flash_color);
                }

                // Render children if expanded
                // Re-check expansion state (may have changed from click above)
                // Clone children from original tree (not the stale clone) to allow mutable self access
                if self.file_explorer.is_expanded(path) {
                    let child_nodes = self.file_explorer.get_children(path).cloned();
                    if let Some(child_nodes) = child_nodes {
                        for child in &child_nodes {
                            if let Some(opened_path) =
                                self.render_tree_node(ui, child, depth + 1, open_paths)
                            {
                                file_to_open = Some(opened_path);
                            }
                        }
                    }
                }
            }
        }

        file_to_open
    }
}

impl eframe::App for MarkdownApp {
    #[cfg(feature = "mcp")]
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.mcp_bridge.process_commands();
        self.mcp_bridge.inject_raw_input(raw_input);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let state = PersistedState {
            dark_mode: Some(self.dark_mode),
            zoom_level: Some(self.zoom_level),
            show_outline: Some(self.show_outline),
            open_tabs: Some(self.get_open_tab_paths()),
            active_tab: Some(self.active_tab),
            show_explorer: Some(self.show_explorer),
            explorer_root: self.file_explorer.root.clone(),
            expanded_dirs: Some(self.file_explorer.expanded_dirs.iter().cloned().collect()),
            explorer_sort_order: Some(self.file_explorer.sort_order),
        };
        eframe::set_value(storage, APP_KEY, &state);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Enable AccessKit for MCP bridge
        #[cfg(feature = "mcp")]
        {
            ctx.enable_accesskit();
            self.mcp_bridge.begin_frame();
            self.mcp_bridge.store_in_context(ctx); // Enable McpUiExt methods
                                                   // Poll for MCP commands at reasonable rate (prevents CPU spin on virtual displays)
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        // Check for file changes and reload affected tabs
        let changed_paths = self.check_file_changes();
        if !changed_paths.is_empty() {
            self.reload_changed_tabs(changed_paths);
        }

        // Clean up expired flash effects and request repaints while animating
        if !self.flashing_paths.is_empty() {
            let flash_duration = Duration::from_millis(FLASH_DURATION_MS);
            self.flashing_paths
                .retain(|_, start_time| start_time.elapsed() < flash_duration);

            // Request repaints while there are active flashes
            if !self.flashing_paths.is_empty() {
                ctx.request_repaint_after(Duration::from_millis(16)); // ~60fps for smooth animation
            }
        }

        // Request periodic repaints when watching is enabled
        // Use watch_enabled instead of watcher.is_some() to ensure recovery can happen
        if self.watch_enabled {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        // Limit frame rate on virtual displays (e.g., Xvfb) which lack vsync
        // request_repaint_after alone doesn't limit actual frame rate, so we sleep
        // This prevents 500%+ CPU usage during E2E testing
        if self.is_virtual_display {
            std::thread::sleep(Duration::from_millis(16)); // ~60 FPS cap
        }

        // Apply theme settings
        let visuals = if self.dark_mode {
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
            style
                .text_styles
                .insert(TextStyle::Body, FontId::proportional(16.0));
            style
                .text_styles
                .insert(TextStyle::Heading, FontId::proportional(32.0));
            style
                .text_styles
                .insert(TextStyle::Small, FontId::proportional(13.0));
            style
                .text_styles
                .insert(TextStyle::Monospace, FontId::monospace(14.0));

            // Smoother scroll animation
            style.animation_time = 0.15; // Faster UI animations (default: 0.1)
            style.scroll_animation.points_per_second = 1500.0; // Faster scroll (default: 1000)

            // Reduce resize grab radius to prevent overlap with adjacent scrollbars
            // Default is ~5.0, reducing to 2.0 prevents jitter between scrollbar and panel resize
            style.interaction.resize_grab_radius_side = 2.0;
        });

        // TEMP: Disable zoom for MCP testing debug
        // ctx.set_zoom_factor(self.zoom_level);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.window_title()));

        // Handle keyboard shortcuts
        let mut open_dialog = false;
        let mut toggle_watch = false;
        let mut toggle_dark = false;
        let mut toggle_outline = false;
        let mut toggle_explorer = false;
        let mut quit_app = false;
        let mut zoom_delta: f32 = 0.0;
        let mut go_back = false;
        let mut go_forward = false;
        let mut close_tab = false;
        let mut new_tab = false;
        let mut next_tab = false;
        let mut prev_tab = false;
        let mut focus_tab: Option<usize> = None;

        ctx.input(|i| {
            // Ctrl+O: Open file
            if i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::O) {
                open_dialog = true;
            }
            // Ctrl+Shift+O: Toggle outline
            if i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::O) {
                toggle_outline = true;
            }
            // Ctrl+Shift+E: Toggle file explorer
            if i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::E) {
                toggle_explorer = true;
            }
            // Ctrl+W: Close current tab
            if i.modifiers.ctrl && i.key_pressed(egui::Key::W) {
                close_tab = true;
            }
            // Ctrl+T: New tab (open file dialog)
            if i.modifiers.ctrl && i.key_pressed(egui::Key::T) {
                new_tab = true;
            }
            // Ctrl+Tab: Next tab
            if i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::Tab) {
                next_tab = true;
            }
            // Ctrl+Shift+Tab: Previous tab
            if i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::Tab) {
                prev_tab = true;
            }
            // Ctrl+1-9: Focus tab by index
            for (idx, key) in [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
                egui::Key::Num6,
                egui::Key::Num7,
                egui::Key::Num8,
                egui::Key::Num9,
            ]
            .iter()
            .enumerate()
            {
                if i.modifiers.ctrl && i.key_pressed(*key) {
                    focus_tab = Some(idx);
                }
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
            if i.modifiers.ctrl
                && (i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals))
            {
                zoom_delta = 0.1;
            }
            // Ctrl+Minus: Zoom out
            if i.modifiers.ctrl && i.key_pressed(egui::Key::Minus) {
                zoom_delta = -0.1;
            }
            // Ctrl+0: Reset zoom
            if i.modifiers.ctrl && i.key_pressed(egui::Key::Num0) {
                zoom_delta = 1.0 - self.zoom_level;
            }
            // Ctrl + scroll wheel for zoom
            if i.modifiers.ctrl && i.raw_scroll_delta.y != 0.0 {
                zoom_delta = if i.raw_scroll_delta.y > 0.0 {
                    0.1
                } else {
                    -0.1
                };
            }
            // F5: Toggle file watching
            if i.key_pressed(egui::Key::F5) {
                toggle_watch = true;
            }
        });

        // Apply zoom changes
        if zoom_delta != 0.0 {
            self.zoom_level = (self.zoom_level + zoom_delta).clamp(0.5, 3.0);
        }

        if open_dialog || new_tab {
            self.open_file_dialog();
        }
        if toggle_watch {
            if self.watcher.is_some() {
                self.stop_watching();
                self.watch_enabled = false;
            } else {
                self.watch_enabled = true;
                self.start_watching();
            }
        }
        if toggle_dark {
            self.dark_mode = !self.dark_mode;
        }
        if toggle_outline {
            self.show_outline = !self.show_outline;
        }
        if toggle_explorer {
            self.show_explorer = !self.show_explorer;
        }
        if quit_app {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if close_tab {
            self.close_active_tab();
        }
        if next_tab {
            self.next_tab();
        }
        if prev_tab {
            self.prev_tab();
        }
        if let Some(idx) = focus_tab {
            self.focus_tab(idx);
        }
        if go_back {
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.navigate_back();
            }
        }
        if go_forward {
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.navigate_forward();
            }
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
                        self.open_in_new_tab(path.clone());
                    } else {
                        self.error_message = Some(
                            "Unsupported file type. Please drop a markdown file (.md, .markdown, .txt)".to_string(),
                        );
                    }
                }
            }
        });

        // Get ctrl state for link handling
        let ctrl_held = ctx.input(|i| i.modifiers.ctrl || i.modifiers.command);

        // Menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui
                        .add(egui::Button::new("New Tab...").shortcut_text("Ctrl+T"))
                        .clicked()
                    {
                        self.open_file_dialog();
                        ui.close();
                    }

                    if ui
                        .add(egui::Button::new("Close Tab").shortcut_text("Ctrl+W"))
                        .clicked()
                    {
                        self.close_active_tab();
                        ui.close();
                    }

                    ui.separator();

                    let is_watching = self.watcher.is_some();
                    let watch_text = if is_watching {
                        "✓ Watch Files"
                    } else {
                        "Watch Files"
                    };
                    if ui
                        .add(egui::Button::new(watch_text).shortcut_text("F5"))
                        .clicked()
                    {
                        if is_watching {
                            self.stop_watching();
                            self.watch_enabled = false;
                        } else {
                            self.watch_enabled = true;
                            self.start_watching();
                        }
                        ui.close();
                    }

                    ui.separator();

                    if ui
                        .add(egui::Button::new("Quit").shortcut_text("Ctrl+Q"))
                        .clicked()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        ui.close();
                    }
                });

                ui.menu_button("Navigate", |ui| {
                    let can_back = self
                        .tabs
                        .get(self.active_tab)
                        .map(|t| t.can_go_back())
                        .unwrap_or(false);
                    if ui
                        .add_enabled(can_back, egui::Button::new("← Back").shortcut_text("Alt+←"))
                        .clicked()
                    {
                        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                            tab.navigate_back();
                        }
                        ui.close();
                    }

                    let can_forward = self
                        .tabs
                        .get(self.active_tab)
                        .map(|t| t.can_go_forward())
                        .unwrap_or(false);
                    if ui
                        .add_enabled(
                            can_forward,
                            egui::Button::new("→ Forward").shortcut_text("Alt+→"),
                        )
                        .clicked()
                    {
                        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                            tab.navigate_forward();
                        }
                        ui.close();
                    }
                });

                ui.menu_button("View", |ui| {
                    let theme_text = if self.dark_mode {
                        "☀ Light Mode"
                    } else {
                        "🌙 Dark Mode"
                    };
                    if ui
                        .add(egui::Button::new(theme_text).shortcut_text("Ctrl+D"))
                        .clicked()
                    {
                        self.dark_mode = !self.dark_mode;
                        ui.close();
                    }

                    let explorer_text = if self.show_explorer {
                        "✓ Show Explorer"
                    } else {
                        "Show Explorer"
                    };
                    if ui
                        .add(egui::Button::new(explorer_text).shortcut_text("Ctrl+Shift+E"))
                        .clicked()
                    {
                        self.show_explorer = !self.show_explorer;
                        ui.close();
                    }

                    let outline_text = if self.show_outline {
                        "✓ Show Outline"
                    } else {
                        "Show Outline"
                    };
                    if ui
                        .add(egui::Button::new(outline_text).shortcut_text("Ctrl+Shift+O"))
                        .clicked()
                    {
                        self.show_outline = !self.show_outline;
                        ui.close();
                    }

                    ui.separator();

                    if ui
                        .add(egui::Button::new("Zoom In").shortcut_text("Ctrl++"))
                        .clicked()
                    {
                        self.zoom_level = (self.zoom_level + 0.1).min(3.0);
                        ui.close();
                    }
                    if ui
                        .add(egui::Button::new("Zoom Out").shortcut_text("Ctrl+-"))
                        .clicked()
                    {
                        self.zoom_level = (self.zoom_level - 0.1).max(0.5);
                        ui.close();
                    }
                    if ui
                        .add(egui::Button::new("Reset Zoom").shortcut_text("Ctrl+0"))
                        .clicked()
                    {
                        self.zoom_level = 1.0;
                        ui.close();
                    }
                });

                // Navigation buttons (visible arrows for back/forward)
                ui.separator();
                let can_back = self
                    .tabs
                    .get(self.active_tab)
                    .map(|t| t.can_go_back())
                    .unwrap_or(false);
                let back_btn = ui.add_enabled(can_back, egui::Button::new("◀").small());
                #[cfg(feature = "mcp")]
                self.mcp_bridge
                    .register_widget("Navigate Back", "button", &back_btn, None);
                if back_btn.on_hover_text("Back (Alt+←)").clicked() {
                    go_back = true;
                }

                let can_forward = self
                    .tabs
                    .get(self.active_tab)
                    .map(|t| t.can_go_forward())
                    .unwrap_or(false);
                let forward_btn = ui.add_enabled(can_forward, egui::Button::new("▶").small());
                #[cfg(feature = "mcp")]
                self.mcp_bridge
                    .register_widget("Navigate Forward", "button", &forward_btn, None);
                if forward_btn.on_hover_text("Forward (Alt+→)").clicked() {
                    go_forward = true;
                }

                // Show status on the right
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Show zoom level if not at 100%
                    if (self.zoom_level - 1.0).abs() > 0.01 {
                        ui.label(
                            egui::RichText::new(format!(
                                "{}%",
                                (self.zoom_level * 100.0).round() as i32
                            ))
                            .small()
                            .color(ui.visuals().weak_text_color()),
                        );
                        ui.separator();
                    }

                    if self.watcher.is_some() {
                        ui.label(
                            egui::RichText::new("● LIVE")
                                .color(egui::Color32::from_rgb(100, 200, 100)),
                        );
                        ui.separator();
                    }

                    // Show current file path from active tab
                    if let Some(tab) = self.tabs.get(self.active_tab) {
                        if tab.path.exists() {
                            ui.label(
                                egui::RichText::new(tab.path.display().to_string())
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                            );
                        }
                    }
                });
            });
        });

        // Handle navigation button clicks (must be after menu bar UI)
        if go_back {
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.navigate_back();
            }
        }
        if go_forward {
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.navigate_forward();
            }
        }

        // Show error message if any
        let mut clear_error = false;
        if let Some(error) = &self.error_message {
            egui::TopBottomPanel::top("error_bar").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("⚠").color(egui::Color32::from_rgb(255, 200, 100)),
                    );
                    ui.label(
                        egui::RichText::new(error).color(egui::Color32::from_rgb(255, 200, 100)),
                    );
                    if ui.small_button("✕").clicked() {
                        clear_error = true;
                    }
                });
            });
        }
        if clear_error {
            self.error_message = None;
        }

        // Tab bar
        let mut tab_to_close: Option<usize> = None;
        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            tab_to_close = self.render_tab_bar(ui);
        });

        // Close tab if requested
        if let Some(idx) = tab_to_close {
            self.close_tab(idx);
        }

        // File explorer (left sidebar)
        let explorer_file = self.render_file_explorer(ctx);

        // Open file from explorer
        if let Some(path) = explorer_file {
            self.open_in_new_tab(path);
        }

        // Outline sidebar (right) - at top level for proper layout
        self.render_outline(ctx);

        // Main content area
        let mut open_in_new_tab: Option<PathBuf> = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(egui::Margin::ZERO))
            .show(ctx, |ui| {
                open_in_new_tab = self.render_tab_content(ui, ctrl_held);
            });

        // Open link in new tab if requested
        if let Some(path) = open_in_new_tab {
            self.open_in_new_tab(path);
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

        // Capture AccessKit output for MCP bridge
        #[cfg(feature = "mcp")]
        self.mcp_bridge.capture_output(ctx);
    }
}

const SAMPLE_MARKDOWN: &str = r#"# Markdown Viewer

A lightweight markdown viewer built with **egui** and **egui_commonmark**.

## Features

- Fast rendering at 60 FPS
- Syntax highlighting for code blocks
- GitHub Flavored Markdown support
- **Tab-based interface** - open multiple documents

## Keyboard Shortcuts

### Tab Management

| Shortcut | Action |
|----------|--------|
| Ctrl+T | New tab (open file) |
| Ctrl+W | Close current tab |
| Ctrl+Tab | Next tab |
| Ctrl+Shift+Tab | Previous tab |
| Ctrl+1-9 | Switch to tab 1-9 |

### Navigation

| Shortcut | Action |
|----------|--------|
| Ctrl+Click | Open link in new tab |
| Alt+← / Alt+→ | Navigate back/forward |

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
- [x] File loading
- [x] Live reload
- [x] Theme toggle
- [x] Custom tab system (no egui_dock)

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

## Alerts

> [!NOTE]
> This is a note with helpful information.

> [!TIP]
> This is a tip for better usage.

> [!IMPORTANT]
> This is important information you should know.

## Links

Visit [egui](https://github.com/emilk/egui) for more information.

## Footnotes

[^1]: This is a footnote with additional details.
"#;
