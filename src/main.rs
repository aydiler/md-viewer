#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, IsTerminal};
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStringExt;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use clap::Parser;
use eframe::egui;
use egui_commonmark_extended::{header_position_key, CommonMarkCache, CommonMarkViewer};
use notify::{PollWatcher, RecommendedWatcher};
use notify_debouncer_mini::{new_debouncer, new_debouncer_opt, DebouncedEventKind, Debouncer};
use regex::Regex;
use serde::{Deserialize, Serialize};

mod system_fonts;
use system_fonts::setup_fonts;

#[cfg(feature = "mcp")]
use egui_mcp_bridge::{McpBridge, McpUiExt};

const APP_KEY: &str = "md-viewer-state";

// Welcome page recent-files: how many to keep, and how many to show before "Show more".
const RECENT_FILES_CAP: usize = 20;
const RECENT_SHOWN: usize = 6;

const MAX_WATCHER_RETRIES: u32 = 3;
const FLASH_DURATION_MS: u64 = 600;

fn next_watcher_retry(current: u32) -> Option<u32> {
    (current < MAX_WATCHER_RETRIES).then(|| current + 1)
}

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
const SIDEBAR_MIN_WIDTH: f32 = 70.0;
const SIDEBAR_RESIZE_GRAB_RADIUS: f32 = 8.0;
const EXPLORER_RIGHT_RESIZE_GUTTER: i8 = 12;
const CONTENT_RIGHT_RESIZE_GUTTER: i8 = 10;

// Keyboard document scroll deltas are centralized so shortcut wiring and tests
// share the same line/page behavior.
const KEYBOARD_LINE_SCROLL_STEP: f32 = 48.0;
const KEYBOARD_PAGE_SCROLL_RATIO: f32 = 0.9;

// App-level keyboard scroll actions; kept private until shortcut wiring needs
// to pass them through `MarkdownApp::update`.
#[derive(Clone, Copy, Debug)]
enum KeyboardScrollAction {
    LineUp,
    LineDown,
    PageUp,
    PageDown,
}

// Compute the next clamped document scroll offset without touching egui state.
fn keyboard_scroll_target(
    current_offset: f32,
    viewport_height: f32,
    content_height: f32,
    action: KeyboardScrollAction,
) -> f32 {
    let page_step = viewport_height * KEYBOARD_PAGE_SCROLL_RATIO;
    let delta = match action {
        KeyboardScrollAction::LineUp => -KEYBOARD_LINE_SCROLL_STEP,
        KeyboardScrollAction::LineDown => KEYBOARD_LINE_SCROLL_STEP,
        KeyboardScrollAction::PageUp => -page_step,
        KeyboardScrollAction::PageDown => page_step,
    };
    let max_scroll = (content_height - viewport_height).max(0.0);

    (current_offset + delta).clamp(0.0, max_scroll)
}

fn content_default_width(full_width_content: bool) -> Option<usize> {
    if full_width_content {
        None
    } else {
        Some(CONTENT_OPTIMAL_WIDTH as usize)
    }
}

fn content_width_limit(full_width_content: bool, available_width: f32) -> usize {
    let available_width = available_width.max(0.0) as usize;
    content_default_width(full_width_content)
        .map_or(available_width, |preferred| preferred.min(available_width))
}

/// Check whether the desktop portal exposes the interface used by rfd's
/// Linux folder picker. Calling rfd without this interface fails like a user
/// cancellation, so checking first lets us distinguish that case and use a
/// local fallback without opening a second dialog after a real cancellation.
#[cfg(target_os = "linux")]
fn portal_file_chooser_available() -> bool {
    Command::new("gdbus")
        .args([
            "introspect",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|output| {
            output.status.success() && portal_introspection_has_file_chooser(&output.stdout)
        })
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn portal_introspection_has_file_chooser(stdout: &[u8]) -> bool {
    String::from_utf8_lossy(stdout).contains("org.freedesktop.portal.FileChooser")
}

#[cfg(target_os = "linux")]
fn path_from_picker_output(stdout: Vec<u8>) -> Option<PathBuf> {
    if stdout.is_empty() {
        None
    } else {
        // Python writes os.fsencode(path), so preserve arbitrary Unix path
        // bytes rather than requiring the selected directory to be UTF-8.
        Some(PathBuf::from(OsString::from_vec(stdout)))
    }
}

/// Use Python's Tk binding as a fallback file picker on Linux desktops where
/// xdg-desktop-portal is not available.
#[cfg(target_os = "linux")]
fn pick_file_with_tkinter(initial_dir: Option<&Path>) -> Result<Option<PathBuf>, String> {
    const SCRIPT: &str = r#"
import os
import sys
import tkinter as tk
from tkinter import filedialog

root = tk.Tk()
root.withdraw()
try:
    options = {
        "title": "Open Markdown File",
        "parent": root,
        "filetypes": [
            ("Markdown", ("*.md", "*.markdown")),
            ("Text", "*.txt"),
            ("All Files", "*"),
        ],
    }
    if len(sys.argv) > 1 and os.path.isdir(sys.argv[1]):
        options["initialdir"] = sys.argv[1]
    selected = filedialog.askopenfilename(**options)
    if selected:
        sys.stdout.buffer.write(os.fsencode(selected))
finally:
    root.destroy()
"#;

    let mut command = Command::new("python3");
    command
        .arg("-c")
        .arg(SCRIPT)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());
    if let Some(directory) = initial_dir.filter(|path| path.is_dir()) {
        command.arg(directory);
    }

    let output = command
        .output()
        .map_err(|error| format!("Tkinter file picker could not start: {error}"))?;

    if !output.status.success() {
        let details = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if details.is_empty() {
            "Tkinter file picker exited with an error".to_string()
        } else {
            format!("Tkinter file picker failed: {details}")
        });
    }

    let selected = path_from_picker_output(output.stdout);
    if let Some(path) = &selected {
        if !path.is_file() {
            return Err(format!(
                "File picker returned a file that does not exist: {}",
                path.display()
            ));
        }
    }
    Ok(selected)
}

fn pick_file(initial_dir: Option<&Path>) -> Result<Option<PathBuf>, String> {
    let mut dialog = rfd::FileDialog::new()
        .add_filter("Markdown", &["md", "markdown"])
        .add_filter("Text", &["txt"])
        .add_filter("All Files", &["*"]);
    if let Some(directory) = initial_dir.filter(|path| path.is_dir()) {
        dialog = dialog.set_directory(directory);
    }

    #[cfg(target_os = "linux")]
    {
        if portal_file_chooser_available() {
            return Ok(dialog.pick_file());
        }

        log::info!("XDG Desktop Portal FileChooser is unavailable; using the Tkinter file picker");
        pick_file_with_tkinter(initial_dir)
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(dialog.pick_file())
    }
}

/// Use Python's Tk binding as a fallback folder picker on Linux desktops where
/// xdg-desktop-portal is not available. The initial directory is passed as a
/// process argument, never interpolated into Python.
#[cfg(target_os = "linux")]
fn pick_folder_with_tkinter(initial_dir: Option<&Path>) -> Result<Option<PathBuf>, String> {
    const SCRIPT: &str = r#"
import os
import sys
import tkinter as tk
from tkinter import filedialog

root = tk.Tk()
root.withdraw()
try:
    options = {"title": "Open Folder", "mustexist": True, "parent": root}
    if len(sys.argv) > 1 and os.path.isdir(sys.argv[1]):
        options["initialdir"] = sys.argv[1]
    selected = filedialog.askdirectory(**options)
    if selected:
        sys.stdout.buffer.write(os.fsencode(selected))
finally:
    root.destroy()
"#;

    let mut command = Command::new("python3");
    command
        .arg("-c")
        .arg(SCRIPT)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());
    if let Some(directory) = initial_dir.filter(|path| path.is_dir()) {
        command.arg(directory);
    }

    let output = command
        .output()
        .map_err(|error| format!("Tkinter folder picker could not start: {error}"))?;

    if !output.status.success() {
        let details = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if details.is_empty() {
            "Tkinter folder picker exited with an error".to_string()
        } else {
            format!("Tkinter folder picker failed: {details}")
        });
    }

    let selected = path_from_picker_output(output.stdout);
    if let Some(path) = &selected {
        if !path.is_dir() {
            return Err(format!(
                "Folder picker returned a directory that does not exist: {}",
                path.display()
            ));
        }
    }
    Ok(selected)
}

fn pick_folder(initial_dir: Option<&Path>) -> Result<Option<PathBuf>, String> {
    let mut dialog = rfd::FileDialog::new();
    if let Some(directory) = initial_dir.filter(|path| path.is_dir()) {
        dialog = dialog.set_directory(directory);
    }

    #[cfg(target_os = "linux")]
    {
        if portal_file_chooser_available() {
            return Ok(dialog.pick_folder());
        }

        log::info!(
            "XDG Desktop Portal FileChooser is unavailable; using the Tkinter folder picker"
        );
        pick_folder_with_tkinter(initial_dir)
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(dialog.pick_folder())
    }
}

/// A recently opened file, for the welcome page's "Recent" list.
#[derive(Serialize, Deserialize, Clone)]
struct RecentEntry {
    path: PathBuf,
    /// Seconds since the Unix epoch when the file was last opened.
    last_opened: u64,
}

/// Current time in seconds since the Unix epoch (0 if the clock is before epoch).
fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Push a path to the front of the recent list, deduped and capped (most-recent first).
fn push_recent(list: &mut Vec<RecentEntry>, path: &Path, now: u64) {
    list.retain(|e| e.path.as_path() != path);
    list.insert(
        0,
        RecentEntry {
            path: path.to_path_buf(),
            last_opened: now,
        },
    );
    list.truncate(RECENT_FILES_CAP);
}

/// Format an epoch-seconds timestamp as a short relative time ("3m ago").
fn format_relative_time(epoch_secs: u64, now: u64) -> String {
    let diff = now.saturating_sub(epoch_secs);
    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86_400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 172_800 {
        "yesterday".to_string()
    } else if diff < 604_800 {
        format!("{}d ago", diff / 86_400)
    } else if diff < 2_592_000 {
        format!("{}w ago", diff / 604_800)
    } else {
        format!("{}mo ago", diff / 2_592_000)
    }
}

/// Persisted state saved between sessions
#[derive(Serialize, Deserialize, Default)]
struct PersistedState {
    dark_mode: Option<bool>,
    zoom_level: Option<f32>,
    show_outline: Option<bool>,
    full_width_content: Option<bool>,
    math_scale: Option<f32>,
    /// Selection/highlight background color override, stored as premultiplied
    /// RGBA bytes (`Color32`'s canonical representation — see `Color32::r/g/b/a`
    /// and `Color32::from_rgba_premultiplied`). `None` = current theme default.
    highlight_color: Option<[u8; 4]>,
    /// Hyperlink text color override, same premultiplied-RGBA encoding as
    /// `highlight_color`. `None` = current theme default.
    link_color: Option<[u8; 4]>,
    open_tabs: Option<Vec<PathBuf>>,
    active_tab: Option<usize>,
    // File explorer state
    show_explorer: Option<bool>,
    explorer_root: Option<PathBuf>,
    expanded_dirs: Option<Vec<PathBuf>>,
    explorer_sort_order: Option<SortOrder>,
    explorer_width: Option<f32>,
    outline_width: Option<f32>,
    recent_files: Option<Vec<RecentEntry>>,
}

fn restored_sidebar_width(width: Option<f32>, default: f32) -> f32 {
    width
        .filter(|width| width.is_finite())
        .map(|width| width.max(SIDEBAR_MIN_WIDTH))
        .unwrap_or(default)
}

/// Represents a markdown header for the outline
#[derive(Clone)]
struct Header {
    level: u8,
    title: String,
    /// Byte offset of the heading's Start event. The renderer records the
    /// position under the same source-stable key, independent of formatting.
    source_start: usize,
    line_number: usize,
}

/// Result of parsing markdown headers
struct ParsedHeaders {
    /// Document title (first h1, if any)
    document_title: Option<String>,
    /// Outline headers (excludes the first h1)
    outline_headers: Vec<Header>,
}

/// A single match in a tab's content, identified by byte range and 1-based line number
#[derive(Clone, Debug, PartialEq, Eq)]
struct SearchMatch {
    byte_start: usize,
    byte_end: usize,
    line_number: usize,
}

/// Per-frame return from `render_search_bar`
#[derive(Default)]
struct SearchBarOutcome {
    close_requested: bool,
    prev_clicked: bool,
    next_clicked: bool,
}

/// App-level search state — only one find bar is visible at a time
#[derive(Default)]
struct SearchState {
    is_open: bool,
    query: String,
    /// Shadow copy of `query` used to detect changes across frames
    last_query: String,
    /// Tab index the cached matches were built for; `None` forces rebuild
    last_tab: Option<usize>,
    /// Set after Ctrl+F so the text input is focused next frame
    focus_requested: bool,
    /// Index into the active tab's `search_matches`
    active_match_index: usize,
}

/// Action from file explorer interaction
#[derive(Default)]
struct ExplorerAction {
    /// File selected with the primary mouse button. The caller decides whether
    /// it replaces the active document or opens a tab from the Ctrl modifier.
    file_to_open: Option<PathBuf>,
    /// File to close (middle-click on open file)
    file_to_close: Option<PathBuf>,
    /// Directory to toggle expansion (deferred to avoid clone)
    dir_to_toggle: Option<PathBuf>,
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

struct ExplorerScanResult {
    root: PathBuf,
    sort_order: SortOrder,
    tree: Vec<FileTreeNode>,
}

/// File explorer state
#[derive(Default)]
struct FileExplorer {
    root: Option<PathBuf>,
    tree: Vec<FileTreeNode>,
    expanded_dirs: HashSet<PathBuf>,
    sort_order: SortOrder,
    /// Receiver for asynchronous root-directory scan results.
    pending_scan: Option<Receiver<ExplorerScanResult>>,
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
                // Show all directories - let users expand what they want
                // (Avoids O(n×m) scanning during initial directory scan)
                nodes.push(FileTreeNode::Directory {
                    path: entry_path,
                    name,
                    modified,
                    children: None, // Lazy - not loaded yet
                });
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

    fn is_markdown_file(path: &Path) -> bool {
        path.extension()
            .map(|ext| {
                let ext = ext.to_string_lossy().to_lowercase();
                ext == "md" || ext == "markdown" || ext == "txt"
            })
            .unwrap_or(false)
    }

    /// Set root directory and start a shallow background scan.
    fn set_root(&mut self, path: PathBuf) {
        // Convert empty path to current directory
        let path = if path.as_os_str().is_empty() {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else {
            path
        };
        self.root = Some(path.clone());
        if !self.start_root_scan(path.clone(), "explorer-scan") {
            self.tree = Self::scan_directory_shallow(&path, self.sort_order);
            Self::restore_expanded_children(&mut self.tree, &self.expanded_dirs, self.sort_order);
        }
    }

    fn start_root_scan(&mut self, path: PathBuf, thread_name: &str) -> bool {
        let sort_order = self.sort_order;
        let expanded_dirs = self.expanded_dirs.clone();
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name(thread_name.to_owned())
            .spawn(move || {
                let mut tree = Self::scan_directory_shallow(&path, sort_order);
                Self::restore_expanded_children(&mut tree, &expanded_dirs, sort_order);
                let _ = tx.send(ExplorerScanResult {
                    root: path,
                    sort_order,
                    tree,
                });
            });
        if spawned.is_ok() {
            self.pending_scan = Some(rx);
            true
        } else {
            false
        }
    }

    /// Check if a background scan completed and apply results
    fn poll_pending_scan(&mut self) -> bool {
        let received = self.pending_scan.as_ref().map(Receiver::try_recv);
        let mut result = match received {
            Some(Ok(result)) => result,
            Some(Err(TryRecvError::Empty)) | None => return false,
            Some(Err(TryRecvError::Disconnected)) => {
                self.pending_scan = None;
                return false;
            }
        };
        self.pending_scan = None;
        if self.root.as_deref() != Some(result.root.as_path()) {
            return false;
        }
        if result.sort_order != self.sort_order {
            Self::resort_tree_recursive(&mut result.tree, self.sort_order);
        }
        // Expansion can change while the worker is running. Reuse its snapshot
        // and load any directories expanded after it started.
        Self::restore_expanded_children(&mut result.tree, &self.expanded_dirs, self.sort_order);
        self.tree = result.tree;
        true
    }

    /// Refresh the file tree in the background.
    fn refresh(&mut self) {
        if let Some(root) = &self.root.clone() {
            if self.start_root_scan(root.clone(), "explorer-refresh") {
                return;
            }
            self.tree = Self::scan_directory_shallow(root, self.sort_order);
            Self::restore_expanded_children(&mut self.tree, &self.expanded_dirs, self.sort_order);
        }
    }

    fn restore_expanded_children(
        tree: &mut [FileTreeNode],
        expanded_dirs: &HashSet<PathBuf>,
        sort_order: SortOrder,
    ) {
        // Parents must be loaded before their expanded descendants can be
        // found in the lazy tree.
        let mut expanded: Vec<&PathBuf> = expanded_dirs.iter().collect();
        expanded.sort_by_key(|path| path.components().count());
        for path in expanded {
            Self::load_children_in_tree(tree, path, sort_order);
        }
    }

    /// Rescan only one changed directory while preserving the rest of the tree.
    fn refresh_directory(&mut self, directory: &Path) {
        let Some(root) = self.root.as_ref() else {
            return;
        };
        if directory == root {
            self.tree = Self::scan_directory_shallow(&root.clone(), self.sort_order);
        } else {
            let mut replacement = Some(Self::scan_directory_shallow(
                &directory.to_path_buf(),
                self.sort_order,
            ));
            if !Self::replace_directory_children(&mut self.tree, directory, &mut replacement) {
                return;
            }
        }

        // Restore expanded descendants from shallowest to deepest so their
        // parents exist before lazy children are loaded.
        let mut expanded: Vec<PathBuf> = self.expanded_dirs.iter().cloned().collect();
        expanded.sort_by_key(|path| path.components().count());
        for path in expanded {
            self.load_children(&path);
        }
    }

    fn replace_directory_children(
        nodes: &mut [FileTreeNode],
        directory: &Path,
        replacement: &mut Option<Vec<FileTreeNode>>,
    ) -> bool {
        for node in nodes {
            if let FileTreeNode::Directory { path, children, .. } = node {
                if path == directory {
                    *children = replacement.take();
                    return true;
                }
                if let Some(children) = children {
                    if Self::replace_directory_children(children, directory, replacement) {
                        return true;
                    }
                }
            }
        }
        false
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

    /// Maximum depth for expand_all to prevent excessive recursion
    const MAX_EXPAND_DEPTH: usize = 10;

    /// Expand all directories in the tree (loads all children recursively up to MAX_EXPAND_DEPTH)
    fn expand_all(&mut self) {
        // First, recursively load all directories (with depth limit)
        Self::load_all_children(&mut self.tree, self.sort_order, 0);
        // Then collect all directory paths
        self.expanded_dirs = Self::collect_all_dirs(&self.tree);
    }

    /// Recursively load all unloaded directories (up to MAX_EXPAND_DEPTH)
    fn load_all_children(nodes: &mut [FileTreeNode], sort_order: SortOrder, depth: usize) {
        if depth >= Self::MAX_EXPAND_DEPTH {
            return;
        }
        for node in nodes.iter_mut() {
            if let FileTreeNode::Directory { path, children, .. } = node {
                // Load children if not yet loaded
                if children.is_none() {
                    *children = Some(Self::scan_directory_shallow(path, sort_order));
                }
                // Recurse into children
                if let Some(ref mut child_nodes) = children {
                    Self::load_all_children(child_nodes, sort_order, depth + 1);
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
    /// Composite header-position key (see `header_position_key`) waiting for
    /// a corrective scroll. Set when the outline-click handler used the
    /// line-ratio fallback because the cache didn't yet have the precise y
    /// for this key. Cleared once the post-render corrective step has
    /// snapped the viewport to the recorded position.
    pending_header_click_key: Option<String>,
    /// One-shot permission for the post-render corrective scroll that snaps
    /// the active search match into view. Set by `scroll_to_active_match`
    /// (jump_match / search-open / tab-switch / query-rebuild) and cleared
    /// after the corrective block runs once. Without this gate, the block
    /// would re-fire every frame while `active_search_y` remains populated,
    /// fighting the user's wheel input and locking the view to the active
    /// match — see issue #19 / docs/devlog/031-search-scroll-lock.md.
    correct_active_search_pending: bool,
    last_content_height: f32,
    last_viewport_height: f32,
    content_lines: usize,
    local_links: Vec<String>,
    /// Cached base URI for markdown image/link resolution (e.g. "file:///path/to/dir/")
    base_uri: String,
    history_back: Vec<PathBuf>,
    history_forward: Vec<PathBuf>,
    /// Cached matches for the current search query; empty when bar is closed or query is empty
    search_matches: Vec<SearchMatch>,
    /// Monotonic counter bumped on every content load/reload. Used as the
    /// invalidation key for the renderer's per-document scroll cache so
    /// parsed events and split_points can survive across frames without
    /// re-hashing the entire content.
    content_version: u64,
}

impl Tab {
    fn compute_base_uri(path: &std::path::Path) -> String {
        path.parent()
            .map(|p| format!("file://{}/", p.display()))
            .unwrap_or_else(|| "file://".to_string())
    }

    fn new(path: PathBuf) -> io::Result<Self> {
        // Canonicalize path for consistent comparison with watcher events
        let path = path.canonicalize().unwrap_or(path);
        let content = String::from_utf8_lossy(&fs::read(&path)?).into_owned();
        let parsed = parse_headers(&content);
        let local_links = parse_local_links(&content, &path);
        let content_lines = content.lines().count();
        let base_uri = Self::compute_base_uri(&path);

        let mut cache = CommonMarkCache::default();
        for link in &local_links {
            cache.add_link_hook(link);
        }

        Ok(Self {
            id: egui::Id::new(&path),
            path,
            content,
            cache,
            document_title: parsed.document_title,
            outline_headers: parsed.outline_headers,
            collapsed_headers: HashSet::new(),
            scroll_offset: 0.0,
            pending_scroll_offset: None,
            pending_header_click_key: None,
            correct_active_search_pending: false,
            last_content_height: 0.0,
            last_viewport_height: 0.0,
            content_lines,
            local_links,
            base_uri,
            history_back: Vec::new(),
            history_forward: Vec::new(),
            search_matches: Vec::new(),
            content_version: 1,
        })
    }

    fn title(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string())
    }

    fn reload(&mut self) -> io::Result<()> {
        let content = String::from_utf8_lossy(&fs::read(&self.path)?).into_owned();
        self.apply_loaded_content(self.path.clone(), content, false);
        Ok(())
    }

    /// Rebuild `search_matches` for `query`. Empty query clears matches.
    fn rebuild_search(&mut self, query: &str) {
        self.search_matches = find_matches(&self.content, query);
    }

    fn load_file(&mut self, path: &Path) -> io::Result<()> {
        let content = String::from_utf8_lossy(&fs::read(path)?).into_owned();
        self.apply_loaded_content(path.to_path_buf(), content, true);
        Ok(())
    }

    fn apply_loaded_content(&mut self, path: PathBuf, content: String, reset_scroll: bool) {
        self.content_lines = content.lines().count();
        self.content = content;
        self.path = path;
        self.id = egui::Id::new(&self.path);
        self.cache = CommonMarkCache::default();
        self.content_version = self.content_version.wrapping_add(1);
        if reset_scroll {
            self.scroll_offset = 0.0;
            self.pending_scroll_offset = None;
        }
        self.base_uri = Self::compute_base_uri(&self.path);

        let parsed = parse_headers(&self.content);
        self.document_title = parsed.document_title;
        self.outline_headers = parsed.outline_headers;
        self.collapsed_headers.clear();

        self.local_links = parse_local_links(&self.content, &self.path);
        for link in &self.local_links {
            self.cache.add_link_hook(link);
        }

        // Stale byte ranges; caller rebuilds if search bar is open.
        self.search_matches.clear();
    }

    fn navigate_to_link(&mut self, link: &str) -> io::Result<bool> {
        if link.starts_with('#') {
            return Ok(false);
        }

        let Some(current_dir) = self.path.parent() else {
            return Ok(false);
        };

        // Share one resolver with `resolve_link` (the ctrl+click path).
        // These used to differ: #78 taught percent-decoding and `file://`
        // handling to `resolve_local_link_path`, but this function still did a
        // raw `join`, so a link with an encoded space opened in a new tab on
        // ctrl+click and silently did nothing on a plain click.
        let Some(target_path) = resolve_local_link_path(link, current_dir) else {
            return Ok(false);
        };

        let target_path = match target_path.canonicalize() {
            Ok(p) => p,
            Err(_) => return Ok(false),
        };

        if target_path == self.path || !target_path.is_file() {
            return Ok(false);
        }
        let previous_path = self.path.clone();
        self.load_file(&target_path)?;
        self.history_back.push(previous_path);
        self.history_forward.clear();
        Ok(true)
    }

    /// Replace this tab's document and record a browser-like history entry.
    /// Returns whether navigation happened; failures leave history untouched.
    fn navigate_to_path(&mut self, target_path: &Path) -> io::Result<bool> {
        let Ok(target_path) = target_path.canonicalize() else {
            return Ok(false);
        };
        if target_path == self.path || !target_path.is_file() {
            return Ok(false);
        }

        let previous_path = self.path.clone();
        self.load_file(&target_path)?;
        self.history_back.push(previous_path);
        self.history_forward.clear();
        Ok(true)
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

    fn navigate_back(&mut self) -> io::Result<bool> {
        let Some(prev_path) = self.history_back.last().cloned() else {
            return Ok(false);
        };
        let current_path = self.path.clone();
        self.load_file(&prev_path)?;
        self.history_back.pop();
        self.history_forward.push(current_path);
        Ok(true)
    }

    fn navigate_forward(&mut self) -> io::Result<bool> {
        let Some(next_path) = self.history_forward.last().cloned() else {
            return Ok(false);
        };
        let current_path = self.path.clone();
        self.load_file(&next_path)?;
        self.history_forward.pop();
        self.history_back.push(current_path);
        Ok(true)
    }

    fn resolve_link(&self, link: &str) -> Option<PathBuf> {
        if link.starts_with('#') {
            return None;
        }

        let current_dir = self.path.parent()?;
        let target_path = resolve_local_link_path(link, current_dir)?;
        target_path.canonicalize().ok()
    }
}

/// Parse explicit links plus bare local Markdown file references, skipping code blocks.
fn parse_local_links(content: &str, document_path: &Path) -> Vec<String> {
    let mut links = HashSet::new();
    let document_dir = document_path.parent().unwrap_or_else(|| Path::new("."));
    let mut in_code_block = false;

    // Let CommonMark decide which bytes are visible text. Event::Text excludes
    // fenced/indented code and link destinations, while Event::Code preserves
    // the viewer extension that makes an inline-code filename clickable.
    for event in pulldown_cmark::Parser::new_ext(content, pulldown_cmark::Options::all()) {
        match event {
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::CodeBlock(_)) => {
                in_code_block = true;
            }
            pulldown_cmark::Event::End(pulldown_cmark::TagEnd::CodeBlock) => {
                in_code_block = false;
            }
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { dest_url, .. }) => {
                let destination = dest_url.as_ref();
                if is_local_markdown_link(destination) || destination.starts_with('#') {
                    links.insert(destination.to_owned());
                }
            }
            pulldown_cmark::Event::Code(code) => {
                register_existing_markdown_path(&code, document_dir, &mut links);
            }
            pulldown_cmark::Event::Text(text) if !in_code_block => {
                register_bare_markdown_paths(&text, document_dir, &mut links);
            }
            _ => {}
        }
    }

    let mut links: Vec<_> = links.into_iter().collect();
    links.sort();
    links
}

fn register_bare_markdown_paths(text: &str, document_dir: &Path, links: &mut HashSet<String>) {
    for token in text.split_whitespace() {
        let destination = token.trim_matches(|ch: char| {
            matches!(
                ch,
                '`' | '*'
                    | '_'
                    | '"'
                    | '\''
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '<'
                    | '>'
                    | ','
                    | '.'
                    | ';'
                    | ':'
                    | '!'
                    | '?'
                    | '，'
                    | '。'
                    | '；'
                    | '：'
                    | '！'
                    | '？'
                    | '（'
                    | '）'
                    | '【'
                    | '】'
                    | '《'
                    | '》'
            )
        });
        register_existing_markdown_path(destination, document_dir, links);
    }
}

fn register_existing_markdown_path(
    destination: &str,
    document_dir: &Path,
    links: &mut HashSet<String>,
) {
    if !is_local_markdown_link(destination) {
        return;
    }
    if resolve_local_link_path(destination, document_dir).is_some_and(|path| path.is_file()) {
        links.insert(destination.to_owned());
    }
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

    let path = if destination.starts_with("file://") {
        let Ok(url) = url::Url::parse(destination) else {
            return false;
        };
        let Ok(path) = url.to_file_path() else {
            return false;
        };
        path
    } else {
        PathBuf::from(destination.split('#').next().unwrap_or(destination))
    };
    path.extension()
        .map(|ext| {
            let ext = ext.to_string_lossy().to_lowercase();
            ext == "md" || ext == "markdown" || ext == "txt"
        })
        .unwrap_or(false)
}

/// Resolve a relative/absolute filesystem path or a `file://` URI against the
/// directory containing the Markdown document.
fn resolve_local_link_path(destination: &str, document_dir: &Path) -> Option<PathBuf> {
    if destination.starts_with("file://") {
        return url::Url::parse(destination).ok()?.to_file_path().ok();
    }

    let encoded_path = destination.split('#').next().unwrap_or(destination);
    let decoded_path = percent_encoding::percent_decode_str(encoded_path)
        .decode_utf8()
        .ok()?;
    let path = Path::new(decoded_path.as_ref());
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        document_dir.join(path)
    })
}

/// Parse headings with the same CommonMark rules used by the renderer.
fn parse_headers(content: &str) -> ParsedHeaders {
    use pulldown_cmark::{Event, HeadingLevel, Tag, TagEnd};

    fn level_number(level: HeadingLevel) -> u8 {
        match level {
            HeadingLevel::H1 => 1,
            HeadingLevel::H2 => 2,
            HeadingLevel::H3 => 3,
            HeadingLevel::H4 => 4,
            HeadingLevel::H5 => 5,
            HeadingLevel::H6 => 6,
        }
    }

    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(content.match_indices('\n').map(|(offset, _)| offset + 1))
        .collect();
    let mut all_headers = Vec::new();
    let mut current: Option<(u8, usize, usize, String)> = None;

    for (event, range) in
        pulldown_cmark::Parser::new_ext(content, pulldown_cmark::Options::all()).into_offset_iter()
    {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let line_number = line_starts
                    .partition_point(|line_start| *line_start <= range.start)
                    .saturating_sub(1);
                current = Some((level_number(level), range.start, line_number, String::new()));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, source_start, line_number, title)) = current.take() {
                    let title = title.trim().to_owned();
                    if !title.is_empty() {
                        all_headers.push(Header {
                            level,
                            title,
                            source_start,
                            line_number,
                        });
                    }
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, _, _, title)) = current.as_mut() {
                    title.push_str(&text);
                }
            }
            Event::InlineMath(text) | Event::DisplayMath(text) => {
                if let Some((_, _, _, title)) = current.as_mut() {
                    title.push_str(&text);
                }
            }
            Event::FootnoteReference(label) => {
                if let Some((_, _, _, title)) = current.as_mut() {
                    title.push('[');
                    title.push_str(&label);
                    title.push(']');
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some((_, _, _, title)) = current.as_mut() {
                    title.push(' ');
                }
            }
            _ => {}
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

/// Find all occurrences of `query` in `content`, case-insensitive (ASCII only).
///
/// Byte offsets are 1:1 with `content` because `to_ascii_lowercase` does not change
/// byte length. Non-ASCII letters match literally (`É` does not match `é`) — see the
/// devlog future-improvements list for the case-toggle and full Unicode case folding.
///
/// Matches spanning a newline are excluded (a search bar should not jump to results
/// the user cannot interpret as a single line).
///
/// Matches inside markdown that's not visibly rendered are also excluded:
/// - Image alt-text `![alt](url)` — alt is hover/screen-reader only
/// - Image URL `![alt](url)` — never visible
/// - Link URL `[text](url)` — never visible (only the text part is)
///
/// Without this, cycling lands on invisible matches: the user sees no visible
/// highlight change and the scroll target points to a paragraph where the rendered
/// text doesn't contain the searched bytes.
fn find_matches(content: &str, query: &str) -> Vec<SearchMatch> {
    if query.is_empty() || content.is_empty() {
        return Vec::new();
    }

    let content_lc = content.to_ascii_lowercase();
    let query_lc = query.to_ascii_lowercase();
    let query_len = query_lc.len();

    // Identify byte ranges of non-renderable markdown parts so we can skip matches
    // inside them. Pattern: `(!?)[alt-or-text](url)`.
    // - Group 1 = `!` for images, empty for links
    // - Group 2 = alt (image) or text (link); exclude only if image (group 1 = `!`)
    // - Group 3 = url; always exclude (URLs are never visible inline)
    let skip_spans: Vec<std::ops::Range<usize>> = {
        static MD_LINK_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = MD_LINK_RE.get_or_init(|| {
            Regex::new(r"(!?)\[([^\]]*)\]\(([^)]*)\)").expect("static regex must compile")
        });
        let mut spans = Vec::new();
        for cap in re.captures_iter(content) {
            let is_image = cap.get(1).map(|m| !m.as_str().is_empty()).unwrap_or(false);
            // Always exclude URL (group 3)
            if let Some(url) = cap.get(3) {
                spans.push(url.start()..url.end());
            }
            // For images, also exclude alt (group 2)
            if is_image {
                if let Some(alt) = cap.get(2) {
                    spans.push(alt.start()..alt.end());
                }
            }
        }
        spans
    };

    let in_skip = |start: usize, end: usize| -> bool {
        skip_spans.iter().any(|s| start >= s.start && end <= s.end)
    };

    let mut matches = Vec::new();
    let mut line_number = 1usize;
    let mut cursor = 0usize;

    for (byte_start, _) in content_lc.match_indices(&query_lc) {
        // Count newlines between cursor and this match's start
        line_number += content[cursor..byte_start]
            .bytes()
            .filter(|&b| b == b'\n')
            .count();
        cursor = byte_start;

        let byte_end = byte_start + query_len;
        if content[byte_start..byte_end].contains('\n') {
            continue; // Skip matches that cross line boundaries
        }
        if in_skip(byte_start, byte_end) {
            continue; // Skip matches inside image alt-text or URL portions of links/images
        }

        matches.push(SearchMatch {
            byte_start,
            byte_end,
            line_number,
        });
    }

    matches
}

fn take_search_correction_request(pending: &mut bool) -> bool {
    std::mem::take(pending)
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

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Parser, Debug)]
#[command(name = "md-viewer")]
#[command(version)]
#[command(about = "A lightweight markdown viewer", long_about = None)]
struct Args {
    /// Markdown file to open
    file: Option<PathBuf>,

    /// Disable live reload (watching is enabled by default)
    #[arg(long)]
    no_watch: bool,

    /// Keep the GUI process attached to the terminal for debugging/logs
    #[arg(long)]
    foreground: bool,

    /// Internal marker used by the detached child process to avoid respawn loops
    #[arg(long, hide = true)]
    no_detach: bool,
}

fn should_detach(args: &Args, launched_from_terminal: bool) -> bool {
    launched_from_terminal && !args.foreground && !args.no_detach
}

fn launched_from_terminal() -> bool {
    std::io::stdin().is_terminal()
        || std::io::stdout().is_terminal()
        || std::io::stderr().is_terminal()
}

fn child_args_with_no_detach<I>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = OsString>,
{
    let mut child_args: Vec<OsString> = args.into_iter().skip(1).collect();
    let marker_position = child_args
        .iter()
        .position(|arg| arg == OsStr::new("--"))
        .unwrap_or(child_args.len());
    child_args.insert(marker_position, OsString::from("--no-detach"));
    child_args
}

fn spawn_detached_child() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let child_args = child_args_with_no_detach(std::env::args_os());
    let mut command = Command::new(exe);

    command
        .args(child_args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;

        // Start a new process group/session so the child survives after the
        // launching terminal hands control back to the shell.
        command.pre_exec(|| {
            extern "C" {
                fn setsid() -> i32;
            }

            if setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }

    command.spawn()?;
    Ok(())
}

fn main() -> eframe::Result<()> {
    env_logger::init();

    let args = Args::parse();

    if should_detach(&args, launched_from_terminal()) {
        if let Err(err) = spawn_detached_child() {
            eprintln!("Failed to detach md-viewer process: {err}. Running in foreground.");
        } else {
            return Ok(());
        }
    }

    fall_back_to_x11_if_wayland_socket_is_dead();
    run_native_once(&args)
}

/// Build the native options and hand the app to eframe. Called once per
/// process: if the event loop cannot be created, winit forbids a second
/// attempt (`RecreationAttempt`), which is why backend selection is settled
/// up front instead of via retry.
fn run_native_once(args: &Args) -> eframe::Result<()> {
    // Calculate optimal window width assuming both sidebars are shown (the default)
    let optimal_width =
        CONTENT_OPTIMAL_WIDTH + EXPLORER_DEFAULT_WIDTH + OUTLINE_DEFAULT_WIDTH + PANEL_SEPARATORS;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([optimal_width, OPTIMAL_WINDOW_HEIGHT])
            .with_min_inner_size([400.0, 300.0])
            .with_title("Markdown Viewer")
            // Wayland app_id: GNOME matches running windows to pinned dock
            // icons via app_id (the Wayland WM_CLASS equivalent); without it
            // the launcher shows a second generic icon (#62). Must match the
            // StartupWMClass in the distributed .desktop file.
            .with_app_id("md-viewer")
            .with_drag_and_drop(true),
        ..Default::default()
    };

    let file = args.file.clone();
    let watch = !args.no_watch;

    eframe::run_native(
        "md-viewer",
        options,
        Box::new(move |cc| Ok(Box::new(MarkdownApp::new(cc, file, watch)))),
    )
}

/// winit 0.30 selects exactly one windowing backend from the environment and
/// never falls back: a non-empty WAYLAND_DISPLAY/WAYLAND_SOCKET commits it to
/// Wayland, and an unusable socket then aborts startup even when Xwayland is
/// available (seen in strict snaps on Ubuntu 26.04, #65). Probe the socket
/// before handing control to eframe; if it cannot be connected, clear the
/// Wayland variables so winit picks X11 up front.
#[cfg(unix)]
fn fall_back_to_x11_if_wayland_socket_is_dead() {
    // An inherited WAYLAND_SOCKET fd cannot be probed without consuming it;
    // trust it and let libwayland report any problem.
    if std::env::var_os("WAYLAND_SOCKET").is_some_and(|value| !value.is_empty()) {
        return;
    }

    let display = std::env::var("WAYLAND_DISPLAY")
        .ok()
        .filter(|v| !v.is_empty());
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_default();
    let Some(path) = wayland_display_socket_path(display.as_deref(), &runtime_dir) else {
        return;
    };

    if UnixStream::connect(&path).is_ok() {
        return;
    }

    log::warn!(
        "WAYLAND_DISPLAY points at {}, which cannot be connected to; falling back to X11",
        path.display()
    );
    std::env::remove_var("WAYLAND_DISPLAY");
    std::env::remove_var("WAYLAND_SOCKET");
}

#[cfg(not(unix))]
fn fall_back_to_x11_if_wayland_socket_is_dead() {}

/// Resolve the filesystem path of the Wayland socket from `WAYLAND_DISPLAY`
/// and `XDG_RUNTIME_DIR`, mirroring libwayland's lookup rules: an absolute
/// display name passes through, otherwise it is joined onto the runtime
/// directory. Returns `None` when the pair does not describe a concrete
/// socket — Wayland was never usable in that case, so there is nothing to
/// probe or clear.
fn wayland_display_socket_path(display: Option<&str>, runtime_dir: &OsStr) -> Option<PathBuf> {
    let name = Path::new(display?);
    if name.as_os_str().is_empty() {
        return None;
    }
    if name.is_absolute() {
        return Some(name.to_path_buf());
    }
    if runtime_dir.is_empty() {
        return None;
    }
    Some(Path::new(runtime_dir).join(name))
}

/// Source of a lightbox texture. Mermaid pre-rasterizes its own texture so we
/// own the handle to keep it alive; loader-managed images (egui_extras) just
/// give us an id whose lifetime is governed by the loader.
enum LightboxTexture {
    Owned(egui::TextureHandle),
    Loaded(egui::TextureId),
}

impl LightboxTexture {
    fn id(&self) -> egui::TextureId {
        match self {
            Self::Owned(handle) => handle.id(),
            Self::Loaded(id) => *id,
        }
    }
}

struct LightboxState {
    /// GPU-resident texture (mermaid-owned or loader-owned). Zoom scales it — instant.
    texture: LightboxTexture,
    /// Original rasterized pixel dimensions (before any zoom)
    base_size: egui::Vec2,
    zoom: f32,
    /// Unique ID per open — prevents stale egui Area/ScrollArea state between opens
    open_id: u64,
    /// Tracks scroll offset for zoom-to-cursor (we maintain our own copy because
    /// egui's ScrollArea may clamp the old offset when content size changes mid-zoom)
    scroll_offset: egui::Vec2,
}

/// Check if a path is on a GVFS FUSE mount (e.g., SFTP via Thunar/Nautilus).
fn is_gvfs_path(path: &Path) -> bool {
    path.starts_with("/run/user/") && path.components().any(|c| c.as_os_str() == "gvfs")
}

/// Wrapper for file watchers that supports both inotify (local) and poll (GVFS/remote).
enum FileWatcher {
    Inotify(Debouncer<RecommendedWatcher>),
    Poll(Debouncer<PollWatcher>),
    Dual {
        inotify: Debouncer<RecommendedWatcher>,
        poll: Debouncer<PollWatcher>,
    },
}

impl FileWatcher {
    fn inotify_watcher(&mut self) -> Option<&mut dyn notify::Watcher> {
        match self {
            FileWatcher::Inotify(d) | FileWatcher::Dual { inotify: d, .. } => Some(d.watcher()),
            FileWatcher::Poll(_) => None,
        }
    }

    fn poll_watcher(&mut self) -> Option<&mut dyn notify::Watcher> {
        match self {
            FileWatcher::Poll(d) | FileWatcher::Dual { poll: d, .. } => Some(d.watcher()),
            FileWatcher::Inotify(_) => None,
        }
    }
}

struct MarkdownApp {
    tabs: Vec<Tab>,
    active_tab: usize,
    dark_mode: bool,
    zoom_level: f32,
    show_outline: bool,
    full_width_content: bool,
    math_scale: f32,
    // Selection/highlight background color override. `None` = theme default.
    highlight_color: Option<egui::Color32>,
    // Mirrors `last_applied_dark_mode`: only rebuild Visuals when this changes.
    last_applied_highlight_color: Option<egui::Color32>,
    // Hyperlink text color override. `None` = theme default.
    link_color: Option<egui::Color32>,
    last_applied_link_color: Option<egui::Color32>,
    show_color_settings_dialog: bool,
    watch_enabled: bool,
    error_message: Option<String>,
    is_dragging: bool,
    // File watcher state (inotify for local, poll for GVFS/remote)
    watcher: Option<FileWatcher>,
    watcher_rx: Option<Receiver<Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>>>,
    watcher_retry_count: u32,
    // Set of paths being watched (individual tab files)
    watched_paths: HashSet<PathBuf>,
    // Explorer directories being watched non-recursively: the root plus each
    // currently-expanded directory. Mirrors the lazy explorer tree so startup
    // doesn't recursively walk the whole root subtree (issue: ~6s hang on a
    // huge home directory).
    watched_explorer_dirs: HashSet<PathBuf>,
    // Tab being hovered for close button
    hovered_tab: Option<usize>,
    // File explorer state
    file_explorer: FileExplorer,
    show_explorer: bool,
    explorer_width: f32,
    outline_width: f32,
    // Flash effect for updated files (path -> start time)
    flashing_paths: HashMap<PathBuf, Instant>,
    // True if running on virtual display (e.g., Xvfb :99) - limits frame rate
    is_virtual_display: bool,
    // Stored context for waking egui from the watcher bridge thread
    egui_ctx: egui::Context,
    // Track state to avoid unconditional repaints
    last_applied_dark_mode: Option<bool>,
    last_window_title: String,
    title_dirty: bool,
    /// Cached set of open tab paths for file explorer highlighting (avoids per-frame syscalls)
    open_tab_paths: HashSet<PathBuf>,
    // Lightbox overlay for enlarged mermaid diagrams
    lightbox: Option<LightboxState>,
    // Scroll delta captured from raw_input_hook for lightbox zoom (stripped from RawInput)
    lightbox_scroll: f32,
    // Counter for unique lightbox IDs (prevents stale egui state between opens)
    lightbox_open_count: u64,
    // Find-bar state (current-document search)
    search: SearchState,
    // Recently opened files (most-recent first), shown on the welcome page
    recent_files: Vec<RecentEntry>,
    // Welcome page: whether the recent list is expanded ("Show more")
    welcome_show_all: bool,
    // MCP bridge for E2E testing
    #[cfg(feature = "mcp")]
    mcp_bridge: McpBridge,
}

impl MarkdownApp {
    fn new(cc: &eframe::CreationContext<'_>, file: Option<PathBuf>, watch: bool) -> Self {
        // Setup fonts with system font fallbacks for Unicode support
        setup_fonts(&cc.egui_ctx);

        // Clear stale egui widget data loaded from disk (scroll offsets, panel sizes, etc.)
        // We don't persist egui memory (see persist_egui_memory), but eframe always
        // loads it if present. This purges the old blob so it doesn't waste startup time/RAM.
        cc.egui_ctx.memory_mut(|mem| mem.data = Default::default());

        // Disable egui's built-in Ctrl+/- zoom — we handle zoom ourselves
        cc.egui_ctx
            .options_mut(|opt| opt.zoom_with_keyboard = false);

        // Set constant styles once at init (never changes at runtime)
        cc.egui_ctx.style_mut(|style| {
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
            style.animation_time = 0.15;
            style.scroll_animation.points_per_second = 1500.0;

            // Reduce resize grab radius to prevent overlap with adjacent scrollbars
            style.interaction.resize_grab_radius_side = SIDEBAR_RESIZE_GRAB_RADIUS;
        });

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
        let full_width_content = persisted.full_width_content.unwrap_or(false);
        let math_scale = persisted.math_scale.unwrap_or(1.0).clamp(1.0, 2.0);
        let highlight_color = persisted
            .highlight_color
            .map(|[r, g, b, a]| egui::Color32::from_rgba_premultiplied(r, g, b, a));
        let link_color = persisted
            .link_color
            .map(|[r, g, b, a]| egui::Color32::from_rgba_premultiplied(r, g, b, a));
        let show_explorer = persisted.show_explorer.unwrap_or(true);
        let explorer_width =
            restored_sidebar_width(persisted.explorer_width, EXPLORER_DEFAULT_WIDTH);
        let outline_width = restored_sidebar_width(persisted.outline_width, OUTLINE_DEFAULT_WIDTH);

        // Determine initial tabs
        let mut startup_error = None;
        let initial_tabs: Vec<Tab> = if let Some(ref path) = file {
            // CLI argument takes priority
            match Tab::new(path.clone()) {
                Ok(tab) => vec![tab],
                Err(error) => {
                    startup_error = Some(format!("Unable to open {}: {error}", path.display()));
                    Vec::new()
                }
            }
        } else if let Some(paths) = persisted.open_tabs {
            // Restore previous session tabs
            paths
                .into_iter()
                .filter(|p| p.exists())
                .filter_map(|path| match Tab::new(path.clone()) {
                    Ok(tab) => Some(tab),
                    Err(error) => {
                        log::warn!("Unable to restore {}: {error}", path.display());
                        None
                    }
                })
                .collect()
        } else {
            // No file and no saved session → start empty (welcome page).
            Vec::new()
        };

        let tabs = initial_tabs;

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

        if let Some(ref root) = explorer_root {
            file_explorer.set_root(root.clone());
        }

        // Restore expanded directories (children will lazy-load on first render)
        if let Some(expanded) = persisted.expanded_dirs {
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
            full_width_content,
            math_scale,
            // Frame 1 always rebuilds Visuals anyway (last_applied_dark_mode
            // starts at None), so this just avoids a spurious second rebuild
            // afterward if nothing further changes.
            highlight_color,
            last_applied_highlight_color: highlight_color,
            link_color,
            last_applied_link_color: link_color,
            show_color_settings_dialog: false,
            watch_enabled: watch,
            error_message: startup_error,
            is_dragging: false,
            watcher: None,
            watcher_rx: None,
            watcher_retry_count: 0,
            watched_paths: HashSet::new(),
            watched_explorer_dirs: HashSet::new(),
            hovered_tab: None,
            file_explorer,
            show_explorer,
            explorer_width,
            outline_width,
            flashing_paths: HashMap::new(),
            is_virtual_display,
            egui_ctx: cc.egui_ctx.clone(),
            last_applied_dark_mode: None,
            last_window_title: String::new(),
            title_dirty: true,
            open_tab_paths: HashSet::new(),
            lightbox: None,
            lightbox_scroll: 0.0,
            lightbox_open_count: 0,
            search: SearchState::default(),
            recent_files: persisted.recent_files.unwrap_or_default(),
            welcome_show_all: false,
            #[cfg(feature = "mcp")]
            mcp_bridge,
        };

        app.refresh_open_tab_paths();

        // Record a CLI-provided file as recent (an explicit open). Session-restored
        // tabs are not treated as explicit opens, so they are not recorded here.
        if let Some(f) = &file {
            let f = f.canonicalize().unwrap_or_else(|_| f.clone());
            app.record_recent(&f);
        }

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
        let initial_dir = self
            .tabs
            .get(self.active_tab)
            .and_then(|tab| tab.path.parent())
            .map(Path::to_path_buf)
            .or_else(|| self.file_explorer.root.clone());

        match pick_file(initial_dir.as_deref()) {
            Ok(Some(path)) => self.open_in_new_tab(path),
            Ok(None) => {}
            Err(error) => {
                log::error!("Unable to open file picker: {error}");
                self.error_message = Some(format!("Unable to open file picker: {error}"));
            }
        }
    }

    /// Open a native folder picker and point the file explorer at the chosen
    /// directory (issue #28). The chosen root is persisted via `save()`.
    fn open_folder_dialog(&mut self) {
        match pick_folder(self.file_explorer.root.as_deref()) {
            Ok(Some(path)) => {
                self.file_explorer.set_root(path);
                // Make sure the explorer is visible so the result is seen.
                self.show_explorer = true;
                // Rebuild the watcher so the new root is watched recursively
                // (`update_watched_paths` only reconciles tab paths, not the root).
                if self.watch_enabled {
                    self.start_watching();
                }
            }
            Ok(None) => {}
            Err(error) => {
                log::error!("Unable to open folder picker: {error}");
                self.error_message = Some(format!("Unable to open folder picker: {error}"));
            }
        }
    }

    fn open_in_new_tab(&mut self, path: PathBuf) {
        // Canonicalize for consistent comparison with existing tabs
        let path = path.canonicalize().unwrap_or(path);
        // Check if already open
        if let Some(idx) = self.tabs.iter().position(|t| t.path == path) {
            self.active_tab = idx;
            self.title_dirty = true;
            return;
        }

        // Add new tab
        let tab = match Tab::new(path.clone()) {
            Ok(tab) => tab,
            Err(error) => {
                self.error_message = Some(format!("Unable to open {}: {error}", path.display()));
                return;
            }
        };
        self.record_recent(&path);
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.title_dirty = true;
        self.refresh_open_tab_paths();

        // Update watcher if enabled
        if self.watch_enabled {
            self.update_watched_paths();
        }
    }

    fn navigate_active_tab_to(&mut self, path: &Path) {
        if self.tabs.is_empty() {
            self.open_in_new_tab(path.to_path_buf());
            return;
        }

        let changed = self
            .tabs
            .get_mut(self.active_tab)
            .is_some_and(|tab| tab.navigate_to_path(path).unwrap_or(false));
        if !changed {
            return;
        }

        let opened_path = self.tabs[self.active_tab].path.clone();
        self.record_recent(&opened_path);
        self.title_dirty = true;
        self.refresh_open_tab_paths();
        if self.watch_enabled {
            self.update_watched_paths();
        }
    }

    fn navigate_active_history(&mut self, go_back: bool) {
        let result = self.tabs.get_mut(self.active_tab).map(|tab| {
            if go_back {
                tab.navigate_back()
            } else {
                tab.navigate_forward()
            }
        });
        if let Some(Err(error)) = result {
            self.error_message = Some(format!("Unable to navigate history: {error}"));
        }
    }

    /// Record a file in the recent list (most-recent first, deduped, capped).
    fn record_recent(&mut self, path: &Path) {
        push_recent(&mut self.recent_files, path, now_epoch_secs());
    }

    fn close_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }

        self.tabs.remove(idx);
        self.title_dirty = true;
        self.refresh_open_tab_paths();

        // Adjust active tab index. Closing the last tab leaves `tabs` empty,
        // which renders the welcome page (active_tab kept in range / at 0).
        if self.tabs.is_empty() {
            self.active_tab = 0;
        } else if self.active_tab >= self.tabs.len() {
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
            self.title_dirty = true;
        }
    }

    fn prev_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.active_tab = if self.active_tab == 0 {
                self.tabs.len() - 1
            } else {
                self.active_tab - 1
            };
            self.title_dirty = true;
        }
    }

    fn focus_tab(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.active_tab = idx;
            self.title_dirty = true;
        }
    }

    fn get_open_tab_paths(&self) -> Vec<PathBuf> {
        self.tabs
            .iter()
            .filter(|t| t.path.exists())
            .map(|t| t.path.clone())
            .collect()
    }

    /// Rebuild the cached open_tab_paths set (call after tab open/close/navigate)
    fn refresh_open_tab_paths(&mut self) {
        self.open_tab_paths.clear();
        for tab in &self.tabs {
            self.open_tab_paths.insert(tab.path.clone());
        }
    }

    /// Start watching because of a user action or configuration change.
    fn start_watching(&mut self) {
        self.watcher_retry_count = 0;
        self.start_watching_attempt();
    }

    /// Construct watchers without resetting the recovery-attempt counter.
    fn start_watching_attempt(&mut self) {
        self.stop_watching();

        let tab_paths = self.get_open_tab_paths();
        let explorer_root = self.file_explorer.root.clone();

        // Need something to watch
        if tab_paths.is_empty() && explorer_root.is_none() {
            return;
        }

        // Partition tab paths into local (inotify) and GVFS (poll)
        let (gvfs_paths, local_paths): (Vec<_>, Vec<_>) =
            tab_paths.iter().partition(|p| is_gvfs_path(p));
        let explorer_is_gvfs = explorer_root.as_ref().is_some_and(|r| is_gvfs_path(r));
        // Explorer root only watched via inotify (local) — GVFS explorer roots are
        // skipped to avoid costly recursive polling over SFTP
        let has_local = !local_paths.is_empty() || (explorer_root.is_some() && !explorer_is_gvfs);
        let has_gvfs = !gvfs_paths.is_empty();

        // Non-recursive: mirror the lazy explorer tree. Watching the whole root
        // subtree recursively walks every directory on the main thread at startup
        // (~6s hang on /home/ahmet, ~455k dirs). We only need change events for
        // the directories the user can actually see: the root plus each expanded
        // directory. GVFS explorer roots stay unwatched (costly over SFTP).
        let mut explorer_dirs: Vec<PathBuf> = Vec::new();
        if let Some(ref root) = explorer_root {
            if !explorer_is_gvfs {
                explorer_dirs.push(root.clone());
                explorer_dirs.extend(
                    self.file_explorer
                        .expanded_dirs
                        .iter()
                        .filter(|d| !is_gvfs_path(d) && d.is_dir())
                        .cloned(),
                );
            }
        }

        // Both debouncers send to the same channel
        let (tx, debouncer_rx) = mpsc::channel();

        // Create inotify debouncer for local paths
        let inotify_debouncer = if has_local {
            match new_debouncer(Duration::from_millis(200), tx.clone()) {
                Ok(mut debouncer) => {
                    for path in &local_paths {
                        if let Err(e) = debouncer
                            .watcher()
                            .watch(path, notify::RecursiveMode::NonRecursive)
                        {
                            log::error!("Failed to watch file {:?}: {}", path, e);
                        } else {
                            self.watched_paths.insert((*path).clone());
                        }
                    }
                    for dir in &explorer_dirs {
                        if let Err(e) = debouncer
                            .watcher()
                            .watch(dir, notify::RecursiveMode::NonRecursive)
                        {
                            log::error!("Failed to watch explorer dir {:?}: {}", dir, e);
                        } else {
                            self.watched_explorer_dirs.insert(dir.clone());
                        }
                    }
                    Some(debouncer)
                }
                Err(e) => {
                    log::error!("Failed to create inotify watcher: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Create poll debouncer for GVFS/remote paths
        let poll_debouncer = if has_gvfs {
            let poll_config = notify_debouncer_mini::Config::default()
                .with_timeout(Duration::from_millis(200))
                .with_notify_config(
                    notify::Config::default().with_poll_interval(Duration::from_secs(2)),
                );
            match new_debouncer_opt::<_, PollWatcher>(poll_config, tx.clone()) {
                Ok(mut debouncer) => {
                    for path in &gvfs_paths {
                        if let Err(e) = debouncer
                            .watcher()
                            .watch(path, notify::RecursiveMode::NonRecursive)
                        {
                            log::error!("Failed to poll-watch file {:?}: {}", path, e);
                        } else {
                            self.watched_paths.insert((*path).clone());
                        }
                    }
                    // Skip recursive watching of GVFS explorer root — polling an
                    // entire remote directory tree every 2s causes lag from SFTP
                    // roundtrips (stat + read_dir for each entry). Users can refresh
                    // the explorer manually instead.
                    Some(debouncer)
                }
                Err(e) => {
                    log::error!("Failed to create poll watcher: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Build FileWatcher enum from whichever debouncers succeeded
        let file_watcher = match (inotify_debouncer, poll_debouncer) {
            (Some(i), Some(p)) => Some(FileWatcher::Dual {
                inotify: i,
                poll: p,
            }),
            (Some(d), None) => Some(FileWatcher::Inotify(d)),
            (None, Some(d)) => Some(FileWatcher::Poll(d)),
            (None, None) => None,
        };

        if let Some(fw) = file_watcher {
            let local_count = local_paths.len();
            let gvfs_count = gvfs_paths.len();
            log::info!(
                "Started watching {} local (inotify) + {} GVFS (poll) files, {} explorer dirs",
                local_count,
                gvfs_count,
                self.watched_explorer_dirs.len()
            );

            // Bridge thread: forward events and wake egui on demand
            let (bridge_tx, bridge_rx) = mpsc::channel();
            let ctx = self.egui_ctx.clone();
            let bridge = std::thread::Builder::new()
                .name("watcher-bridge".into())
                .spawn(move || {
                    while let Ok(event) = debouncer_rx.recv() {
                        let _ = bridge_tx.send(event);
                        ctx.request_repaint();
                    }
                });

            if let Err(error) = bridge {
                log::error!("Failed to spawn watcher bridge thread: {error}");
                self.error_message = Some(format!("Failed to start file watcher bridge: {error}"));
                return;
            }

            self.watcher = Some(fw);
            self.watcher_rx = Some(bridge_rx);
            self.watch_enabled = true;
        } else {
            log::error!("Failed to create any file watcher");
            self.error_message = Some("Failed to create file watcher".to_string());
        }
    }

    fn stop_watching(&mut self) {
        if self.watcher.is_some() {
            log::info!("Stopped watching files");
        }
        self.watcher = None;
        self.watcher_rx = None;
        self.watched_paths.clear();
        self.watched_explorer_dirs.clear();
    }

    fn update_watched_paths(&mut self) {
        if !self.watch_enabled {
            return;
        }

        let current_paths: HashSet<PathBuf> = self.get_open_tab_paths().into_iter().collect();

        if let Some(fw) = &mut self.watcher {
            // Check if we need a watcher type that doesn't currently exist
            let needs_poll = current_paths.iter().any(|p| is_gvfs_path(p));
            let needs_inotify = current_paths.iter().any(|p| !is_gvfs_path(p));
            let has_poll = fw.poll_watcher().is_some();
            let has_inotify = fw.inotify_watcher().is_some();

            if (needs_poll && !has_poll) || (needs_inotify && !has_inotify) {
                // Watcher configuration changed (e.g., first GVFS tab added), restart
                log::info!("Watcher type mismatch, restarting watchers");
                self.start_watching();
                return;
            }

            // Add new paths to the appropriate watcher
            for path in current_paths.difference(&self.watched_paths) {
                let watcher = if is_gvfs_path(path) {
                    fw.poll_watcher()
                } else {
                    fw.inotify_watcher()
                };
                if let Some(w) = watcher {
                    if let Err(e) = w.watch(path, notify::RecursiveMode::NonRecursive) {
                        log::error!("Failed to watch file {:?}: {}", path, e);
                    }
                }
            }

            // Remove old paths from the appropriate watcher
            for path in self.watched_paths.difference(&current_paths) {
                let watcher = if is_gvfs_path(path) {
                    fw.poll_watcher()
                } else {
                    fw.inotify_watcher()
                };
                if let Some(w) = watcher {
                    let _ = w.unwatch(path);
                }
            }
        }

        self.watched_paths = current_paths;
    }

    /// Reconcile the non-recursive explorer-directory watches (root + expanded
    /// dirs) against the live watcher after the expanded set changes. Mirrors
    /// `update_watched_paths`'s incremental diff so expand/collapse doesn't tear
    /// down and rebuild the debouncer + bridge thread.
    fn reconcile_explorer_watches(&mut self) {
        if !self.watch_enabled {
            return;
        }

        // Desired set: root + each expanded dir (local, still existing).
        let mut desired: HashSet<PathBuf> = HashSet::new();
        if let Some(root) = &self.file_explorer.root {
            if !is_gvfs_path(root) {
                desired.insert(root.clone());
            }
        }
        for d in &self.file_explorer.expanded_dirs {
            if !is_gvfs_path(d) && d.is_dir() {
                desired.insert(d.clone());
            }
        }

        if let Some(fw) = &mut self.watcher {
            if let Some(w) = fw.inotify_watcher() {
                for path in desired.difference(&self.watched_explorer_dirs) {
                    if let Err(e) = w.watch(path, notify::RecursiveMode::NonRecursive) {
                        log::error!("Failed to watch explorer dir {:?}: {}", path, e);
                    }
                }
                for path in self.watched_explorer_dirs.difference(&desired) {
                    let _ = w.unwatch(path);
                }
            }
        }

        self.watched_explorer_dirs = desired;
    }

    fn check_file_changes(&mut self) -> Vec<PathBuf> {
        let Some(rx) = &self.watcher_rx else {
            // Attempt recovery if watching is enabled and there's something to watch
            // Check actual tabs and explorer root, not watched_paths (which may be empty after failure)
            let has_watchable = !self.tabs.is_empty() || self.file_explorer.root.is_some();
            if self.watch_enabled && has_watchable {
                if let Some(attempt) = next_watcher_retry(self.watcher_retry_count) {
                    self.watcher_retry_count = attempt;
                    log::info!("Attempting to recover file watcher (attempt {attempt})");
                    self.start_watching_attempt();
                    self.egui_ctx.request_repaint_after(Duration::from_secs(2));
                } else {
                    self.error_message = Some(format!(
                        "File watcher failed after {MAX_WATCHER_RETRIES} retries"
                    ));
                    self.watch_enabled = false;
                }
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

                    if let Some(attempt) = next_watcher_retry(self.watcher_retry_count) {
                        self.watcher_retry_count = attempt;
                        log::info!("Attempting watcher recovery (attempt {attempt})");
                        self.start_watching_attempt();
                        self.egui_ctx.request_repaint_after(Duration::from_secs(2));
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
        let mut refresh_directories = HashSet::new();
        // If the active tab gets reloaded while the find bar is open, its
        // `search_matches` will be cleared by `Tab::reload`. Force a rebuild
        // on the next frame by invalidating the cache-validity shadow state.
        let active_path = self.tabs.get(self.active_tab).map(|t| t.path.clone());

        for path in changed_paths {
            // Trigger flash effect for the changed file (use canonical path for consistent lookup)
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            self.flashing_paths.insert(canonical, now);

            // Also flash parent directories up to the explorer root
            if let Some(root) = &self.file_explorer.root {
                // Check if the changed path is within the explorer root
                if path.starts_with(root) {
                    if let Some(parent) = path.parent() {
                        refresh_directories.insert(parent.to_path_buf());
                    }
                }

                let mut current = path.parent();
                while let Some(parent) = current {
                    if parent.starts_with(root) || parent == root {
                        self.flashing_paths.insert(
                            parent
                                .canonicalize()
                                .unwrap_or_else(|_| parent.to_path_buf()),
                            now,
                        );
                    }
                    if parent == root {
                        break;
                    }
                    current = parent.parent();
                }
            }

            // Reload the tab content
            let mut active_was_reloaded = false;
            for tab in &mut self.tabs {
                if tab.path == path {
                    log::info!("Reloading tab: {:?}", path);
                    if let Err(error) = tab.reload() {
                        self.error_message =
                            Some(format!("Unable to reload {}: {error}", path.display()));
                        continue;
                    }
                    if Some(&tab.path) == active_path.as_ref() {
                        active_was_reloaded = true;
                    }
                }
            }
            if active_was_reloaded && self.search.is_open {
                // Invalidate maybe_rebuild_search's "cached for" shadow
                self.search.last_tab = None;
            }
        }

        // Refresh only affected parents instead of rebuilding the entire tree.
        for directory in refresh_directories {
            log::debug!("Refreshing explorer directory: {:?}", directory);
            self.file_explorer.refresh_directory(&directory);
        }
    }

    /// Outcome of rendering the find bar (consumed after panel renders, before global input handler runs)
    fn render_search_bar(&mut self, ctx: &egui::Context) -> SearchBarOutcome {
        let mut outcome = SearchBarOutcome::default();
        if !self.search.is_open {
            return outcome;
        }

        let input_id = egui::Id::new("search_input");
        let total_matches = self
            .tabs
            .get(self.active_tab)
            .map(|t| t.search_matches.len())
            .unwrap_or(0);
        let active_idx = self.search.active_match_index;

        egui::TopBottomPanel::top("search_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("🔍");

                let text_edit = egui::TextEdit::singleline(&mut self.search.query)
                    .id(input_id)
                    .hint_text("Find in document")
                    .desired_width(280.0);
                let response = ui.add(text_edit);

                #[cfg(feature = "mcp")]
                self.mcp_bridge
                    .register_widget("Search: Input", "textbox", &response, None);

                if self.search.focus_requested {
                    response.request_focus();
                    self.search.focus_requested = false;
                }

                ui.add_space(8.0);

                let label_text = if self.search.query.is_empty() {
                    String::new()
                } else if total_matches == 0 {
                    "0 matches".to_string()
                } else {
                    format!("{} / {}", active_idx + 1, total_matches)
                };
                let label_response = ui.label(
                    egui::RichText::new(&label_text)
                        .color(ui.style().visuals.weak_text_color())
                        .small(),
                );

                #[cfg(feature = "mcp")]
                self.mcp_bridge.register_widget(
                    "Search: Match Count",
                    "label",
                    &label_response,
                    Some(&label_text),
                );
                #[cfg(not(feature = "mcp"))]
                let _ = label_response;

                ui.add_space(4.0);
                let prev_btn = ui
                    .add_enabled(total_matches > 0, egui::Button::new("↑").small())
                    .on_hover_text("Previous match (Shift+Enter / ↑)");
                #[cfg(feature = "mcp")]
                self.mcp_bridge
                    .register_widget("Search: Previous", "button", &prev_btn, None);
                if prev_btn.clicked() {
                    outcome.prev_clicked = true;
                }

                let next_btn = ui
                    .add_enabled(total_matches > 0, egui::Button::new("↓").small())
                    .on_hover_text("Next match (Enter / ↓)");
                #[cfg(feature = "mcp")]
                self.mcp_bridge
                    .register_widget("Search: Next", "button", &next_btn, None);
                if next_btn.clicked() {
                    outcome.next_clicked = true;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let close_btn = ui.small_button("✕").on_hover_text("Close (Esc)");
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge
                        .register_widget("Search: Close", "button", &close_btn, None);
                    if close_btn.clicked() {
                        outcome.close_requested = true;
                    }
                });
            });
        });

        outcome
    }

    /// Move the active match index forward (dir > 0) or backward (dir < 0), wrapping
    /// around, and request a scroll-to-line via `pending_scroll_offset`.
    fn jump_match(&mut self, dir: i32) {
        let n = self
            .tabs
            .get(self.active_tab)
            .map(|t| t.search_matches.len())
            .unwrap_or(0);
        if n == 0 {
            return;
        }
        let new_idx = if dir >= 0 {
            (self.search.active_match_index + 1) % n
        } else if self.search.active_match_index == 0 {
            n - 1
        } else {
            self.search.active_match_index - 1
        };
        self.search.active_match_index = new_idx;
        self.scroll_to_active_match();
    }

    /// Request a scroll-to-line for the current `active_match_index`. No-op when
    /// there's no active tab, no matches, or content height hasn't been measured yet.
    /// Used both by `jump_match` (cycling) and by `maybe_rebuild_search` (so typing a
    /// fresh query or switching tabs lands the view on the first match instead of
    /// leaving the view at wherever the user previously scrolled).
    fn scroll_to_active_match(&mut self) {
        let idx = self.search.active_match_index;
        let tab = match self.tabs.get_mut(self.active_tab) {
            Some(t) => t,
            None => return,
        };
        let Some(m) = tab.search_matches.get(idx) else {
            return;
        };
        if tab.last_content_height <= 0.0 || tab.content_lines == 0 {
            return;
        }
        // Line-ratio estimate gets the view roughly in the right area. The renderer
        // records the actual y of the active match during paint; render_tab_content
        // schedules a corrective scroll next frame if this estimate was off.
        let estimated_y =
            (m.line_number as f32 / tab.content_lines as f32) * tab.last_content_height;
        let margin = if tab.last_viewport_height > 0.0 {
            tab.last_viewport_height * 0.35
        } else {
            100.0
        };
        tab.pending_scroll_offset = Some((estimated_y - margin).max(0.0));
        // Grant the next render one full-paint attempt to record an exact Y.
        // The request is consumed before that render even when the source
        // match has no visible glyph, so later frames return to clipping.
        tab.correct_active_search_pending = true;
    }

    /// Rebuild the active tab's search matches if the query or active tab has changed.
    /// Cheap (no-op) when nothing changed.
    fn maybe_rebuild_search(&mut self) {
        if !self.search.is_open {
            return;
        }
        let tab_idx = self.active_tab;
        let needs_rebuild =
            self.search.query != self.search.last_query || self.search.last_tab != Some(tab_idx);
        if !needs_rebuild {
            return;
        }
        let q = self.search.query.clone();
        if let Some(tab) = self.tabs.get_mut(tab_idx) {
            tab.rebuild_search(&q);
        }
        self.search.last_query = q;
        self.search.last_tab = Some(tab_idx);
        // Always start at the first match after any rebuild — query change, tab
        // change, or watcher reload. Match indices are not semantically comparable
        // across rebuilds; preserving a numeric index across docs is coincidental
        // (matches Firefox / Chrome / VS Code behavior).
        self.search.active_match_index = 0;
        // Scroll to the new first match so the user sees the active highlight
        // immediately rather than wherever they were previously scrolled. Without
        // this, the visible match (somewhere else on screen) gets the regular
        // highlight color, leaving the user with the impression that the active
        // highlight is "stuck on the previous result".
        self.scroll_to_active_match();
    }

    /// Close the find bar and clear highlight state on every tab.
    fn close_search(&mut self) {
        self.search.is_open = false;
        self.search.query.clear();
        self.search.last_query.clear();
        self.search.last_tab = None;
        self.search.focus_requested = false;
        self.search.active_match_index = 0;
        for tab in self.tabs.iter_mut() {
            tab.search_matches.clear();
            tab.cache.clear_search_ranges();
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

            // Placeholder hint when no document is open (the welcome page is showing)
            if tab_count == 0 {
                ui.add_space(4.0);
                ui.label(egui::RichText::new("No file open").weak());
                ui.add_space(4.0);
            }

            // Opening a file is what creates a new tab; there is no empty-tab state.
            let new_tab_btn = ui
                .button("+")
                .on_hover_text("Open File in New Tab... (Ctrl+T)");

            // Collect the open-file action for MCP.
            #[cfg(feature = "mcp")]
            widget_data.push((
                "Open File in New Tab".to_string(),
                "button",
                new_tab_btn.rect,
                None,
            ));

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
            self.title_dirty = true;
        }

        // Handle close others
        if let Some(keep_idx) = close_others {
            let kept = self.tabs.remove(keep_idx);
            self.tabs.clear();
            self.tabs.push(kept);
            self.active_tab = 0;
            self.title_dirty = true;
            self.refresh_open_tab_paths();
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

        let panel = egui::SidePanel::right("outline")
            .resizable(true)
            .default_width(self.outline_width)
            .width_range(SIDEBAR_MIN_WIDTH..=f32::INFINITY)
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
                // Pre-compute the visible header indices (skip those hidden
                // by collapsed ancestors). This lets us virtualize via
                // show_rows, paying O(visible-on-screen) instead of
                // O(total-headers) per frame. On a 100k-line doc with ~15k
                // headers this is the difference between visibly laggy and
                // smooth outline interactions.
                let visible_indices: Vec<usize> = (0..tab.outline_headers.len())
                    .filter(|&i| !header_is_hidden(&tab.outline_headers, i, &tab.collapsed_headers))
                    .collect();
                let show_fold_indicators = any_header_has_children(&tab.outline_headers);
                // Row height: fold indicator is 20px tall, fold-indicator-less
                // rows fall back to the standard interact_size which is
                // typically 18–20px anyway. A small fudge keeps neighboring
                // rows from clipping into each other.
                let row_height = ui.spacing().interact_size.y.max(20.0);

                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(
                        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                    )
                    .id_salt("outline")
                    .show_rows(ui, row_height, visible_indices.len(), |ui, row_range| {
                        let mut toggle_index: Option<usize> = None;
                        for &idx in &visible_indices[row_range] {
                            let header = &tab.outline_headers[idx];

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

                                let response = ui.add(
                                    egui::Button::selectable(false, &header.title)
                                        .wrap_mode(egui::TextWrapMode::Extend),
                                );

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
        self.outline_width = panel.response.rect.width();

        // Register all collected widgets with MCP bridge
        #[cfg(feature = "mcp")]
        for (name, widget_type, rect, value) in widget_data {
            self.mcp_bridge
                .register_widget_rect(&name, widget_type, rect, value.as_deref());
        }

        // Calculate scroll target if header was clicked
        if let Some(idx) = clicked_header_index {
            if let Some(header) = tab.outline_headers.get(idx) {
                // The renderer records the same source-stable key, so formatting
                // and duplicate display titles cannot redirect the click.
                let key = header_position_key(header.source_start);
                // Try to get actual rendered position from cache first.
                // With virtualization, the cache may hold a stale value from a
                // partial render — record the key for the post-render
                // corrective step which re-checks after the bootstrap full paint.
                if let Some(y_pos) = tab.cache.get_header_position(&key) {
                    tab.pending_scroll_offset = Some((y_pos - 50.0).max(0.0));
                } else if tab.last_content_height > 0.0 && tab.content_lines > 0 {
                    // Fallback: estimate position based on line number ratio
                    let estimated_y = (header.line_number as f32 / tab.content_lines as f32)
                        * tab.last_content_height;
                    tab.pending_scroll_offset = Some((estimated_y - 50.0).max(0.0));
                    // The estimate only gets us near the target. Request one
                    // complete paint so the corrective pass can measure it.
                    tab.pending_header_click_key = Some(key);
                }
            }
        }
    }

    /// Render the active tab's content
    /// Render the welcome / idle page shown when no document is open (issue #28).
    fn render_welcome(&mut self, ui: &mut egui::Ui) {
        let mut do_open_file = false;
        let mut do_open_folder = false;
        let mut open_path: Option<PathBuf> = None;
        let mut toggle_show_all = false;
        let now = now_epoch_secs();

        // Snapshot the entries to display so we don't borrow self while acting on clicks.
        let shown = if self.welcome_show_all {
            RECENT_FILES_CAP
        } else {
            RECENT_SHOWN
        };
        let recents: Vec<(RecentEntry, bool)> = self
            .recent_files
            .iter()
            .take(shown)
            .map(|e| (e.clone(), e.path.exists()))
            .collect();
        let total_recent = self.recent_files.len();
        let show_all = self.welcome_show_all;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(64.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("📄").size(56.0).weak());
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("Open a file or folder")
                            .size(22.0)
                            .strong(),
                    );
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new("No document is open.").weak());
                });
                ui.add_space(18.0);
                // Center the button row manually: a horizontal inside
                // vertical_centered expands to full width, defeating centering.
                ui.horizontal(|ui| {
                    let avail = ui.available_width();
                    ui.add_space(((avail - 230.0) / 2.0).max(0.0));
                    if ui
                        .button(egui::RichText::new("📂  Open File").size(15.0))
                        .clicked()
                    {
                        do_open_file = true;
                    }
                    ui.add_space(10.0);
                    if ui
                        .button(egui::RichText::new("📁  Open Folder").size(15.0))
                        .clicked()
                    {
                        do_open_folder = true;
                    }
                });

                if !recents.is_empty() {
                    ui.add_space(32.0);
                    // Center a fixed-width, left-aligned column for the recent list.
                    let avail = ui.available_width();
                    let col_w = 560.0_f32.min((avail - 16.0).max(200.0));
                    let margin = ((avail - col_w) / 2.0).max(0.0);
                    ui.horizontal(|ui| {
                        ui.add_space(margin);
                        ui.vertical(|ui| {
                            ui.set_max_width(col_w);
                            ui.label(egui::RichText::new("Recent").strong());
                            ui.add_space(2.0);
                            ui.separator();
                            for (entry, exists) in &recents {
                                let name = entry
                                    .path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| entry.path.to_string_lossy().to_string());
                                let dir = entry
                                    .path
                                    .parent()
                                    .map(|p| p.to_string_lossy().to_string())
                                    .unwrap_or_default();
                                let when = format_relative_time(entry.last_opened, now);
                                ui.horizontal(|ui| {
                                    let label =
                                        egui::RichText::new(format!("📄 {name}")).size(14.0);
                                    let label = if *exists { label } else { label.weak() };
                                    let resp = ui
                                        .add_enabled(*exists, egui::Button::new(label).frame(false))
                                        .on_hover_text(entry.path.to_string_lossy());
                                    if resp.clicked() {
                                        open_path = Some(entry.path.clone());
                                    }
                                    ui.label(
                                        egui::RichText::new(format!("{dir}   ·   {when}"))
                                            .weak()
                                            .small(),
                                    );
                                });
                            }
                            if total_recent > RECENT_SHOWN {
                                ui.add_space(6.0);
                                let more = if show_all {
                                    "Show less"
                                } else {
                                    "Show more…"
                                };
                                if ui.link(more).clicked() {
                                    toggle_show_all = true;
                                }
                            }
                        });
                    });
                }
            });

        // Deferred actions (run after the borrow on self.recent_files is released).
        if do_open_file {
            self.open_file_dialog();
        }
        if do_open_folder {
            self.open_folder_dialog();
        }
        if let Some(p) = open_path {
            self.open_in_new_tab(p);
        }
        if toggle_show_all {
            self.welcome_show_all = !self.welcome_show_all;
        }
    }

    fn render_tab_content(&mut self, ui: &mut egui::Ui, ctrl_held: bool) -> Option<PathBuf> {
        let mut open_in_new_tab: Option<PathBuf> = None;
        let mut navigation_error = None;

        // Snapshot search state before taking a mutable borrow on the active tab
        let search_is_open = self.search.is_open;
        let active_idx = self.search.active_match_index;

        // No document open → render the welcome / idle page instead.
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            self.render_welcome(ui);
            return None;
        };

        // Push current search match ranges into the cache so the renderer can paint highlights
        if search_is_open && !tab.search_matches.is_empty() {
            let ranges: Vec<_> = tab
                .search_matches
                .iter()
                .map(|m| m.byte_start..m.byte_end)
                .collect();
            let active = tab
                .search_matches
                .get(active_idx)
                .map(|m| m.byte_start..m.byte_end);
            tab.cache.set_search_ranges(ranges);
            tab.cache.set_active_search_range(active);
        } else {
            tab.cache.clear_search_ranges();
        }

        // A search correction grants exactly one full-paint attempt. Some
        // source matches (for example Markdown syntax markers) have no
        // rendered glyph and therefore never record a Y; do not let those
        // matches disable viewport clipping forever.
        let correct_search_this_frame =
            take_search_correction_request(&mut tab.correct_active_search_pending);

        // Content area (no inner CentralPanel needed - we're already in one)
        // Left margin for breathing room, right margin prevents scrollbar/resize-handle overlap jitter
        egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: 8,
                right: CONTENT_RIGHT_RESIZE_GUTTER,
                ..Default::default()
            })
            .show(ui, |ui| {
                // Capture scroll input for manual handling during selection
                let raw_scroll = ui.ctx().input(|i| i.raw_scroll_delta.y);
                let content_rect = ui.available_rect_before_wrap();

                // The renderer owns the ScrollArea now (via show_scrollable),
                // so we configure scroll_source / pending offset / content
                // version through builder methods. The returned ScrollAreaOutput
                // exposes state.offset and inner_rect for the post-render
                // selection-preserving wheel hack below.
                let force_full_render =
                    tab.pending_header_click_key.is_some() || correct_search_this_frame;
                let pending = tab.pending_scroll_offset.take();
                let default_width = content_default_width(self.full_width_content);
                let content_width =
                    content_width_limit(self.full_width_content, content_rect.width());
                let mut scroll_output = CommonMarkViewer::new()
                    .default_implicit_uri_scheme(&tab.base_uri)
                    .max_image_width(Some(800))
                    .default_width(default_width)
                    // Full Width governs the complete document layout. With
                    // the reading cap active, tables reflow within it; their
                    // horizontal scroller still handles minimum-width or
                    // user-resized overflow (#110).
                    .table_max_width(Some(content_width))
                    .indentation_spaces(2)
                    .use_strong_font_family(true)
                    // Frontmatter is metadata, not prose: show it as a
                    // key/value table rather than a thematic break plus a
                    // paragraph of raw `key: value` lines (#117).
                    .render_frontmatter(true)
                    .math_scale(self.math_scale)
                    .show_alt_text_on_hover(true)
                    .syntax_theme_dark("base16-ocean.dark")
                    .syntax_theme_light("base16-ocean.light")
                    .line_height(1.5)
                    .code_line_height(1.3)
                    .paragraph_spacing(2.0)
                    .heading_spacing_above(2.0)
                    .heading_spacing_below(0.75)
                    .content_version(tab.content_version)
                    .pending_scroll_offset(pending)
                    .force_full_render(force_full_render)
                    .scroll_source(egui::scroll_area::ScrollSource {
                        scroll_bar: true,
                        drag: false,
                        mouse_wheel: true,
                    })
                    .show_scrollable(tab.id, ui, &mut tab.cache, &tab.content);

                tab.scroll_offset = scroll_output.state.offset.y;
                tab.last_viewport_height = scroll_output.inner_rect.height();
                tab.last_content_height = scroll_output.content_size.y;

                // If the renderer recorded an exact y for the active match, check
                // whether the current scroll position keeps it visible. If not,
                // schedule a corrective scroll using the recorded y — this fixes
                // line-ratio overshoot/undershoot in image-heavy documents.
                // Corrective scroll for search-match jump. Gated by the
                // one-shot `correct_active_search_pending` flag (set by
                // `scroll_to_active_match`) so it fires at most once per
                // jump — without this gate the block would re-trigger every
                // frame the active match is off-screen, fighting the user's
                // wheel input and locking the view (issue #19).
                if correct_search_this_frame {
                    if let Some(actual_y) = tab.cache.active_search_y() {
                        let current_scroll = scroll_output.state.offset.y;
                        let viewport_top = current_scroll;
                        let viewport_bottom = current_scroll + tab.last_viewport_height;
                        // Consider "not visible" if outside the viewport with a small margin
                        let margin_outside = 20.0_f32;
                        let needs_correction = actual_y < viewport_top + margin_outside
                            || actual_y > viewport_bottom - margin_outside;
                        if needs_correction && tab.last_viewport_height > 0.0 {
                            // Place the active match ~35% from the top of the viewport
                            let inset = tab.last_viewport_height * 0.35;
                            let want_scroll = (actual_y - inset).max(0.0);
                            // Avoid stomping if we're already at the target (within a frame)
                            if (want_scroll - current_scroll).abs() > 2.0 {
                                tab.pending_scroll_offset = Some(want_scroll);
                            }
                        }
                    }
                }

                // Corrective scroll for outline-click. The bootstrap full paint
                // triggered above by `pending_scroll_offset` recorded every
                // heading's precise y under its composite cache key. If the
                // line-ratio first-frame estimate left the target outside the
                // viewport (common in image-heavy docs and the failure mode for
                // duplicate-titled headers before this), snap the next frame to
                // the recorded y. Mirrors the search-match corrective above.
                if let Some(key) = tab.pending_header_click_key.take() {
                    if let Some(actual_y) = tab.cache.get_header_position(&key) {
                        let current_scroll = scroll_output.state.offset.y;
                        let want_scroll = (actual_y - 50.0).max(0.0);
                        if (want_scroll - current_scroll).abs() > 2.0 {
                            tab.pending_scroll_offset = Some(want_scroll);
                        }
                    }
                }

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
                if let Err(error) = tab.navigate_to_link(&clicked_link) {
                    navigation_error = Some(format!("Unable to open {clicked_link}: {error}"));
                }
            }
        }

        if let Some(error) = navigation_error {
            self.error_message = Some(error);
        }

        open_in_new_tab
    }

    /// Render the file explorer sidebar
    /// Returns actions for files to open or close
    fn render_file_explorer(&mut self, ctx: &egui::Context) -> ExplorerAction {
        let mut action = ExplorerAction::default();

        if !self.show_explorer {
            return action;
        }

        let panel = egui::SidePanel::left("file_explorer")
            .resizable(true)
            .default_width(self.explorer_width)
            .width_range(SIDEBAR_MIN_WIDTH..=f32::INFINITY)
            .frame(
                egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin {
                    left: 8,
                    right: EXPLORER_RIGHT_RESIZE_GUTTER,
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
                        self.reconcile_explorer_watches();
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
                        self.reconcile_explorer_watches();
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

                // Pre-load children for all expanded dirs to avoid mutation during render
                for dir in self
                    .file_explorer
                    .expanded_dirs
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                {
                    if self.file_explorer.get_children(&dir).is_none() {
                        self.file_explorer.load_children(&dir);
                    }
                }

                // Take tree to iterate without cloning, then put it back
                let tree = std::mem::take(&mut self.file_explorer.tree);
                // Clone the small set of open tab paths (avoids borrow conflict with &mut self)
                let open_paths = self.open_tab_paths.clone();

                // File tree inside ScrollArea
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .id_salt("file_explorer")
                    .show(ui, |ui| {
                        for node in &tree {
                            let node_action = self.render_tree_node(ui, node, 0, &open_paths);
                            if node_action.file_to_open.is_some() {
                                action.file_to_open = node_action.file_to_open;
                            }
                            if node_action.file_to_close.is_some() {
                                action.file_to_close = node_action.file_to_close;
                            }
                            if node_action.dir_to_toggle.is_some() {
                                action.dir_to_toggle = node_action.dir_to_toggle;
                            }
                        }
                    });

                // Put tree back and apply deferred toggle
                self.file_explorer.tree = tree;
                if let Some(ref dir_path) = action.dir_to_toggle {
                    self.file_explorer.toggle_expanded(dir_path);
                    // Keep the non-recursive explorer watches in sync with the
                    // newly expanded/collapsed directory.
                    self.reconcile_explorer_watches();
                }
            });
        self.explorer_width = panel.response.rect.width();

        action
    }

    /// Calculate flash intensity for a path (0.0 = no flash, 1.0 = full flash)
    fn get_flash_intensity(&self, path: &PathBuf) -> f32 {
        if self.flashing_paths.is_empty() {
            return 0.0;
        }

        if let Some(start_time) = self.flashing_paths.get(path) {
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
    ) -> ExplorerAction {
        let mut action = ExplorerAction::default();
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

                    // Highlight if file is open in a tab
                    let is_open = open_paths.contains(path);
                    let text = if is_open {
                        egui::RichText::new(name.as_str()).strong()
                    } else {
                        egui::RichText::new(name.as_str())
                    };

                    let response = ui.add(
                        egui::Button::selectable(is_open, text)
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
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

                    if response.clicked() {
                        action.file_to_open = Some(path.clone());
                    }
                    // Middle-click to close tab (only if file is open)
                    if response.middle_clicked() && is_open {
                        action.file_to_close = Some(path.clone());
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

                    let response = ui.add(
                        egui::Label::new(name.as_str())
                            .extend()
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

                    // Click directory name to toggle expansion
                    if response.clicked() {
                        should_toggle = true;
                    }
                });

                // Defer toggle to after tree is restored (avoids clone)
                if should_toggle {
                    action.dir_to_toggle = Some(path.clone());
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

                // Render children if expanded (read from node directly, no clone needed)
                if self.file_explorer.is_expanded(path) {
                    if let FileTreeNode::Directory {
                        children: Some(ref child_nodes),
                        ..
                    } = node
                    {
                        for child in child_nodes {
                            let child_action =
                                self.render_tree_node(ui, child, depth + 1, open_paths);
                            if child_action.file_to_open.is_some() {
                                action.file_to_open = child_action.file_to_open;
                            }
                            if child_action.file_to_close.is_some() {
                                action.file_to_close = child_action.file_to_close;
                            }
                            if child_action.dir_to_toggle.is_some() {
                                action.dir_to_toggle = child_action.dir_to_toggle;
                            }
                        }
                    }
                }
            }
        }

        action
    }

    fn render_color_settings(&mut self, ctx: &egui::Context) {
        if !self.show_color_settings_dialog {
            return;
        }

        let mut open = true;
        // Set inside the window closure, applied after `.show()` returns —
        // same one-shot-flag pattern the font/tab-close deferred actions use
        // elsewhere in this file. Each row is independently resettable, so
        // each gets its own flag.
        let mut new_highlight_color: Option<Option<egui::Color32>> = None;
        let mut new_link_color: Option<Option<egui::Color32>> = None;

        egui::Window::new("Colors")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Highlight Color:");
                    // Read the currently *effective* color (override or
                    // theme default — update() already applied Visuals for
                    // this frame) rather than re-deriving egui's own
                    // dark/light selection defaults by hand.
                    let mut color = ui.visuals().selection.bg_fill;
                    let color_btn = ui.color_edit_button_srgba(&mut color);
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Colors Dialog: Highlight Color",
                        "color",
                        &color_btn,
                        None,
                    );
                    if color_btn.changed() {
                        new_highlight_color = Some(Some(color));
                    }

                    let reset_btn =
                        ui.add_enabled(self.highlight_color.is_some(), egui::Button::new("Reset"));
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Colors Dialog: Highlight Color Reset",
                        "button",
                        &reset_btn,
                        None,
                    );
                    if reset_btn.clicked() {
                        new_highlight_color = Some(None);
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Link Color:");
                    let mut color = ui.visuals().hyperlink_color;
                    let color_btn = ui.color_edit_button_srgba(&mut color);
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Colors Dialog: Link Color",
                        "color",
                        &color_btn,
                        None,
                    );
                    if color_btn.changed() {
                        new_link_color = Some(Some(color));
                    }

                    let reset_btn =
                        ui.add_enabled(self.link_color.is_some(), egui::Button::new("Reset"));
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Colors Dialog: Link Color Reset",
                        "button",
                        &reset_btn,
                        None,
                    );
                    if reset_btn.clicked() {
                        new_link_color = Some(None);
                    }
                });
            });

        self.show_color_settings_dialog = open;
        if let Some(color) = new_highlight_color {
            self.highlight_color = color;
        }
        if let Some(color) = new_link_color {
            self.link_color = color;
        }
    }

    fn render_lightbox(&mut self, ctx: &egui::Context) {
        let Some(lightbox) = &mut self.lightbox else {
            return;
        };

        let screen_rect = ctx.available_rect();
        let mut should_close = false;

        // 1. Semi-transparent backdrop (visual only, drawn below Tooltip layers)
        ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("lightbox_backdrop"),
        ))
        .rect_filled(
            screen_rect,
            0.0,
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200),
        );

        // 2. Full-screen input sink at Tooltip order — blocks all input to lower layers.
        //    Clicking on the sink (outside image/close) closes the lightbox.
        let oid = lightbox.open_id;
        egui::Area::new(egui::Id::new("lightbox_sink").with(oid))
            .order(egui::Order::Tooltip)
            .movable(false)
            .fixed_pos(screen_rect.min)
            .show(ctx, |ui| {
                let r = ui.allocate_response(screen_rect.size(), egui::Sense::click());
                if r.clicked() {
                    should_close = true;
                }
            });

        // 3. Apply scroll-wheel zoom (proportional to scroll amount for smooth feel)
        let old_zoom = lightbox.zoom;
        if self.lightbox_scroll != 0.0 {
            // ~10% per scroll notch, smooth with proportional delta
            let factor = (1.0_f32 + self.lightbox_scroll * 0.08).clamp(0.5, 2.0);
            lightbox.zoom = (lightbox.zoom * factor).clamp(0.1, 10.0);
        }
        let zoom_changed = (lightbox.zoom - old_zoom).abs() > f32::EPSILON;

        // 4. Image — GPU texture scaling, no re-rasterization
        let padding = 40.0;
        let area_rect = screen_rect.shrink(padding);
        let area_size = area_rect.size();
        let zoom = lightbox.zoom;
        let base_size = lightbox.base_size;
        let display_size = base_size * zoom;
        let tex_id = lightbox.texture.id();
        let tex_uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));

        // Use fixed_pos instead of anchor to avoid position jitter when content
        // size changes (anchor recalculates position based on laid-out size,
        // causing a one-frame shift when scrollbars appear/disappear).
        let area_pos = egui::pos2(
            screen_rect.center().x - area_size.x / 2.0,
            screen_rect.center().y - area_size.y / 2.0,
        );

        // Pre-compute zoom-to-cursor offset BEFORE ScrollArea renders,
        // so it uses the correct offset on the same frame (no one-frame jitter).
        let pre_offset: Option<egui::Vec2> = if zoom_changed {
            ctx.input(|i| i.pointer.latest_pos()).map(|mouse_pos| {
                // area_pos ≈ viewport origin (no scrollbar/frame margins)
                let mouse_in_vp = mouse_pos - area_pos;

                let old_display = base_size * old_zoom;
                let new_display = display_size;

                let old_content = egui::vec2(
                    old_display.x.max(area_size.x),
                    old_display.y.max(area_size.y),
                );
                let new_content = egui::vec2(
                    new_display.x.max(area_size.x),
                    new_display.y.max(area_size.y),
                );

                let old_pad = (old_content - old_display) * 0.5;
                let new_pad = (new_content - new_display) * 0.5;

                let frac = (mouse_in_vp + lightbox.scroll_offset - old_pad) / old_display;

                let new_offset = frac * new_display + new_pad - mouse_in_vp;

                let max_offset = egui::vec2(
                    (new_content.x - area_size.x).max(0.0),
                    (new_content.y - area_size.y).max(0.0),
                );

                egui::vec2(
                    new_offset.x.clamp(0.0, max_offset.x),
                    new_offset.y.clamp(0.0, max_offset.y),
                )
            })
        } else {
            None
        };

        if let Some(offset) = pre_offset {
            lightbox.scroll_offset = offset;
        }

        egui::Area::new(egui::Id::new("lightbox_image").with(oid))
            .order(egui::Order::Tooltip)
            .movable(false)
            .fixed_pos(area_pos)
            .show(ctx, |ui| {
                ui.set_min_size(area_size);
                ui.set_max_size(area_size);

                // Only enable scroll/drag on axes where image overflows the viewport
                let mut scroll_area = egui::ScrollArea::new([
                    display_size.x > area_size.x,
                    display_size.y > area_size.y,
                ])
                .id_salt(egui::Id::new("lightbox_scroll").with(oid))
                .max_width(area_size.x)
                .max_height(area_size.y)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .scroll_source(egui::scroll_area::ScrollSource {
                    drag: true,
                    ..Default::default()
                });

                // Set the pre-computed offset so ScrollArea renders at the
                // correct position on this frame (avoids one-frame lag).
                if let Some(offset) = pre_offset {
                    scroll_area = scroll_area
                        .horizontal_scroll_offset(offset.x)
                        .vertical_scroll_offset(offset.y);
                }

                let scroll_output = scroll_area.show(ui, |ui| {
                    // Allocate content: at least area_size for centering,
                    // larger when image overflows (for scroll panning)
                    let content_size = egui::vec2(
                        display_size.x.max(area_size.x),
                        display_size.y.max(area_size.y),
                    );
                    let (content_rect, response) =
                        ui.allocate_exact_size(content_size, egui::Sense::click());
                    // Draw image centered within the content rect
                    let image_rect =
                        egui::Rect::from_center_size(content_rect.center(), display_size);
                    if ui.is_rect_visible(image_rect) {
                        let mut mesh = egui::Mesh::with_texture(tex_id);
                        mesh.add_rect_with_uv(image_rect, tex_uv, egui::Color32::WHITE);
                        ui.painter().add(egui::Shape::mesh(mesh));
                    }
                    if response.double_clicked() {
                        lightbox.zoom = 1.0;
                        lightbox.scroll_offset = egui::Vec2::ZERO;
                    }
                    // Click outside image area → close lightbox
                    if response.clicked() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            if !image_rect.contains(pos) {
                                should_close = true;
                            }
                        }
                    }
                });

                // Sync tracking: captures drag-to-scroll changes on non-zoom frames
                if pre_offset.is_none() {
                    lightbox.scroll_offset = scroll_output.state.offset;
                }
            });

        // 5. Close button (top-right)
        egui::Area::new(egui::Id::new("lightbox_close").with(oid))
            .order(egui::Order::Tooltip)
            .movable(false)
            .fixed_pos(egui::pos2(
                screen_rect.right() - 48.0,
                screen_rect.top() + 8.0,
            ))
            .show(ctx, |ui| {
                let btn = ui.add(
                    egui::Button::new(
                        egui::RichText::new("\u{2715}")
                            .size(20.0)
                            .color(egui::Color32::from_gray(200)),
                    )
                    .frame(false),
                );
                if btn.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if btn.clicked() {
                    should_close = true;
                }
            });

        // 6. Zoom indicator (bottom-center) when zoom ≠ 100%
        let zoom_pct = (lightbox.zoom * 100.0).round() as i32;
        if zoom_pct != 100 {
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("lightbox_zoom_indicator").with(oid),
            ))
            .text(
                egui::pos2(screen_rect.center().x, screen_rect.bottom() - 24.0),
                egui::Align2::CENTER_CENTER,
                format!("{zoom_pct}%"),
                egui::FontId::proportional(16.0),
                egui::Color32::from_gray(180),
            );
        }

        // 7. Escape to close
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Escape) {
                should_close = true;
            }
        });

        if should_close {
            self.lightbox = None;
        }
    }
}

impl eframe::App for MarkdownApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        #[cfg(feature = "mcp")]
        {
            self.mcp_bridge.process_commands();
            self.mcp_bridge.inject_raw_input(raw_input);
        }

        // When lightbox is open, intercept scroll events before they reach InputState.
        // This prevents the document's ScrollArea from scrolling while letting us
        // use the captured delta for lightbox zoom.
        self.lightbox_scroll = 0.0;
        if self.lightbox.is_some() {
            for event in &raw_input.events {
                if let egui::Event::MouseWheel { delta, .. } = event {
                    self.lightbox_scroll += delta.y;
                }
            }
            raw_input
                .events
                .retain(|e| !matches!(e, egui::Event::MouseWheel { .. }));
        }
    }

    fn persist_egui_memory(&self) -> bool {
        false // Don't persist egui's internal Memory (widget states, panel sizes, etc.)
              // Our PersistedState handles everything we need; egui's blob grows unbounded (~170KB)
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let state = PersistedState {
            dark_mode: Some(self.dark_mode),
            zoom_level: Some(self.zoom_level),
            show_outline: Some(self.show_outline),
            full_width_content: Some(self.full_width_content),
            math_scale: Some(self.math_scale),
            highlight_color: self.highlight_color.map(|c| [c.r(), c.g(), c.b(), c.a()]),
            link_color: self.link_color.map(|c| [c.r(), c.g(), c.b(), c.a()]),
            open_tabs: Some(self.get_open_tab_paths()),
            active_tab: Some(self.active_tab),
            show_explorer: Some(self.show_explorer),
            explorer_root: self.file_explorer.root.clone(),
            expanded_dirs: Some(self.file_explorer.expanded_dirs.iter().cloned().collect()),
            explorer_sort_order: Some(self.file_explorer.sort_order),
            explorer_width: Some(self.explorer_width),
            outline_width: Some(self.outline_width),
            recent_files: Some(self.recent_files.clone()),
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

        // Poll for asynchronous Explorer root scan completion.
        if self.file_explorer.pending_scan.is_some() {
            if self.file_explorer.poll_pending_scan() {
                log::info!("Explorer directory scan completed");
            }
            ctx.request_repaint_after(Duration::from_millis(100));
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

        // Limit frame rate on virtual displays (e.g., Xvfb) which lack vsync
        // request_repaint_after alone doesn't limit actual frame rate, so we sleep
        // This prevents 500%+ CPU usage during E2E testing
        if self.is_virtual_display {
            std::thread::sleep(Duration::from_millis(16)); // ~60 FPS cap
        }

        // Apply theme settings when dark_mode or a color override changes
        if self.last_applied_dark_mode != Some(self.dark_mode)
            || self.last_applied_highlight_color != self.highlight_color
            || self.last_applied_link_color != self.link_color
        {
            self.last_applied_dark_mode = Some(self.dark_mode);
            self.last_applied_highlight_color = self.highlight_color;
            self.last_applied_link_color = self.link_color;
            let mut visuals = if self.dark_mode {
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
            if let Some(color) = self.highlight_color {
                visuals.selection.bg_fill = color;
            }
            if let Some(color) = self.link_color {
                visuals.hyperlink_color = color;
            }
            ctx.set_visuals(visuals);
        }

        ctx.set_zoom_factor(self.zoom_level);

        // Update window title only when dirty
        if self.title_dirty {
            self.title_dirty = false;
            let title = self.window_title();
            if title != self.last_window_title {
                self.last_window_title = title;
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.last_window_title.clone()));
            }
        }

        // Handle keyboard shortcuts (suppressed when lightbox is open)
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
        let mut open_search = false;
        let mut next_match = false;
        let mut prev_match = false;
        let mut close_search_kb = false;
        let mut keyboard_scroll_action: Option<KeyboardScrollAction> = None;

        // Ctrl+/- zoom: applies to lightbox when open, document otherwise
        ctx.input(|i| {
            if i.modifiers.ctrl
                && (i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals))
            {
                zoom_delta = 0.1;
            }
            if i.modifiers.ctrl && i.key_pressed(egui::Key::Minus) {
                zoom_delta = -0.1;
            }
            if i.modifiers.ctrl && i.key_pressed(egui::Key::Num0) {
                if self.lightbox.is_some() {
                    // Reset lightbox zoom
                    zoom_delta = 0.0;
                    if let Some(lb) = &mut self.lightbox {
                        lb.zoom = 1.0;
                    }
                } else {
                    zoom_delta = 1.0 - self.zoom_level;
                }
            }
        });

        // Apply zoom to lightbox or document
        if zoom_delta != 0.0 {
            if let Some(lb) = &mut self.lightbox {
                let factor = if zoom_delta > 0.0 { 1.25 } else { 1.0 / 1.25 };
                lb.zoom = (lb.zoom * factor).clamp(0.1, 10.0);
            } else {
                self.zoom_level = (self.zoom_level + zoom_delta).clamp(0.5, 3.0);
            }
        }

        if self.lightbox.is_none() {
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
                // Ctrl+T: Open a file in a new tab
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
                // Ctrl + scroll wheel for zoom
                if i.modifiers.ctrl && i.raw_scroll_delta.y != 0.0 {
                    self.zoom_level = (self.zoom_level
                        + if i.raw_scroll_delta.y > 0.0 {
                            0.1
                        } else {
                            -0.1
                        })
                    .clamp(0.5, 3.0);
                }
                // F5: Toggle file watching
                if i.key_pressed(egui::Key::F5) {
                    toggle_watch = true;
                }
                // Ctrl+F: Open find bar (or refocus if already open)
                if i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::F) {
                    open_search = true;
                }
                // While the find bar is open, intercept Enter / Shift+Enter / ↑↓ / Esc.
                // Up/Down are safe to bind even when the singleline TextEdit has focus
                // because it doesn't use vertical arrows for cursor movement.
                if self.search.is_open {
                    if i.key_pressed(egui::Key::Enter) {
                        if i.modifiers.shift {
                            prev_match = true;
                        } else {
                            next_match = true;
                        }
                    }
                    if i.key_pressed(egui::Key::ArrowDown) {
                        next_match = true;
                    }
                    if i.key_pressed(egui::Key::ArrowUp) {
                        prev_match = true;
                    }
                    if i.key_pressed(egui::Key::Escape) {
                        close_search_kb = true;
                    }
                }
                // Plain document scroll keys reuse the existing pending-scroll pipeline.
                // Arrow keys stay available for search navigation while the find bar is open.
                let no_scroll_modifier =
                    !i.modifiers.ctrl && !i.modifiers.alt && !i.modifiers.command;
                if no_scroll_modifier {
                    if !self.search.is_open {
                        if i.key_pressed(egui::Key::ArrowUp) {
                            keyboard_scroll_action = Some(KeyboardScrollAction::LineUp);
                        }
                        if i.key_pressed(egui::Key::ArrowDown) {
                            keyboard_scroll_action = Some(KeyboardScrollAction::LineDown);
                        }
                    }
                    if i.key_pressed(egui::Key::PageUp) {
                        keyboard_scroll_action = Some(KeyboardScrollAction::PageUp);
                    }
                    if i.key_pressed(egui::Key::PageDown) {
                        keyboard_scroll_action = Some(KeyboardScrollAction::PageDown);
                    }
                }
            });
        } // end lightbox guard

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
            self.navigate_active_history(true);
        }
        if go_forward {
            self.navigate_active_history(false);
        }

        // Search bar actions (Ctrl+F open, Enter/Shift+Enter cycle, Esc close)
        if open_search {
            self.search.is_open = true;
            self.search.focus_requested = true;
        }
        if next_match {
            self.jump_match(1);
        }
        if prev_match {
            self.jump_match(-1);
        }
        if close_search_kb {
            self.close_search();
        }
        if let Some(action) = keyboard_scroll_action {
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                let target = keyboard_scroll_target(
                    tab.scroll_offset,
                    tab.last_viewport_height,
                    tab.last_content_height,
                    action,
                );
                tab.pending_scroll_offset = Some(target);
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
                        .add(egui::Button::new("Open File in New Tab...").shortcut_text("Ctrl+T"))
                        .clicked()
                    {
                        self.open_file_dialog();
                        ui.close();
                    }

                    if ui.button("Open Folder...").clicked() {
                        self.open_folder_dialog();
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

                    if ui
                        .add(egui::Button::new("Find...").shortcut_text("Ctrl+F"))
                        .clicked()
                    {
                        self.search.is_open = true;
                        self.search.focus_requested = true;
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
                        self.navigate_active_history(true);
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
                        self.navigate_active_history(false);
                        ui.close();
                    }
                });

                #[cfg_attr(not(feature = "mcp"), allow(unused_variables))]
                let view_menu = ui.menu_button("View", |ui| {
                    let theme_text = if self.dark_mode {
                        "☀ Light Mode"
                    } else {
                        "🌙 Dark Mode"
                    };
                    let theme_btn = ui.add(egui::Button::new(theme_text).shortcut_text("Ctrl+D"));
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Menu: View → Dark Mode",
                        "button",
                        &theme_btn,
                        Some(if self.dark_mode { "dark" } else { "light" }),
                    );
                    if theme_btn.clicked() {
                        self.dark_mode = !self.dark_mode;
                        ui.close();
                    }

                    let explorer_text = if self.show_explorer {
                        "✓ Show Explorer"
                    } else {
                        "Show Explorer"
                    };
                    let explorer_btn =
                        ui.add(egui::Button::new(explorer_text).shortcut_text("Ctrl+Shift+E"));
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Menu: View → Show Explorer",
                        "button",
                        &explorer_btn,
                        Some(if self.show_explorer { "on" } else { "off" }),
                    );
                    if explorer_btn.clicked() {
                        self.show_explorer = !self.show_explorer;
                        ui.close();
                    }

                    let outline_text = if self.show_outline {
                        "✓ Show Outline"
                    } else {
                        "Show Outline"
                    };
                    let outline_btn =
                        ui.add(egui::Button::new(outline_text).shortcut_text("Ctrl+Shift+O"));
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Menu: View → Show Outline",
                        "button",
                        &outline_btn,
                        Some(if self.show_outline { "on" } else { "off" }),
                    );
                    if outline_btn.clicked() {
                        self.show_outline = !self.show_outline;
                        ui.close();
                    }

                    let full_width_text = if self.full_width_content {
                        "✓ Full Width"
                    } else {
                        "Full Width"
                    };
                    let full_width_btn = ui.add(egui::Button::new(full_width_text));
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Menu: View → Full Width",
                        "button",
                        &full_width_btn,
                        Some(if self.full_width_content { "on" } else { "off" }),
                    );
                    if full_width_btn.clicked() {
                        self.full_width_content = !self.full_width_content;
                        ui.close();
                    }

                    let highlight_btn = ui.add(egui::Button::new("Colors…"));
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Menu: View → Colors",
                        "button",
                        &highlight_btn,
                        None,
                    );
                    if highlight_btn.clicked() {
                        self.show_color_settings_dialog = true;
                        ui.close();
                    }

                    ui.separator();

                    let zoom_in_btn = ui.add(egui::Button::new("Zoom In").shortcut_text("Ctrl++"));
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Menu: View → Zoom In",
                        "button",
                        &zoom_in_btn,
                        None,
                    );
                    if zoom_in_btn.clicked() {
                        self.zoom_level = (self.zoom_level + 0.1).min(3.0);
                        ui.close();
                    }
                    let zoom_out_btn =
                        ui.add(egui::Button::new("Zoom Out").shortcut_text("Ctrl+-"));
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Menu: View → Zoom Out",
                        "button",
                        &zoom_out_btn,
                        None,
                    );
                    if zoom_out_btn.clicked() {
                        self.zoom_level = (self.zoom_level - 0.1).max(0.5);
                        ui.close();
                    }
                    let reset_zoom_btn =
                        ui.add(egui::Button::new("Reset Zoom").shortcut_text("Ctrl+0"));
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Menu: View → Reset Zoom",
                        "button",
                        &reset_zoom_btn,
                        None,
                    );
                    if reset_zoom_btn.clicked() {
                        self.zoom_level = 1.0;
                        ui.close();
                    }

                    ui.separator();
                    // Distinct from zoom: zoom scales the whole UI, leaving
                    // formulas and prose in the same ratio. This changes only
                    // the formulas, relative to the text around them.
                    ui.menu_button("Formula Size", |ui| {
                        for (label, scale) in [
                            ("100%", 1.0f32),
                            ("110%", 1.1),
                            ("125%", 1.25),
                            ("150%", 1.5),
                        ] {
                            let selected = (self.math_scale - scale).abs() < 0.001;
                            let text = if selected {
                                format!("✓ {label}")
                            } else {
                                format!("   {label}")
                            };
                            let btn = ui.add(egui::Button::new(text));
                            #[cfg(feature = "mcp")]
                            self.mcp_bridge.register_widget(
                                &format!("Menu: View → Formula Size → {label}"),
                                "button",
                                &btn,
                                Some(if selected { "selected" } else { "unselected" }),
                            );
                            if btn.clicked() {
                                self.math_scale = scale;
                                ui.close();
                            }
                        }
                    });
                });
                #[cfg(feature = "mcp")]
                self.mcp_bridge
                    .register_widget("Menu: View", "button", &view_menu.response, None);

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
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(tab.path.display().to_string())
                                        .small()
                                        .color(ui.visuals().weak_text_color()),
                                )
                                .truncate(),
                            );
                        }
                    }
                });
            });
        });

        // Handle navigation button clicks (must be after menu bar UI)
        if go_back {
            self.navigate_active_history(true);
        }
        if go_forward {
            self.navigate_active_history(false);
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

        // Find bar (conditional, between error bar and tab bar)
        let search_outcome = self.render_search_bar(ctx);
        // Rebuild matches if query or tab changed (TextEdit may have mutated the query)
        self.maybe_rebuild_search();
        if search_outcome.close_requested {
            self.close_search();
        }
        if search_outcome.next_clicked {
            self.jump_match(1);
        }
        if search_outcome.prev_clicked {
            self.jump_match(-1);
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
        let explorer_action = self.render_file_explorer(ctx);

        // Explorer matches document links: primary click replaces the active
        // document, while Ctrl/Cmd+click opens (or focuses) a separate tab.
        if let Some(path) = explorer_action.file_to_open {
            if ctrl_held {
                self.open_in_new_tab(path);
            } else {
                self.navigate_active_tab_to(&path);
            }
        }

        // Close tab from explorer (middle-click on open file)
        if let Some(path) = explorer_action.file_to_close {
            if let Some(idx) = self.tabs.iter().position(|t| t.path == path) {
                self.close_tab(idx);
            }
        }

        // Outline sidebar (right) - at top level for proper layout
        self.render_outline(ctx);

        // Main content area
        let mut open_in_new_tab: Option<PathBuf> = None;
        let path_before_render = self.tabs.get(self.active_tab).map(|tab| tab.path.clone());
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(egui::Margin::ZERO))
            .show(ctx, |ui| {
                open_in_new_tab = self.render_tab_content(ui, ctrl_held);
            });

        // A primary-clicked document link navigates inside Tab while it is
        // rendered. Synchronize the app-level state that depends on tab paths.
        let path_after_render = self.tabs.get(self.active_tab).map(|tab| tab.path.clone());
        if path_after_render != path_before_render {
            if let Some(path) = path_after_render {
                self.record_recent(&path);
            }
            self.title_dirty = true;
            self.refresh_open_tab_paths();
            if self.watch_enabled {
                self.update_watched_paths();
            }
        }

        // Open link in new tab if requested
        if let Some(path) = open_in_new_tab {
            self.open_in_new_tab(path);
        }

        // Check if a mermaid diagram was clicked → open lightbox
        // Texture is pre-rasterized at 2x by background thread — no work on click
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            if let Some((texture, base_size)) = tab.cache.take_clicked_mermaid() {
                self.lightbox_open_count += 1;
                self.lightbox = Some(LightboxState {
                    texture: LightboxTexture::Owned(texture),
                    base_size,
                    zoom: 1.0,
                    open_id: self.lightbox_open_count,
                    scroll_offset: egui::Vec2::ZERO,
                });
            }
            if let Some((tex_id, base_size)) = tab.cache.take_clicked_image() {
                self.lightbox_open_count += 1;
                self.lightbox = Some(LightboxState {
                    texture: LightboxTexture::Loaded(tex_id),
                    base_size,
                    zoom: 1.0,
                    open_id: self.lightbox_open_count,
                    scroll_offset: egui::Vec2::ZERO,
                });
            }
        }

        // Lightbox overlay for enlarged diagrams or images
        self.render_lightbox(ctx);

        // Highlight color picker window
        self.render_color_settings(ctx);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_recent_dedupes_and_moves_to_front() {
        let mut v = Vec::new();
        push_recent(&mut v, Path::new("/a.md"), 100);
        push_recent(&mut v, Path::new("/b.md"), 200);
        push_recent(&mut v, Path::new("/a.md"), 300); // re-open a → moves to front, no dup
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].path, PathBuf::from("/a.md"));
        assert_eq!(v[0].last_opened, 300);
        assert_eq!(v[1].path, PathBuf::from("/b.md"));
    }

    #[test]
    fn push_recent_caps_length_keeping_newest() {
        let mut v = Vec::new();
        for i in 0..(RECENT_FILES_CAP + 5) {
            push_recent(&mut v, Path::new(&format!("/f{i}.md")), i as u64);
        }
        assert_eq!(v.len(), RECENT_FILES_CAP);
        // The most recently pushed entry is at the front.
        assert_eq!(
            v[0].path,
            PathBuf::from(format!("/f{}.md", RECENT_FILES_CAP + 4))
        );
    }

    #[test]
    fn relative_time_buckets() {
        let now = 1_000_000u64;
        assert_eq!(format_relative_time(now, now), "just now");
        assert_eq!(format_relative_time(now - 120, now), "2m ago");
        assert_eq!(format_relative_time(now - 7200, now), "2h ago");
        assert_eq!(format_relative_time(now - 90_000, now), "yesterday");
        assert_eq!(format_relative_time(now - 3 * 86_400, now), "3d ago");
        // Clock skew (future timestamp) must not underflow.
        assert_eq!(format_relative_time(now + 500, now), "just now");
    }

    #[test]
    fn wayland_socket_path_mirrors_libwayland_lookup() {
        assert_eq!(
            wayland_display_socket_path(Some("wayland-0"), OsStr::new("/run/user/1000")),
            Some(PathBuf::from("/run/user/1000/wayland-0"))
        );
        assert_eq!(
            wayland_display_socket_path(Some("/custom/wayland-1"), OsStr::new("/run/user/1000")),
            Some(PathBuf::from("/custom/wayland-1"))
        );
        // Nothing usable to probe when the variables are missing or empty.
        assert_eq!(
            wayland_display_socket_path(None, OsStr::new("/run/user/1000")),
            None
        );
        assert_eq!(
            wayland_display_socket_path(Some(""), OsStr::new("/run/user/1000")),
            None
        );
        assert_eq!(
            wayland_display_socket_path(Some("wayland-0"), OsStr::new("")),
            None
        );
    }

    #[test]
    fn default_terminal_launch_detaches() {
        let args = Args::try_parse_from(["md-viewer", "README.md"]).unwrap();
        assert!(should_detach(&args, true));
    }

    #[test]
    fn non_terminal_launch_does_not_detach() {
        let args = Args::try_parse_from(["md-viewer", "README.md"]).unwrap();
        assert!(!should_detach(&args, false));
    }

    #[test]
    fn foreground_flag_disables_detach() {
        let args = Args::try_parse_from(["md-viewer", "--foreground", "README.md"]).unwrap();
        assert!(!should_detach(&args, true));
    }

    #[test]
    fn hidden_no_detach_marker_disables_detach() {
        let args = Args::try_parse_from(["md-viewer", "--no-detach", "README.md"]).unwrap();
        assert!(!should_detach(&args, true));
    }

    #[test]
    fn child_args_preserve_user_args_and_append_marker() {
        let child_args = child_args_with_no_detach([
            OsString::from("md-viewer"),
            OsString::from("README.md"),
            OsString::from("--no-watch"),
        ]);

        assert_eq!(
            child_args,
            vec![
                OsString::from("README.md"),
                OsString::from("--no-watch"),
                OsString::from("--no-detach"),
            ]
        );
    }

    #[test]
    fn child_args_insert_marker_before_double_dash() {
        let child_args = child_args_with_no_detach([
            OsString::from("md-viewer"),
            OsString::from("--"),
            OsString::from("README.md"),
        ]);

        assert_eq!(
            child_args,
            vec![
                OsString::from("--no-detach"),
                OsString::from("--"),
                OsString::from("README.md"),
            ]
        );
    }

    #[test]
    fn hidden_no_detach_marker_is_not_in_help() {
        use clap::CommandFactory;

        let help = Args::command().render_long_help().to_string();
        assert!(help.contains("--foreground"));
        assert!(!help.contains("--no-detach"));
    }

    #[test]
    fn version_flag_reports_crate_version() {
        use clap::CommandFactory;

        let version = Args::command()
            .get_version()
            .expect("--version should be wired up")
            .to_string();
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn capped_content_width_uses_optimal_width() {
        assert_eq!(content_default_width(false), Some(600));
    }

    #[test]
    fn full_width_content_uses_available_width() {
        assert_eq!(content_default_width(true), None);
    }

    #[test]
    fn capped_content_applies_the_same_limit_to_tables() {
        assert_eq!(content_width_limit(false, 900.0), 600);
        assert_eq!(content_width_limit(false, 480.0), 480);
    }

    #[test]
    fn full_width_tables_use_the_available_content_width() {
        assert_eq!(content_width_limit(true, 900.0), 900);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn portal_introspection_detects_file_chooser_interface() {
        let output = br#"interface org.freedesktop.portal.FileChooser {"#;
        assert!(portal_introspection_has_file_chooser(output));
        assert!(!portal_introspection_has_file_chooser(
            b"interface org.freedesktop.portal.OpenURI {"
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn empty_folder_picker_output_means_cancelled() {
        assert_eq!(path_from_picker_output(Vec::new()), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn folder_picker_output_preserves_path_bytes() {
        let bytes = b"/tmp/example folder".to_vec();
        assert_eq!(
            path_from_picker_output(bytes),
            Some(PathBuf::from("/tmp/example folder"))
        );
    }

    #[test]
    fn search_correction_request_is_consumed_even_without_a_rendered_match() {
        let mut pending = true;

        assert!(take_search_correction_request(&mut pending));
        assert!(!pending);
        assert!(!take_search_correction_request(&mut pending));
    }

    #[test]
    fn find_matches_empty_query_returns_none() {
        assert_eq!(find_matches("hello world", ""), vec![]);
    }

    #[test]
    fn find_matches_empty_content_returns_none() {
        assert_eq!(find_matches("", "hello"), vec![]);
    }

    #[test]
    fn find_matches_single_ascii() {
        let m = find_matches("hello world", "world");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].byte_start, 6);
        assert_eq!(m[0].byte_end, 11);
        assert_eq!(m[0].line_number, 1);
    }

    #[test]
    fn find_matches_case_insensitive_ascii() {
        let m = find_matches("Hello WORLD", "world");
        assert_eq!(m.len(), 1);
        assert_eq!(&"Hello WORLD"[m[0].byte_start..m[0].byte_end], "WORLD");
    }

    #[test]
    fn find_matches_multiple_per_line() {
        // "foo bar foo baz foo" → 3 matches, all on line 1
        let m = find_matches("foo bar foo baz foo", "foo");
        assert_eq!(m.len(), 3);
        assert!(m.iter().all(|sm| sm.line_number == 1));
        let starts: Vec<_> = m.iter().map(|sm| sm.byte_start).collect();
        assert_eq!(starts, vec![0, 8, 16]);
    }

    #[test]
    fn find_matches_line_numbers_one_based() {
        let content = "first line\nsecond match here\nthird\nmatch on four";
        let m = find_matches(content, "match");
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].line_number, 2);
        assert_eq!(m[1].line_number, 4);
    }

    #[test]
    fn find_matches_preserves_byte_offsets_with_utf8() {
        // "café résumé naïve" — each accented char is 2 bytes in UTF-8
        let content = "café résumé naïve";
        let m = find_matches(content, "café");
        assert_eq!(m.len(), 1);
        // Confirm byte range slices back to the original substring
        assert_eq!(&content[m[0].byte_start..m[0].byte_end], "café");
    }

    #[test]
    fn find_matches_skips_cross_newline_matches() {
        // Query "oo\nb" theoretically spans two lines — should not be reported
        let content = "foo\nbar";
        let m = find_matches(content, "oo\nb");
        assert_eq!(m, vec![]);
    }

    #[test]
    fn find_matches_overlapping_uses_match_indices_semantics() {
        // str::match_indices does NOT overlap matches: "aaaa" / "aa" → [0, 2]
        let m = find_matches("aaaa", "aa");
        let starts: Vec<_> = m.iter().map(|sm| sm.byte_start).collect();
        assert_eq!(starts, vec![0, 2]);
    }

    #[test]
    fn find_matches_skips_image_alt_text() {
        // Image alt text isn't visibly rendered (used only as hover/screen-reader text).
        // A match inside the alt portion of `![alt](url)` would have no visible target
        // for highlighting/scrolling — exclude it.
        let content = "See ![Syntax docs](pic.png) and the syntax guide.";
        let m = find_matches(content, "syntax");
        let starts: Vec<_> = m.iter().map(|sm| sm.byte_start).collect();
        assert_eq!(m.len(), 1, "got {:?}", m);
        // The surviving match is "syntax" in "the syntax guide", not "Syntax" in alt
        assert_eq!(&content[starts[0]..starts[0] + 6], "syntax");
    }

    #[test]
    fn find_matches_skips_image_url() {
        // Image URL isn't visibly rendered — exclude matches inside.
        let content = "![ok](path/syntax.png) more text";
        let m = find_matches(content, "syntax");
        assert_eq!(m.len(), 0, "URL match should be filtered: {:?}", m);
    }

    #[test]
    fn find_matches_skips_link_url_keeps_link_text() {
        // Link text IS rendered (visible clickable text). Link URL is not.
        let content = "Click [syntax docs](https://example.com/syntax.html) here";
        let m = find_matches(content, "syntax");
        // Two raw matches: "syntax" in link text (visible) + "syntax" in URL (invisible)
        assert_eq!(m.len(), 1, "should keep link text, drop URL: {:?}", m);
        let s = m[0].byte_start;
        // The kept match is "syntax" in "[syntax docs]" (visible link text)
        assert_eq!(&content[s..s + 6], "syntax");
        // And it's the FIRST occurrence (inside the brackets), not the URL one
        assert!(s < content.find("https").unwrap());
    }

    #[test]
    fn find_matches_multiple_alt_text_images() {
        let content = "![a one](u.png) one ![two two](w.png) two";
        let m = find_matches(content, "one");
        let starts: Vec<_> = m.iter().map(|sm| sm.byte_start).collect();
        // Two raw "one" matches in alt + 1 in visible text → only 1 survives
        assert_eq!(m.len(), 1);
        assert_eq!(&content[starts[0]..starts[0] + 3], "one");

        let m2 = find_matches(content, "two");
        // Two raw "two" matches in alt + 1 in visible text → only 1 survives
        assert_eq!(m2.len(), 1);
    }

    #[test]
    fn shortcode_search_range_uses_raw_source_bytes() {
        let content = "before :pushpin: after";
        let matches = find_matches(content, ":pushpin:");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].byte_start, 7);
        assert_eq!(matches[0].byte_end, 16);
        assert_eq!(
            &content[matches[0].byte_start..matches[0].byte_end],
            ":pushpin:"
        );
    }

    #[test]
    fn rendered_emoji_does_not_match_shortcode_only_source() {
        assert!(find_matches("before :pushpin: after", "📌").is_empty());
    }

    #[test]
    fn literal_unicode_emoji_still_matches_raw_source() {
        let matches = find_matches("before 📌 after", "📌");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].byte_start, 7);
        assert_eq!(matches[0].byte_end, 11);
    }

    #[test]
    fn shared_resolver_keeps_absolute_paths_and_literal_percent_names() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-141-edges-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        fs::create_dir_all(&root).unwrap();
        let home = root.join("home.md");
        let abs = root.join("absolute.md");
        // `%` not followed by two hex digits is not an escape and must survive.
        let literal = root.join("100%done.md");
        fs::write(&home, "# Home").unwrap();
        fs::write(&abs, "# Abs").unwrap();
        fs::write(&literal, "# Literal").unwrap();

        let mut tab = Tab::new(home.clone()).unwrap();

        let abs_link = abs.to_string_lossy().to_string();
        assert!(tab.navigate_to_link(&abs_link).unwrap(), "absolute path");
        assert_eq!(tab.path, abs.canonicalize().unwrap());

        assert!(
            tab.navigate_to_link("100%done.md").unwrap(),
            "literal percent"
        );
        assert_eq!(tab.path, literal.canonicalize().unwrap());

        // an anchor-only destination is still not a path
        assert!(!tab.navigate_to_link("#section").unwrap());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn plain_click_and_ctrl_click_resolve_the_same_link_identically() {
        // #141: `resolve_link` (ctrl+click) and `navigate_to_link` (plain click)
        // must resolve a destination identically. #78 taught percent-decoding
        // and `file://` to the former only, so an encoded space opened in a new
        // tab on ctrl+click and silently did nothing on a plain click.
        let root = std::env::temp_dir().join(format!(
            "md-viewer-141-baseline-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        let spaced = root.join("docs with spaces");
        fs::create_dir_all(&spaced).unwrap();
        let home = root.join("home.md");
        let guide = spaced.join("guide.md");
        fs::write(&home, "# Home").unwrap();
        fs::write(&guide, "# Guide").unwrap();

        let mut tab = Tab::new(home.clone()).unwrap();
        let encoded = "docs%20with%20spaces/guide.md";

        // ctrl+click path: decodes, resolves
        assert_eq!(
            tab.resolve_link(encoded),
            Some(guide.canonicalize().unwrap()),
            "resolve_link should decode percent escapes"
        );

        // plain-click path must reach the same file
        assert!(
            tab.navigate_to_link(encoded).unwrap(),
            "navigate_to_link must decode percent escapes too"
        );
        assert_eq!(tab.path, guide.canonicalize().unwrap());
        assert!(tab.navigate_to_path(&home).unwrap());

        // same divergence for file:// destinations
        let uri = format!("file://{}", guide.canonicalize().unwrap().display());
        assert_eq!(
            tab.resolve_link(&uri),
            Some(guide.canonicalize().unwrap()),
            "resolve_link should accept file:// URIs"
        );
        assert!(
            tab.navigate_to_link(&uri).unwrap(),
            "navigate_to_link must accept file:// URIs too"
        );
        assert_eq!(tab.path, guide.canonicalize().unwrap());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn tab_history_changes_only_after_successful_loads() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-history-safety-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("first.md");
        let second = root.join("second.md");
        fs::write(&first, "# First").unwrap();
        fs::write(&second, "# Second").unwrap();

        let mut tab = Tab::new(first.clone()).unwrap();
        assert!(tab.navigate_to_link("second.md").unwrap());
        assert_eq!(tab.path, second.canonicalize().unwrap());
        assert!(tab.can_go_back());

        fs::remove_file(&first).unwrap();
        assert!(tab.navigate_back().is_err());
        assert_eq!(tab.path, second.canonicalize().unwrap());
        assert!(tab.can_go_back());
        assert!(!tab.can_go_forward());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unreadable_new_tab_returns_an_error() {
        let missing = std::env::temp_dir().join(format!(
            "md-viewer-missing-{}-{}.md",
            std::process::id(),
            now_epoch_secs()
        ));
        assert!(Tab::new(missing).is_err());
    }

    #[test]
    fn heading_parser_keeps_raw_shortcode_and_source_identity() {
        let markdown = "# Doc\n\n## Pin :pushpin:\n\n## Pin :pushpin:\n";
        let parsed = parse_headers(markdown);

        assert_eq!(parsed.outline_headers.len(), 3);
        assert_eq!(parsed.outline_headers[1].title, "Pin :pushpin:");
        assert_eq!(
            parsed.outline_headers[1].source_start,
            markdown.find("## Pin").unwrap()
        );
        assert_eq!(
            parsed.outline_headers[2].source_start,
            markdown.rfind("## Pin").unwrap()
        );
        assert_eq!(
            header_position_key(parsed.outline_headers[1].source_start),
            "heading-source:7"
        );
    }

    #[test]
    fn unknown_shortcode_heading_stays_raw() {
        let parsed = parse_headers("# Doc\n\n## Pin :not_a_gemoji:\n");
        assert_eq!(parsed.outline_headers[1].title, "Pin :not_a_gemoji:");
    }

    #[test]
    fn heading_parser_uses_commonmark_for_formatting_links_and_setext() {
        let markdown = concat!(
            "# **Bold** and `code` [link](guide.md)\n\n",
            "Setext title\n---\n\n",
            "~~~markdown\n# Hidden heading\n~~~\n",
        );
        let parsed = parse_headers(markdown);

        assert_eq!(parsed.outline_headers.len(), 2);
        assert_eq!(parsed.outline_headers[0].title, "Bold and code link");
        assert_eq!(parsed.outline_headers[0].level, 1);
        assert_eq!(parsed.outline_headers[1].title, "Setext title");
        assert_eq!(parsed.outline_headers[1].level, 2);
        assert_eq!(
            parsed.outline_headers[1].source_start,
            markdown.find("Setext").unwrap()
        );
    }

    #[test]
    fn bare_existing_markdown_path_is_registered_as_local_link() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-auto-link-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        fs::create_dir_all(root.join("docs")).unwrap();
        let document = root.join("index.md");
        fs::write(&document, "See docs/guide.md for details.").unwrap();
        fs::write(root.join("docs/guide.md"), "# Guide").unwrap();

        let links = parse_local_links("See docs/guide.md for details.", &document);
        assert_eq!(links, vec!["docs/guide.md"]);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inline_code_markdown_path_can_touch_chinese_prose_and_punctuation() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-inline-link-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        fs::create_dir_all(root.join("github/research/docs")).unwrap();
        let document = root.join("index.md");
        fs::write(&document, "# Index").unwrap();
        fs::write(root.join("github/research/docs/audit.md"), "# Audit").unwrap();

        let content = "触发文档：`github/research/docs/audit.md`：详细说明";
        assert_eq!(
            parse_local_links(content, &document),
            vec!["github/research/docs/audit.md"]
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn nonexistent_bare_markdown_path_is_not_registered() {
        let document = std::env::temp_dir().join("md-viewer-auto-link-missing/index.md");
        assert!(parse_local_links("See missing.md for details.", &document).is_empty());
    }

    #[test]
    fn fenced_and_indented_code_never_register_bare_markdown_paths() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-fenced-link-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        fs::create_dir_all(&root).unwrap();
        let document = root.join("index.md");
        fs::write(&document, "# Index").unwrap();
        fs::write(root.join("guide.md"), "# Guide").unwrap();

        let code_blocks = concat!(
            "~~~text\nguide.md\n~~~\n\n",
            "````text\nguide.md\n````\n\n",
            "    guide.md\n",
        );
        assert!(parse_local_links(code_blocks, &document).is_empty());
        assert_eq!(parse_local_links("`guide.md`", &document), ["guide.md"]);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn absolute_path_and_file_uri_are_registered_as_local_links() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-auto-link-uri-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        fs::create_dir_all(root.join("docs with spaces")).unwrap();
        let document = root.join("index.md");
        let absolute_target = root.join("guide.md");
        let uri_target = root.join("docs with spaces/guide.md");
        fs::write(&document, "# Index").unwrap();
        fs::write(&absolute_target, "# Absolute guide").unwrap();
        fs::write(&uri_target, "# URI guide").unwrap();

        let absolute = absolute_target.to_string_lossy();
        let file_uri = url::Url::from_file_path(&uri_target).unwrap().to_string();
        let content = format!("`{absolute}` and `{file_uri}`");
        let links = parse_local_links(&content, &document);

        assert_eq!(links, vec![absolute.to_string(), file_uri]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn commonmark_reference_with_encoded_path_is_registered() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-reference-link-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        fs::create_dir_all(root.join("docs with spaces")).unwrap();
        let document = root.join("index.md");
        fs::write(&document, "# Index").unwrap();
        fs::write(root.join("docs with spaces/guide(1).md"), "# Guide").unwrap();

        let content =
            "Read [the guide][guide].\n\n[guide]: docs%20with%20spaces/guide(1).md \"Title\"";
        assert_eq!(
            parse_local_links(content, &document),
            vec!["docs%20with%20spaces/guide(1).md"]
        );
        assert!(
            resolve_local_link_path("docs%20with%20spaces/guide(1).md", &root)
                .is_some_and(|path| path.is_file())
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tab_path_navigation_tracks_back_and_forward_history() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-navigation-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("first.md");
        let second = root.join("second.md");
        fs::write(&first, "# First").unwrap();
        fs::write(&second, "# Second").unwrap();

        let mut tab = Tab::new(first.canonicalize().unwrap()).unwrap();
        assert!(tab.navigate_to_path(&second).unwrap());
        assert_eq!(tab.path, second.canonicalize().unwrap());
        assert!(tab.can_go_back());
        assert!(!tab.can_go_forward());

        tab.navigate_back().unwrap();
        assert_eq!(tab.path, first.canonicalize().unwrap());
        assert!(tab.can_go_forward());

        tab.navigate_forward().unwrap();
        assert_eq!(tab.path, second.canonicalize().unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn keyboard_scroll_target_moves_by_line_step() {
        assert_eq!(
            keyboard_scroll_target(100.0, 500.0, 2_000.0, KeyboardScrollAction::LineDown),
            148.0
        );
        assert_eq!(
            keyboard_scroll_target(100.0, 500.0, 2_000.0, KeyboardScrollAction::LineUp),
            52.0
        );
    }

    #[test]
    fn keyboard_scroll_target_moves_by_page_step() {
        assert_eq!(
            keyboard_scroll_target(1_000.0, 500.0, 3_000.0, KeyboardScrollAction::PageDown),
            1_450.0
        );
        assert_eq!(
            keyboard_scroll_target(1_000.0, 500.0, 3_000.0, KeyboardScrollAction::PageUp),
            550.0
        );
    }

    #[test]
    fn keyboard_scroll_target_clamps_to_document_bounds() {
        assert_eq!(
            keyboard_scroll_target(10.0, 500.0, 2_000.0, KeyboardScrollAction::PageUp),
            0.0
        );
        assert_eq!(
            keyboard_scroll_target(1_490.0, 500.0, 2_000.0, KeyboardScrollAction::PageDown),
            1_500.0
        );
    }

    #[test]
    fn keyboard_scroll_target_handles_short_documents() {
        assert_eq!(
            keyboard_scroll_target(25.0, 500.0, 300.0, KeyboardScrollAction::LineDown),
            0.0
        );
    }

    #[test]
    fn watcher_retry_sequence_stops_at_configured_limit() {
        assert_eq!(next_watcher_retry(0), Some(1));
        assert_eq!(next_watcher_retry(1), Some(2));
        assert_eq!(next_watcher_retry(2), Some(3));
        assert_eq!(next_watcher_retry(3), None);
        assert_eq!(next_watcher_retry(u32::MAX), None);
    }
    #[test]
    fn asynchronous_explorer_result_restores_expanded_children() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-explorer-refresh-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        let expanded = root.join("docs");
        fs::create_dir_all(&expanded).unwrap();
        fs::write(expanded.join("guide.md"), "# Guide").unwrap();

        let (tx, rx) = mpsc::channel();
        tx.send(ExplorerScanResult {
            root: root.clone(),
            sort_order: SortOrder::NameAsc,
            tree: FileExplorer::scan_directory_shallow(&root, SortOrder::NameAsc),
        })
        .unwrap();
        let mut explorer = FileExplorer {
            root: Some(root.clone()),
            expanded_dirs: HashSet::from([expanded.clone()]),
            pending_scan: Some(rx),
            ..Default::default()
        };

        assert!(explorer.poll_pending_scan());
        assert!(explorer
            .get_children(&expanded)
            .is_some_and(|children| children.iter().any(|node| node.name() == "guide.md")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_root_scan_runs_asynchronously_and_restores_expansion() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-local-explorer-scan-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        let expanded = root.join("docs");
        fs::create_dir_all(&expanded).unwrap();
        fs::write(expanded.join("guide.md"), "# Guide").unwrap();

        let mut explorer = FileExplorer {
            expanded_dirs: HashSet::from([expanded.clone()]),
            ..Default::default()
        };
        explorer.set_root(root.clone());

        let deadline = Instant::now() + Duration::from_secs(2);
        while explorer.pending_scan.is_some() && Instant::now() < deadline {
            explorer.poll_pending_scan();
            std::thread::sleep(Duration::from_millis(1));
        }

        assert!(explorer.pending_scan.is_none());
        assert!(explorer
            .get_children(&expanded)
            .is_some_and(|children| children.iter().any(|node| node.name() == "guide.md")));

        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn targeted_explorer_refresh_preserves_unrelated_subtrees() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-targeted-refresh-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        let docs = root.join("docs");
        let notes = root.join("notes");
        fs::create_dir_all(&docs).unwrap();
        fs::create_dir_all(&notes).unwrap();
        fs::write(docs.join("old.md"), "# Old").unwrap();
        fs::write(notes.join("keep.md"), "# Keep").unwrap();

        let mut explorer = FileExplorer::default();
        explorer.set_root(root.clone());
        // set_root scans asynchronously since #84: drain the pending scan
        // before interacting with the tree.
        let deadline = Instant::now() + Duration::from_secs(2);
        while explorer.pending_scan.is_some() && Instant::now() < deadline {
            explorer.poll_pending_scan();
            std::thread::sleep(Duration::from_millis(1));
        }
        explorer.toggle_expanded(&docs);
        explorer.toggle_expanded(&notes);
        fs::write(docs.join("new.md"), "# New").unwrap();

        explorer.refresh_directory(&docs);

        assert!(explorer
            .get_children(&docs)
            .is_some_and(|children| children.iter().any(|node| node.name() == "new.md")));
        assert!(explorer
            .get_children(&notes)
            .is_some_and(|children| children.iter().any(|node| node.name() == "keep.md")));
        assert!(explorer.is_expanded(&docs));
        assert!(explorer.is_expanded(&notes));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persisted_sidebar_widths_are_sanitized() {
        assert_eq!(restored_sidebar_width(Some(320.0), 200.0), 320.0);
        assert_eq!(restored_sidebar_width(Some(20.0), 200.0), SIDEBAR_MIN_WIDTH);
        assert_eq!(restored_sidebar_width(Some(f32::NAN), 200.0), 200.0);
        assert_eq!(restored_sidebar_width(Some(f32::INFINITY), 200.0), 200.0);
        assert_eq!(restored_sidebar_width(None, 200.0), 200.0);
    }
}
