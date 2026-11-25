#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::{HashMap, HashSet, VecDeque};
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
use egui::Widget;
use egui_commonmark_extended::{header_position_key, CommonMarkCache, CommonMarkViewer};
use notify::{PollWatcher, RecommendedWatcher};
use notify_debouncer_mini::{new_debouncer, new_debouncer_opt, DebouncedEventKind, Debouncer};
use regex::Regex;
use serde::{Deserialize, Serialize};

mod system_fonts;
use system_fonts::{
    base_covers, install_fonts, scan_pickable_font_families, setup_fonts, FontLoader, FontPreset,
    FontScope, TextSizeClass,
};

#[cfg(feature = "mcp")]
use egui_mcp_bridge::{McpBridge, McpUiExt};

const APP_KEY: &str = "md-viewer-state";

// Welcome page recent-files: how many to keep, and how many to show before "Show more".
const RECENT_FILES_CAP: usize = 20;
const RECENT_FOLDERS_CAP: usize = 20;
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
const SIDEBAR_RESIZE_GRAB_RADIUS: f32 = 3.0;
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

/// Points scrolled per mouse-wheel line: 2x egui's native default, the
/// per-click distance md-viewer has always had (see `MarkdownApp::new`).
const WHEEL_LINE_SCROLL_SPEED: f32 = 80.0;

/// Button theme that sizes button text with `TextStyle::Button`.
///
/// egui 0.37's default button theme sets button text from `TextStyle::Body`.
/// md-viewer sets `Body` to the document size (16 px), so every button,
/// menu entry, tab and outline row grew from 13 px to 16 px with the
/// upgrade. This wraps the default theme and only swaps the font back to
/// `Button`; everything else (frames, colors, states) stays egui's
/// (devlog 089).
struct ChromeButtonStyle(egui::theme::DefaultStyle);

impl egui::theme::StyleProvider<egui::widget_style::ButtonStyle> for ChromeButtonStyle {
    fn style(
        &mut self,
        args: &egui::widget_style::StyleArgs<'_>,
    ) -> egui::widget_style::ButtonStyle {
        let mut style: egui::widget_style::ButtonStyle =
            egui::theme::StyleProvider::style(&mut self.0, args);
        style.atom_layout.text_style.font_id = egui::TextStyle::Button.resolve(args.style);
        style
    }
}

fn content_width_limit(full_width_content: bool, available_width: f32) -> usize {
    let available_width = available_width.max(0.0) as usize;
    content_default_width(full_width_content)
        .map_or(available_width, |preferred| preferred.min(available_width))
}

/// Draw a visible affordance for a resizable sidebar's drag strip.
///
/// egui's `SidePanel` resize interaction paints nothing — the only hint is a
/// cursor change on hover — and the strip (`±SIDEBAR_RESIZE_GRAB_RADIUS`
/// around the panel border) sits over the content pane's edge, where users
/// reach for table column separators and scrollbars. Pressing there grabs the
/// sidebar resizer instead of the content, and the reflow reads as "the table
/// slid behind the sidebar". A line that brightens on hover and while
/// resizing makes the grab zone discoverable.
fn paint_sidebar_resize_affordance(
    ctx: &egui::Context,
    boundary_x: f32,
    y_range: egui::Rangef,
    panel_id: egui::Id,
) {
    // egui reads this same response at the top of `SidePanel::show`, so one
    // frame of latency is already inherent to hover feedback there.
    let Some(resize_response) = ctx.read_response(panel_id.with("__resize")) else {
        return;
    };
    let resizing = resize_response.dragged();
    let hover = resize_response.hovered();
    if !hover && !resizing {
        return;
    }
    let visuals = &ctx.global_style().visuals;
    let stroke = if resizing {
        visuals.widgets.active.bg_stroke
    } else {
        visuals.widgets.hovered.bg_stroke
    };
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique(panel_id).with("resize_affordance"),
    ));
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(boundary_x - 1.0, y_range.min),
            egui::pos2(boundary_x + 1.0, y_range.max),
        ),
        0.0,
        stroke.color,
    );
}

/// Height of one Explorer row: the tallest thing a row holds (a folder
/// name in the Body style, a button in the Button style, or egui's minimum
/// interact height). Every row is drawn exactly this tall, which is what
/// lets `show_rows` place rows by index (devlog 120).
fn explorer_row_height(ui: &egui::Ui) -> f32 {
    let spacing = &ui.style().spacing;
    let body = ui.text_style_height(&egui::TextStyle::Body);
    let button = ui.text_style_height(&egui::TextStyle::Button) + 2.0 * spacing.button_padding.y;
    spacing.interact_size.y.max(body).max(button)
}

/// Draw a small vector "document" icon (page with folded corner) for file
/// rows in the explorer. Pure painter drawing — no font or emoji involved.
fn paint_file_icon(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let stroke = egui::Stroke::new(1.2_f32, ui.visuals().weak_text_color());
    let body = egui::Rect::from_min_max(
        rect.left_top() + egui::vec2(2.0, 1.0),
        rect.right_bottom() - egui::vec2(2.0, 1.0),
    );
    let fold = body.width() * 0.3;
    // Page outline: top edge stops at the fold, then a diagonal to the right.
    painter.line_segment(
        [
            egui::pos2(body.right_top().x - fold, body.top()),
            body.left_top(),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(body.right_top().x - fold, body.top()),
            body.right_top() + egui::vec2(0.0, fold),
        ],
        stroke,
    );
    painter.line_segment([body.left_top(), body.left_bottom()], stroke);
    painter.line_segment([body.left_bottom(), body.right_bottom()], stroke);
    painter.line_segment(
        [
            body.right_bottom(),
            body.right_top() + egui::vec2(0.0, fold),
        ],
        stroke,
    );
}

/// Draw a small vector folder icon for directory rows in the explorer.
/// An expanded folder is drawn slightly open (raised tab, lighter fill).
fn paint_folder_icon(ui: &mut egui::Ui, size: f32, expanded: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    let stroke = egui::Stroke::new(1.2_f32, visuals.weak_text_color());
    let fill = if expanded {
        visuals.extreme_bg_color
    } else {
        visuals.faint_bg_color
    };
    let tab_w = rect.width() * 0.5;
    let tab_h = rect.height() * 0.25;
    let body_top = rect.top() + tab_h;
    // Tab (only spans the left half, like a real folder).
    painter.rect_filled(
        egui::Rect::from_min_max(
            rect.left_top() + egui::vec2(1.0, 1.0),
            egui::pos2(rect.left() + tab_w, body_top + 1.0),
        ),
        2.0,
        fill,
    );
    painter.rect_stroke(
        egui::Rect::from_min_max(
            rect.left_top() + egui::vec2(1.0, 1.0),
            egui::pos2(rect.left() + tab_w, body_top + 1.0),
        ),
        2.0,
        stroke,
        egui::StrokeKind::Middle,
    );
    // Body.
    let body = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 1.0, body_top),
        rect.right_bottom() - egui::vec2(1.0, 1.0),
    );
    painter.rect_filled(body, 2.0, fill);
    painter.rect_stroke(body, 2.0, stroke, egui::StrokeKind::Middle);
}

/// Check whether the desktop portal exposes the interface used by rfd's
/// Linux folder picker. Calling rfd without this interface fails like a user
/// cancellation, so checking first lets us distinguish that case and use a
/// local fallback without opening a second dialog after a real cancellation.
///
/// The probe runs in-process over the session bus. It used to shell out to
/// `gdbus`, which the snap and its core22 base do not ship, so the probe
/// always failed there and fell through to a Tkinter picker that strict
/// confinement cannot run either (#206).
#[cfg(target_os = "linux")]
fn portal_file_chooser_available() -> bool {
    match portal_file_chooser_version() {
        Ok(version) => {
            log::debug!("XDG Desktop Portal FileChooser available (version {version})");
            true
        }
        Err(error) => {
            log::info!("XDG Desktop Portal FileChooser probe failed: {error}");
            false
        }
    }
}

/// Read `org.freedesktop.portal.FileChooser`'s `version` property. Succeeds
/// only if the portal service exists *and* implements the FileChooser
/// interface; D-Bus activation starts the portal if it is not yet running.
#[cfg(target_os = "linux")]
fn portal_file_chooser_version() -> zbus::Result<u32> {
    let connection = zbus::blocking::connection::Builder::session()?
        .method_timeout(std::time::Duration::from_secs(5))
        .build()?;
    let reply = connection.call_method(
        Some("org.freedesktop.portal.Desktop"),
        "/org/freedesktop/portal/desktop",
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &("org.freedesktop.portal.FileChooser", "version"),
    )?;
    let value: zbus::zvariant::OwnedValue = reply.body().deserialize()?;
    u32::try_from(value).map_err(zbus::Error::from)
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

/// Display-zoom bounds and step, shared by keyboard, menu, and Ctrl+wheel
/// zooming. Steps are multiplicative so the perceived change stays uniform
/// across the whole range (like browsers and the lightbox already do).
const ZOOM_MIN: f32 = 0.5;
const ZOOM_MAX: f32 = 3.0;
const ZOOM_STEP: f32 = 1.25;

/// One multiplicative zoom step applied to `current`, clamped to the app's
/// zoom bounds. Neutral, non-finite, or non-positive factors are no-ops —
/// egui reports `zoom_delta() == 1.0` on frames without a zoom gesture.
fn zoomed(current: f32, factor: f32) -> f32 {
    if !factor.is_finite() || factor <= 0.0 || (factor - 1.0).abs() < 1e-3 {
        return current.clamp(ZOOM_MIN, ZOOM_MAX);
    }
    (current * factor).clamp(ZOOM_MIN, ZOOM_MAX)
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

fn push_recent_folder(folders: &mut Vec<PathBuf>, path: &Path) {
    folders.retain(|folder| folder != path);
    folders.insert(0, path.to_path_buf());
    folders.truncate(RECENT_FOLDERS_CAP);
}

fn initial_explorer_root(
    closed: bool,
    cli_file: Option<&Path>,
    saved_root: Option<PathBuf>,
    first_tab: Option<&Path>,
) -> Option<PathBuf> {
    if closed {
        return None;
    }
    cli_file
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .or_else(|| saved_root.filter(|path| path.exists()))
        .or_else(|| first_tab.and_then(Path::parent).map(Path::to_path_buf))
        .or_else(|| std::env::current_dir().ok())
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

/// The tab bar shows when the user has not hidden it (#208), and always while
/// the active tab is in Edit mode: the bar carries the Preview|Edit toggle,
/// the unsaved-changes marker and the save confirmation.
fn tab_bar_visible(show_tab_bar: bool, active_tab_editing: bool) -> bool {
    show_tab_bar || active_tab_editing
}

/// Persisted state saved between sessions
#[derive(Serialize, Deserialize, Default)]
struct PersistedState {
    dark_mode: Option<bool>,
    zoom_level: Option<f32>,
    show_outline: Option<bool>,
    /// Tab bar visibility (#208). `None` (older state files) shows it.
    show_tab_bar: Option<bool>,
    full_width_content: Option<bool>,
    math_scale: Option<f32>,
    /// Selection/highlight background color override, stored as premultiplied
    /// RGBA bytes (`Color32`'s canonical representation — see `Color32::r/g/b/a`
    /// and `Color32::from_rgba_premultiplied`). `None` = current theme default.
    highlight_color: Option<[u8; 4]>,
    /// Hyperlink text color override, same premultiplied-RGBA encoding as
    /// `highlight_color`. `None` = current theme default.
    link_color: Option<[u8; 4]>,
    /// Preferred document body/heading font family name. `None` means use the
    /// auto-detected system sans-serif (see `system_fonts::setup_fonts`).
    selected_font_family: Option<String>,
    /// Disable egui's pixel-snapping for glyph positions (see
    /// `TessellationOptions::round_text_to_pixels`). Off by default, matching
    /// today's behavior; `Some(true)` trades a touch of sharpness for glyphs
    /// that no longer look unevenly spaced at fractional zoom levels.
    smooth_text_rendering: Option<bool>,
    /// Remote image policy (#289); `None` (older state) loads them.
    remote_images: Option<RemoteImages>,
    /// Which markdown viewer's font choices lead the font chains.
    font_preset: Option<FontPreset>,
    /// Shared text-size class; `None` keeps the Normal default.
    text_size_class: Option<TextSizeClass>,
    open_tabs: Option<Vec<PathBuf>>,
    active_tab: Option<usize>,
    // File explorer state
    show_explorer: Option<bool>,
    explorer_root: Option<PathBuf>,
    /// Missing in older sessions, which may infer a root from an open file.
    explorer_closed: Option<bool>,
    expanded_dirs: Option<Vec<PathBuf>>,
    explorer_sort_order: Option<SortOrder>,
    explorer_width: Option<f32>,
    outline_width: Option<f32>,
    recent_files: Option<Vec<RecentEntry>>,
    recent_folders: Option<Vec<PathBuf>>,
}

fn restored_sidebar_width(width: Option<f32>, default: f32) -> f32 {
    width
        .filter(|width| width.is_finite())
        .map(|width| width.max(SIDEBAR_MIN_WIDTH))
        .unwrap_or(default)
}

/// A pending scroll to a heading that has not been measured yet.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PendingHeaderJump {
    /// The heading's source byte offset, which also keys its recorded
    /// position (`header_position_key`).
    source_start: usize,
    /// For the line-ratio fallback if the heading never records a position.
    line_number: usize,
}

impl PendingHeaderJump {
    fn new(header: &Header) -> Self {
        Self {
            source_start: header.source_start,
            line_number: header.line_number,
        }
    }
}

/// What the post-render step does with a pending heading jump.
#[derive(Debug, PartialEq)]
enum HeaderJumpStep {
    /// The heading's position is recorded: scroll there.
    ScrollTo(f32),
    /// The measured part has not reached the heading yet: keep measuring.
    Wait,
    /// The document is measured past the heading but no position was
    /// recorded: fall back to the line-ratio estimate.
    Estimate,
}

/// Decide a pending heading jump after a frame. `measured_source_end` is
/// `None` once the whole document is measured.
fn resolve_header_jump(
    position: Option<f32>,
    measured_source_end: Option<usize>,
    source_start: usize,
) -> HeaderJumpStep {
    match position {
        Some(y) => HeaderJumpStep::ScrollTo((y - 50.0).max(0.0)),
        None if measured_source_end.is_some_and(|end| end <= source_start) => HeaderJumpStep::Wait,
        None => HeaderJumpStep::Estimate,
    }
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

/// What md-viewer does with images that would be fetched over the network
/// (#289). Local and `data:` images always load.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
enum RemoteImages {
    /// Load them as the document is shown, as before #289.
    #[default]
    Load,
    /// Show placeholders, and a bar per document offering to load them.
    Ask,
    /// Show placeholders only.
    Block,
}

impl RemoteImages {
    const ALL: [RemoteImages; 3] = [Self::Load, Self::Ask, Self::Block];

    fn label(self) -> &'static str {
        match self {
            Self::Load => "Load automatically",
            Self::Ask => "Ask per document",
            Self::Block => "Block",
        }
    }

    /// Whether the renderer may fetch remote images for a tab whose
    /// "Load images" was clicked (`allowed_for_tab`).
    fn allows(self, allowed_for_tab: bool) -> bool {
        match self {
            Self::Load => true,
            Self::Ask => allowed_for_tab,
            Self::Block => false,
        }
    }
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
    bulk_expansion: Option<(HashSet<PathBuf>, bool)>,
}

struct PendingExplorerScan {
    receiver: Receiver<ExplorerScanResult>,
    expands_all: bool,
    discarded: bool,
}

/// File explorer state
#[derive(Default)]
struct FileExplorer {
    root: Option<PathBuf>,
    tree: Vec<FileTreeNode>,
    expanded_dirs: HashSet<PathBuf>,
    sort_order: SortOrder,
    /// Receiver for asynchronous root-directory scan results.
    pending_scan: Option<PendingExplorerScan>,
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
        if !self.start_root_scan(path.clone(), false) {
            self.tree = Self::scan_directory_shallow(&path, self.sort_order);
            Self::restore_expanded_children(&mut self.tree, &self.expanded_dirs, self.sort_order);
        }
    }

    fn close_root(&mut self) {
        self.root = None;
        self.tree.clear();
        self.expanded_dirs.clear();
        self.pending_scan = None;
    }

    fn start_root_scan(&mut self, path: PathBuf, expands_all: bool) -> bool {
        let sort_order = self.sort_order;
        let expanded_dirs = (!expands_all).then(|| self.expanded_dirs.clone());
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("explorer-scan".to_owned())
            .spawn(move || {
                let mut tree = Self::scan_directory_shallow(&path, sort_order);
                let bulk_expansion = if let Some(expanded_dirs) = expanded_dirs {
                    Self::restore_expanded_children(&mut tree, &expanded_dirs, sort_order);
                    None
                } else {
                    let mut expanded = HashSet::new();
                    let truncated = Self::expand_children(&mut tree, sort_order, &mut expanded);
                    Some((expanded, truncated))
                };
                let _ = tx.send(ExplorerScanResult {
                    root: path,
                    sort_order,
                    tree,
                    bulk_expansion,
                });
            });
        self.pending_scan = spawned.is_ok().then_some(PendingExplorerScan {
            receiver: rx,
            expands_all,
            discarded: false,
        });
        self.pending_scan.is_some()
    }

    /// Apply a completed scan. Return whether the expansion budget was hit.
    fn poll_pending_scan(&mut self) -> Option<bool> {
        let pending = self.pending_scan.as_ref()?;
        let discarded = pending.discarded;
        let received = pending.receiver.try_recv();
        let mut result = match received {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                self.pending_scan = None;
                return None;
            }
        };
        self.pending_scan = None;
        if discarded || self.root.as_deref() != Some(result.root.as_path()) {
            return None;
        }
        if result.sort_order != self.sort_order {
            Self::resort_tree_recursive(&mut result.tree, self.sort_order);
        }
        let truncated = if let Some((expanded, truncated)) = result.bulk_expansion {
            self.expanded_dirs = expanded;
            truncated
        } else {
            // Reuse the snapshot and load any directories expanded since it started.
            Self::restore_expanded_children(&mut result.tree, &self.expanded_dirs, self.sort_order);
            false
        };
        self.tree = result.tree;
        Some(truncated)
    }

    /// Refresh the file tree in the background.
    fn refresh(&mut self) {
        // A pending Expand All rescans the same root and replaces the tree
        // when it lands; a plain scan started now would drop it.
        if self
            .pending_scan
            .as_ref()
            .is_some_and(|scan| scan.expands_all && !scan.discarded)
        {
            return;
        }
        if let Some(root) = &self.root.clone() {
            if self.start_root_scan(root.clone(), false) {
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
        self.discard_pending_expansion();
        if self.expanded_dirs.contains(path) {
            self.expanded_dirs.remove(path);
        } else {
            // Load children before expanding if not yet loaded
            self.load_children(path);
            self.expanded_dirs.insert(path.clone());
        }
    }

    /// The rows the Explorer lists, in display order, with their depth:
    /// every node, descending into expanded directories whose children are
    /// loaded. Expanded directories whose children are not loaded yet go to
    /// `unloaded`, so they can be loaded once the frame is drawn.
    fn visible_rows<'a>(
        nodes: &'a [FileTreeNode],
        expanded: &HashSet<PathBuf>,
        depth: usize,
        rows: &mut Vec<(usize, &'a FileTreeNode)>,
        unloaded: &mut Vec<PathBuf>,
    ) {
        for node in nodes {
            rows.push((depth, node));
            if let FileTreeNode::Directory { path, children, .. } = node {
                if expanded.contains(path) {
                    match children {
                        Some(children) => {
                            Self::visible_rows(children, expanded, depth + 1, rows, unloaded)
                        }
                        None => unloaded.push(path.clone()),
                    }
                }
            }
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

    /// Get children for a directory by path (looks up in original tree, not clone).
    /// Only tests look folders up by path; the panel walks the tree once per
    /// frame (`visible_rows`).
    #[cfg(test)]
    fn get_children(&self, target_path: &PathBuf) -> Option<&Vec<FileTreeNode>> {
        Self::find_children_in_tree(&self.tree, target_path)
    }

    /// Recursively find children for a directory in the tree
    #[cfg(test)]
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

    // Limit bulk operations by both depth and directory count. Even a
    // shallow tree can otherwise generate thousands of reads and watches.
    const MAX_EXPAND_DEPTH: usize = 10;
    const BULK_EXPANSION_LIMIT: usize = 256;
    /// Expand All also stops before the Explorer would list more rows than
    /// this. When the Explorer laid out every row each frame, 6,400 rows took
    /// 72 % of a core in an optimized build while the pointer moved over it,
    /// against 22 % for the 1,024 rows of the unexpanded tree. It now draws
    /// only the visible rows (devlog 120), so this limit could be raised.
    const BULK_ROW_LIMIT: usize = 2048;

    /// Restore a saved expansion set, returning whether it was truncated.
    ///
    /// The saved set can hold folders from roots opened earlier (`set_root`
    /// keeps them). When it has to be cut, folders under `root` go first, so
    /// earlier roots cannot crowd out the one being shown.
    fn restore_expansion(&mut self, mut paths: Vec<PathBuf>, root: Option<&Path>) -> bool {
        if paths.len() > Self::BULK_EXPANSION_LIMIT {
            let outside = |path: &PathBuf| root.is_some_and(|root| !path.starts_with(root));
            // Then parents before descendants, with ties broken by path, so
            // the result is independent of the saved HashSet iteration order.
            paths.sort_unstable_by(|a, b| {
                outside(a)
                    .cmp(&outside(b))
                    .then_with(|| a.components().count().cmp(&b.components().count()))
                    .then_with(|| a.cmp(b))
            });
            paths.dedup();
        }
        let truncated = paths.len() > Self::BULK_EXPANSION_LIMIT;
        paths.truncate(Self::BULK_EXPANSION_LIMIT);
        self.expanded_dirs = paths.into_iter().collect();
        truncated
    }

    /// Start a bounded expansion on the existing background scan path.
    fn expand_all(&mut self) -> bool {
        self.root
            .clone()
            .is_some_and(|root| self.start_root_scan(root, true))
    }

    fn discard_pending_expansion(&mut self) {
        if let Some(scan) = &mut self.pending_scan {
            scan.discarded = scan.expands_all;
        }
    }

    /// Expand directories level by level until the folder or row budget is
    /// spent, so a deep first subdirectory cannot take every slot from its
    /// siblings. Returns whether a budget cut the expansion short.
    fn expand_children(
        nodes: &mut [FileTreeNode],
        sort_order: SortOrder,
        expanded: &mut HashSet<PathBuf>,
    ) -> bool {
        // Breadth-first over paths, then graft the scanned levels into the
        // tree; a queue of `&mut` nodes would need the whole tree borrowed.
        let mut queue: VecDeque<(PathBuf, usize)> = nodes
            .iter()
            .filter_map(|node| match node {
                FileTreeNode::Directory { path, .. } => Some((path.clone(), 0)),
                FileTreeNode::File { .. } => None,
            })
            .collect();
        let mut scanned: HashMap<PathBuf, Vec<FileTreeNode>> = HashMap::new();
        let mut truncated = false;
        // The top level is listed whether or not anything is expanded.
        let mut rows = nodes.len();
        while let Some((path, depth)) = queue.pop_front() {
            if expanded.len() == Self::BULK_EXPANSION_LIMIT {
                truncated = true;
                break;
            }
            let children = Self::scan_directory_shallow(&path, sort_order);
            if rows + children.len() > Self::BULK_ROW_LIMIT {
                truncated = true;
                break;
            }
            rows += children.len();
            if depth + 1 < Self::MAX_EXPAND_DEPTH {
                queue.extend(children.iter().filter_map(|node| match node {
                    FileTreeNode::Directory { path, .. } => Some((path.clone(), depth + 1)),
                    FileTreeNode::File { .. } => None,
                }));
            }
            expanded.insert(path.clone());
            scanned.insert(path, children);
        }
        Self::graft_children(nodes, &mut scanned);
        truncated
    }

    /// Attach scanned child lists to their directories, depth first.
    fn graft_children(
        nodes: &mut [FileTreeNode],
        scanned: &mut HashMap<PathBuf, Vec<FileTreeNode>>,
    ) {
        for node in nodes {
            if let FileTreeNode::Directory { path, children, .. } = node {
                if let Some(loaded) = scanned.remove(path) {
                    let children = children.get_or_insert(loaded);
                    Self::graft_children(children, scanned);
                }
            }
        }
    }

    /// Collapse all directories in the tree
    fn collapse_all(&mut self) {
        self.discard_pending_expansion();
        self.expanded_dirs.clear();
    }

    /// Keep the root and a bounded set of its expanded local subdirectories,
    /// and say whether the budget cut the set. Open tabs are watched
    /// separately.
    fn watch_directories(&self) -> (Vec<PathBuf>, bool) {
        let Some(root) = self.root.as_ref().filter(|root| !is_gvfs_path(root)) else {
            return (Vec::new(), false);
        };
        // Drop folders that no longer exist before cutting, so they cannot
        // take slots from ones that do.
        let mut expanded: Vec<_> = self
            .expanded_dirs
            .iter()
            .filter(|path| {
                *path != root && path.starts_with(root) && !is_gvfs_path(path) && path.is_dir()
            })
            .collect();
        let cut = expanded.len() > Self::BULK_EXPANSION_LIMIT;
        if cut {
            expanded.sort_unstable();
            expanded.truncate(Self::BULK_EXPANSION_LIMIT);
        }
        let dirs = std::iter::once(root).chain(expanded).cloned().collect();
        (dirs, cut)
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
    /// A heading the outline click or a `#fragment` link wants to scroll to
    /// whose position is not recorded yet. The renderer keeps measuring
    /// toward it (`measure_to_source`), and the post-render step jumps once
    /// the position is there (`resolve_header_jump`, devlog 115).
    pending_header_jump: Option<PendingHeaderJump>,
    /// "Load images" was clicked for this tab while remote images are set to
    /// Ask (#289). Session only.
    remote_images_allowed: bool,
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
    /// When true, the content area shows a TextEdit instead of CommonMarkViewer.
    is_editing: bool,
    /// Snapshot of the on-disk content at last load/save. The edit buffer is
    /// dirty when `content != saved_content`.
    saved_content: String,
    /// "Saved ✓" / "Save failed" flash trigger for the edit toolbar
    /// (true = success, false = failure).
    save_flash: Option<(bool, Instant)>,
    /// Scroll ratio (0..=1) captured when entering edit mode, applied to the
    /// editor's ScrollArea on its first frame.
    pending_edit_scroll: Option<f32>,
    /// The editor offset computed from `pending_edit_scroll` once the editor's
    /// content height is known, applied through the ScrollArea's offset
    /// builder on the following frame (egui 0.37 keeps `State::offset`
    /// private; the old direct `State` store also took effect one frame later).
    pending_edit_offset: Option<f32>,
    /// The editor's focus and selection, kept through a scrollbar press (#258).
    edit_focus: EditorFocus,
    /// View-mode content height captured when entering edit mode; converts
    /// the edit scroll ratio back to an absolute offset when leaving edit.
    pre_edit_view_content_height: f32,
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
        let (parsed, local_links) = parse_document(&content, &path);
        let content_lines = content.lines().count();
        let base_uri = Self::compute_base_uri(&path);

        let mut cache = CommonMarkCache::default();
        for link in &local_links {
            cache.add_link_hook(link);
        }

        let saved_content = content.clone();
        Ok(Self {
            id: egui::Id::unique(&path),
            path,
            content,
            cache,
            document_title: parsed.document_title,
            outline_headers: parsed.outline_headers,
            collapsed_headers: HashSet::new(),
            scroll_offset: 0.0,
            pending_scroll_offset: None,
            pending_header_jump: None,
            remote_images_allowed: false,
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
            is_editing: false,
            saved_content,
            save_flash: None,
            pending_edit_scroll: None,
            pending_edit_offset: None,
            edit_focus: EditorFocus::default(),
            pre_edit_view_content_height: 0.0,
        })
    }

    /// Switch between rendered view and raw-edit mode, preserving the scroll
    /// ratio across the switch (DSH-better-sidebar-style mode toggle).
    fn set_edit_mode(&mut self, to_edit: bool) {
        if self.is_editing == to_edit {
            return;
        }
        // Scroll mapping across the mode switch: capture the current ratio
        // and restore it in the other surface (ratio * that surface's
        // content height).
        let ratio = self.scroll_offset / self.last_content_height.max(1.0);
        if to_edit {
            self.pending_edit_scroll = Some(ratio);
            self.pre_edit_view_content_height = self.last_content_height;
        } else {
            // Leaving edit: first-frame estimate for the view; the
            // renderer's own correctives refine it after.
            self.pending_scroll_offset = Some(ratio * self.pre_edit_view_content_height.max(1.0));
        }
        self.is_editing = to_edit;
        self.cache.clear_scrollable();
        self.content_version = self.content_version.wrapping_add(1);
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

    /// Atomic save: write a temp file next to the target, then rename over
    /// it (mirrors the dsh-better-sidebar editor's tmp+rename save path).
    /// On success `saved_content` is refreshed so the buffer is no longer
    /// dirty.
    fn save_to_disk(&mut self) -> io::Result<()> {
        let tmp = PathBuf::from(format!(
            "{}.mdv-tmp-{}",
            self.path.display(),
            std::process::id()
        ));
        fs::write(&tmp, &self.content)?;
        if let Err(error) = fs::rename(&tmp, &self.path) {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
        self.saved_content = self.content.clone();
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
        self.saved_content = self.content.clone();
        self.path = path;
        self.id = egui::Id::unique(&self.path);
        self.cache = CommonMarkCache::default();
        self.content_version = self.content_version.wrapping_add(1);
        if reset_scroll {
            self.scroll_offset = 0.0;
            self.pending_scroll_offset = None;
        }
        self.base_uri = Self::compute_base_uri(&self.path);

        let (parsed, local_links) = parse_document(&self.content, &self.path);
        self.document_title = parsed.document_title;
        self.outline_headers = parsed.outline_headers;
        self.collapsed_headers.clear();

        self.local_links = local_links;
        for link in &self.local_links {
            self.cache.add_link_hook(link);
        }

        // Stale byte ranges; caller rebuilds if search bar is open.
        self.search_matches.clear();
    }

    fn navigate_to_link(&mut self, link: &str) -> io::Result<bool> {
        // A same-document fragment scrolls this tab — no path resolution, no
        // history entry, the same semantics as clicking the heading in the
        // outline. Unmatched fragments do nothing rather than error: the
        // document opened fine, the anchor simply does not exist.
        if let Some(fragment) = link.strip_prefix('#') {
            return Ok(apply_fragment_navigation(self, fragment));
        }

        // Share resolution and error handling with the ctrl+click path. These
        // used to differ: #78 taught percent-decoding and `file://` handling
        // to `resolve_local_link_path`, but this function still did a raw
        // `join`, so a link with an encoded space opened in a new tab on
        // ctrl+click and silently did nothing on a plain click.
        let (destination, fragment) = split_link_fragment(link);
        let Some(target_path) = self.resolve_link(destination)? else {
            return Ok(false);
        };

        if target_path == self.path || !target_path.is_file() {
            return Ok(false);
        }
        let previous_path = self.path.clone();
        self.load_file(&target_path)?;
        // `guide.md#section` opens the document *and* lands on the heading —
        // the #141 decision that an anchor selects a place in the document,
        // not just the document. Applied after `load_file`, whose
        // `reset_scroll` would otherwise clear it.
        if let Some(fragment) = &fragment {
            apply_fragment_navigation(self, fragment);
        }
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

    fn resolve_link(&self, link: &str) -> io::Result<Option<PathBuf>> {
        if link.starts_with('#') {
            return Ok(None);
        }

        let Some(current_dir) = self.path.parent() else {
            return Ok(None);
        };
        // `?` rather than a discard: a destination that cannot be decoded is
        // reported through the same error bar as a missing file (#181), instead
        // of rendering as an internal link that does nothing when clicked.
        let Some(target_path) = resolve_local_link_path(link, current_dir)? else {
            return Ok(None);
        };
        target_path.canonicalize().map(Some)
    }
}

/// Split a clicked destination into its file part and `#fragment`, if the
/// fragment is non-empty. A trailing `#` with nothing after it is a link with
/// no anchor, not a navigation request.
fn split_link_fragment(link: &str) -> (&str, Option<&str>) {
    match link.split_once('#') {
        Some((path, fragment)) if !fragment.is_empty() => (path, Some(fragment)),
        _ => (link, None),
    }
}

/// GitHub-style anchor slug for a heading title: lowercased, whitespace runs
/// become hyphens, and anything that is not a letter, number, hyphen or
/// underscore is dropped. This is the convention most markdown sites use for
/// `#fragment` anchors, so `#settings--config` matches a `Settings & Config`
/// heading.
fn slugify_heading(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else if c.is_whitespace() {
                '-'
            } else {
                '\u{0}'
            }
        })
        .filter(|c| *c != '\u{0}')
        .collect()
}

/// Where a `#fragment` landed inside a document.
enum FragmentTarget<'a> {
    /// The document title (the first `h1`, which the outline excludes) — the
    /// very top of the document.
    DocumentTop,
    Header(&'a Header),
    Missing,
}

/// Resolve a `#fragment` against a document's headings using slug semantics:
/// exact slug match, first heading wins. The document title is matched before
/// the outline headers because it is the page's primary anchor.
fn find_fragment_target<'a>(
    document_title: Option<&str>,
    headers: &'a [Header],
    fragment: &str,
) -> FragmentTarget<'a> {
    let wanted = slugify_heading(fragment);
    if wanted.is_empty() {
        return FragmentTarget::Missing;
    }
    if let Some(title) = document_title {
        if slugify_heading(title) == wanted {
            return FragmentTarget::DocumentTop;
        }
    }
    headers
        .iter()
        .find(|header| slugify_heading(&header.title) == wanted)
        .map(FragmentTarget::Header)
        .unwrap_or(FragmentTarget::Missing)
}

/// Point a tab at the heading a `#fragment` names, mirroring the outline-click
/// flow: a cached heading position scrolls immediately, and a fresh document
/// (no measured height, nothing cached) starts at the top and lets the
/// post-render corrective pass snap to the recorded position once the
/// bootstrap has painted. Returns whether the fragment named a heading.
fn apply_fragment_navigation(tab: &mut Tab, fragment: &str) -> bool {
    match find_fragment_target(
        tab.document_title.as_deref(),
        &tab.outline_headers,
        fragment,
    ) {
        FragmentTarget::DocumentTop => {
            tab.pending_scroll_offset = Some(0.0);
            true
        }
        FragmentTarget::Header(header) => {
            let key = header_position_key(header.source_start);
            if let Some(y_pos) = tab.cache.get_header_position(&key) {
                tab.pending_scroll_offset = Some((y_pos - 50.0).max(0.0));
            } else {
                // Not measured yet: the renderer measures toward it and the
                // post-render step jumps once it is recorded (devlog 115).
                tab.pending_header_jump = Some(PendingHeaderJump::new(header));
            }
            true
        }
        FragmentTarget::Missing => false,
    }
}

/// Handle a renderer link hook without coupling path resolution to the UI.
///
/// Plain click navigates the current tab, while Ctrl/Cmd+click returns a path
/// and `#fragment` for the caller to open in a new tab. Both modes surface
/// path-resolution errors consistently.
fn handle_local_link_click(
    tab: &mut Tab,
    clicked_link: &str,
    open_in_new_tab: bool,
) -> (Option<(PathBuf, Option<String>)>, Option<String>) {
    let result = if open_in_new_tab {
        let (destination, fragment) = split_link_fragment(clicked_link);
        tab.resolve_link(destination)
            .map(|path| path.map(|p| (p, fragment.map(str::to_owned))))
    } else {
        tab.navigate_to_link(clicked_link)
            .map(|_| None::<(PathBuf, Option<String>)>)
    };

    match result {
        Ok(target) => (target, None),
        Err(error) => (
            None,
            Some(format!("Unable to open {clicked_link}: {error}")),
        ),
    }
}

/// Parse explicit links plus bare local Markdown file references, skipping code blocks.
/// The app uses [`parse_document`]; this single pass is the tests' reference.
#[cfg(test)]
fn parse_local_links(content: &str, document_path: &Path) -> Vec<String> {
    let mut links = LinkCollector::new(document_path);
    for event in pulldown_cmark::Parser::new_ext(content, pulldown_cmark::Options::all()) {
        links.event(&event);
    }
    links.finish()
}

/// The outline and the link hooks of a document from one parse. Loading a
/// document used to parse it once for each (devlog 110).
fn parse_document(content: &str, document_path: &Path) -> (ParsedHeaders, Vec<String>) {
    let mut headers = HeaderCollector::new(content);
    let mut links = LinkCollector::new(document_path);
    for (event, range) in
        pulldown_cmark::Parser::new_ext(content, pulldown_cmark::Options::all()).into_offset_iter()
    {
        headers.event(&event, &range);
        links.event(&event);
    }
    (headers.finish(), links.finish())
}

/// The per-event half of [`parse_local_links`].
struct LinkCollector<'p> {
    links: HashSet<String>,
    document_dir: &'p Path,
    in_code_block: bool,
}

impl<'p> LinkCollector<'p> {
    fn new(document_path: &'p Path) -> Self {
        Self {
            links: HashSet::new(),
            document_dir: document_path.parent().unwrap_or_else(|| Path::new(".")),
            in_code_block: false,
        }
    }

    // Let CommonMark decide which bytes are visible text. Event::Text excludes
    // fenced/indented code and link destinations, while Event::Code preserves
    // the viewer extension that makes an inline-code filename clickable.
    fn event(&mut self, event: &pulldown_cmark::Event<'_>) {
        match event {
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::CodeBlock(_)) => {
                self.in_code_block = true;
            }
            pulldown_cmark::Event::End(pulldown_cmark::TagEnd::CodeBlock) => {
                self.in_code_block = false;
            }
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { dest_url, .. }) => {
                let destination = dest_url.as_ref();
                if is_local_markdown_link(destination) || destination.starts_with('#') {
                    self.links.insert(destination.to_owned());
                }
            }
            pulldown_cmark::Event::Code(code) => {
                register_existing_markdown_path(code, self.document_dir, &mut self.links);
            }
            pulldown_cmark::Event::Text(text) if !self.in_code_block => {
                register_bare_markdown_paths(text, self.document_dir, &mut self.links);
            }
            _ => {}
        }
    }

    fn finish(self) -> Vec<String> {
        let mut links: Vec<_> = self.links.into_iter().collect();
        links.sort();
        links
    }
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
    // A bare mention stays existence-gated, and an undecodable one is simply not
    // registered: only an *explicit* Markdown link should be able to report a
    // broken target, which is the boundary #181 drew.
    if resolve_local_link_path(destination, document_dir)
        .ok()
        .flatten()
        .is_some_and(|path| path.is_file())
    {
        links.insert(destination.to_owned());
    }
}

/// One classification of a destination's shape, used by *both* the gate that
/// decides which destinations become clickable and the resolver that turns one
/// into a path.
///
/// They used to classify separately and disagreed on four shapes, which #188
/// pinned: a query string produced a path with `?v=2` in the file name while
/// the gate rejected it; an unencoded `#` produced an extensionless path; an
/// empty destination produced the directory itself; and a destination whose
/// percent escapes are not UTF-8 passed the gate but resolved to nothing, so
/// the link rendered as internal and a click did nothing at all — no
/// navigation and no error. Sharing one classification is what makes those
/// disagreements unrepresentable rather than merely fixed.
enum DestinationShape<'a> {
    /// A leading `#`: within the current document, no path involved.
    Fragment,
    /// Nothing this viewer opens from the filesystem — a non-`file` scheme, a
    /// `file://` URI with a non-local authority, a protocol-relative URL, or a
    /// destination with nothing left to name a file with.
    External,
    /// Names a local file. `raw` keeps the source spelling with fragment and
    /// query removed, because the extension test has to run on what the author
    /// wrote; `decoded` is `None` when the percent escapes are not UTF-8.
    Local {
        raw: &'a str,
        decoded: Option<std::borrow::Cow<'a, str>>,
    },
}

fn classify_destination(destination: &str) -> DestinationShape<'_> {
    if destination.starts_with('#') {
        return DestinationShape::Fragment;
    }
    // A protocol-relative URL carries no scheme, so the path branch below would
    // read `//host/path` as an absolute filesystem path — the reinterpretation
    // #141 point 8 forbids.
    if destination.starts_with("//") {
        return DestinationShape::External;
    }
    if let Some(scheme) = explicit_uri_scheme(destination) {
        if !scheme.eq_ignore_ascii_case("file") {
            return DestinationShape::External;
        }
        // `to_file_path` decodes the path and rejects a non-local authority.
        let Some(path) = url::Url::parse(destination)
            .ok()
            .and_then(|url| url.to_file_path().ok())
        else {
            return DestinationShape::External;
        };
        return match path.into_os_string().into_string() {
            Ok(decoded) => DestinationShape::Local {
                raw: destination,
                decoded: Some(std::borrow::Cow::Owned(decoded)),
            },
            Err(_) => DestinationShape::Local {
                raw: destination,
                decoded: None,
            },
        };
    }
    // Fragment first, then query: `guide.md?v=2#s` names `guide.md`. Stripping
    // the query is what stops `?v=2` becoming part of the file name.
    let without_fragment = destination.split('#').next().unwrap_or(destination);
    let raw = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    if raw.is_empty() {
        // Nothing left to name a file with. The resolver used to answer with the
        // containing directory here, which only stayed unreachable because
        // `Tab::resolve_link` guarded a leading `#` separately.
        return DestinationShape::External;
    }
    DestinationShape::Local {
        raw,
        decoded: percent_encoding::percent_decode_str(raw).decode_utf8().ok(),
    }
}

fn has_markdown_extension(path: &Path) -> bool {
    path.extension()
        .map(|ext| {
            let ext = ext.to_string_lossy().to_lowercase();
            ext == "md" || ext == "markdown" || ext == "txt"
        })
        .unwrap_or(false)
}

/// Check if a link destination points to a local markdown file.
///
/// The extension test runs on the source spelling, not the decoded path: an
/// author writing `bad%FF.md` plainly means a Markdown file, and it has to pass
/// this gate for a click to be able to *report* that it cannot be opened.
fn is_local_markdown_link(destination: &str) -> bool {
    // The extension test needs a literal `.` in the source spelling, so a
    // destination without one can never pass. This answers most of a
    // document's words without classifying them: `parse_local_links` asks
    // about every word of plain text, which took 93 ms on 5 MB of prose
    // (devlog 110).
    destination.contains('.') && classified_as_local_markdown(destination)
}

fn classified_as_local_markdown(destination: &str) -> bool {
    match classify_destination(destination) {
        DestinationShape::Local { raw, .. } => has_markdown_extension(Path::new(raw)),
        DestinationShape::Fragment | DestinationShape::External => false,
    }
}

/// Return an RFC 3986 URI scheme when the destination explicitly contains one.
///
/// Markdown link destinations are URIs, so a colon after a valid scheme prefix
/// is not a filesystem separator. On Windows, preserve drive-letter paths such
/// as `C:\docs\guide.md` instead of classifying `C` as a URI scheme.
fn explicit_uri_scheme(destination: &str) -> Option<&str> {
    let colon = destination.find(':')?;
    let scheme = &destination[..colon];
    let mut chars = scheme.chars();
    if !chars.next().is_some_and(|ch| ch.is_ascii_alphabetic())
        || !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
    {
        return None;
    }

    #[cfg(target_os = "windows")]
    if scheme.len() == 1 {
        return None;
    }

    Some(scheme)
}

/// Resolve a relative/absolute filesystem path or a `file:` URI against the
/// directory containing the Markdown document.
/// Turn a destination into a path to try.
///
/// `Ok(None)` means there is nothing to open — a fragment, or a destination
/// this viewer treats as external. `Err` means the destination names a local
/// file that cannot be turned into a path at all, which callers must surface
/// rather than swallow: before #141 that case returned `None` like the others,
/// so an internal-looking link did nothing whatsoever when clicked.
fn resolve_local_link_path(destination: &str, document_dir: &Path) -> io::Result<Option<PathBuf>> {
    match classify_destination(destination) {
        DestinationShape::Fragment | DestinationShape::External => Ok(None),
        DestinationShape::Local { raw, decoded: None } => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{raw} is not valid UTF-8 after percent-decoding"),
        )),
        DestinationShape::Local {
            decoded: Some(decoded),
            ..
        } => {
            let path = Path::new(decoded.as_ref());
            Ok(Some(if path.is_absolute() {
                path.to_path_buf()
            } else {
                document_dir.join(path)
            }))
        }
    }
}

/// Parse headings with the same CommonMark rules used by the renderer.
/// The outline alone. The app uses [`parse_document`]; this single pass is
/// the tests' reference.
#[cfg(test)]
fn parse_headers(content: &str) -> ParsedHeaders {
    let mut headers = HeaderCollector::new(content);
    for (event, range) in
        pulldown_cmark::Parser::new_ext(content, pulldown_cmark::Options::all()).into_offset_iter()
    {
        headers.event(&event, &range);
    }
    headers.finish()
}

/// The per-event half of [`parse_headers`].
struct HeaderCollector {
    line_starts: Vec<usize>,
    all_headers: Vec<Header>,
    current: Option<(u8, usize, usize, String)>,
}

impl HeaderCollector {
    fn new(content: &str) -> Self {
        Self {
            line_starts: std::iter::once(0)
                .chain(content.match_indices('\n').map(|(offset, _)| offset + 1))
                .collect(),
            all_headers: Vec::new(),
            current: None,
        }
    }

    fn event(&mut self, event: &pulldown_cmark::Event<'_>, range: &std::ops::Range<usize>) {
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

        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let line_number = self
                    .line_starts
                    .partition_point(|line_start| *line_start <= range.start)
                    .saturating_sub(1);
                self.current = Some((
                    level_number(*level),
                    range.start,
                    line_number,
                    String::new(),
                ));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, source_start, line_number, title)) = self.current.take() {
                    let title = title.trim().to_owned();
                    if !title.is_empty() {
                        self.all_headers.push(Header {
                            level,
                            title,
                            source_start,
                            line_number,
                        });
                    }
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, _, _, title)) = self.current.as_mut() {
                    title.push_str(text);
                }
            }
            Event::InlineMath(text) | Event::DisplayMath(text) => {
                if let Some((_, _, _, title)) = self.current.as_mut() {
                    title.push_str(text);
                }
            }
            Event::FootnoteReference(label) => {
                if let Some((_, _, _, title)) = self.current.as_mut() {
                    title.push('[');
                    title.push_str(label);
                    title.push(']');
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some((_, _, _, title)) = self.current.as_mut() {
                    title.push(' ');
                }
            }
            _ => {}
        }
    }

    fn finish(self) -> ParsedHeaders {
        let all_headers = self.all_headers;
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
        // No vsync override: eframe 0.37 itself never blocks the swap on
        // Wayland (egui#5136), which is what devlog 073 fixed here for 0.33.
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

/// The Fonts dialog's family list. The scan behind it probes every installed
/// family's Latin coverage (`scan_pickable_font_families`, about 0.5 s of
/// CPU), so it starts only when the dialog first asks for it, at most once per
/// run (devlog 109).
#[derive(Default)]
struct FontFamilyList {
    /// One canonical name per body-capable family. Empty until the scan lands.
    names: Vec<String>,
    /// Lowercased `names`, computed once so the dialog's search filter does
    /// not re-lowercase every name on every frame.
    names_lower: Vec<String>,
    pending: Option<Receiver<Vec<String>>>,
    requested: bool,
}

impl FontFamilyList {
    /// Starts the scan unless one was already started in this run.
    fn request(&mut self, spawn: impl FnOnce() -> Option<Receiver<Vec<String>>>) {
        if self.requested {
            return;
        }
        self.requested = true;
        self.pending = spawn();
    }

    /// True while a started scan has not delivered yet.
    fn scanning(&self) -> bool {
        self.pending.is_some()
    }

    /// Collects a finished scan without blocking. Returns true when the list
    /// arrived this call.
    fn poll(&mut self) -> bool {
        let Some(rx) = &self.pending else {
            return false;
        };
        match rx.try_recv() {
            Ok(names) => {
                self.names_lower = names.iter().map(|name| name.to_ascii_lowercase()).collect();
                self.names = names;
                self.pending = None;
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                // The scan thread ended without sending (it panicked): stop
                // polling and keep "System Default" as the only choice.
                self.pending = None;
                false
            }
        }
    }
}

/// Runs `scan_pickable_font_families` on its own thread. `None` if the thread
/// could not be spawned; the dialog then offers only "System Default".
fn spawn_font_family_scan() -> Option<Receiver<Vec<String>>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("font-family-scan".to_owned())
        .spawn(move || {
            let _ = tx.send(scan_pickable_font_families());
        })
        .ok()
        .map(|_| rx)
}

struct MarkdownApp {
    tabs: Vec<Tab>,
    active_tab: usize,
    dark_mode: bool,
    zoom_level: f32,
    show_outline: bool,
    /// Whether the tab bar is shown (#208). The bar is still shown while the
    /// active tab is in Edit mode, because it holds the Preview|Edit toggle
    /// and the unsaved-changes marker.
    show_tab_bar: bool,
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
    // Document body/heading font. `None` = auto-detected system sans-serif.
    selected_font_family: Option<String>,
    // Body-font families the picker offers, scanned in a background thread
    // the first time the Fonts dialog opens (devlog 109).
    font_families: FontFamilyList,
    // Mirrors the font setup inputs (picked family, preset, size class): only
    // reload fonts when this tuple changes.
    last_applied_font_config: Option<(Option<String>, FontPreset, TextSizeClass)>,
    // Full font set (every script and emoji fallback) being built off the UI
    // thread after a startup that only needed the base set (devlog 100). The
    // config is the one it was built for; a result for an older config is
    // dropped.
    pending_full_fonts: Option<Receiver<(FontConfig, egui::FontDefinitions)>>,
    show_font_dialog: bool,
    // Transient UI-only search text for the font picker (not persisted).
    font_filter: String,
    // Disables egui's glyph pixel-snapping when true. Applied unconditionally
    // every frame (see ctx.set_zoom_factor below) since it's a single write
    // into egui's own memory, not worth a last-applied change-gate.
    smooth_text_rendering: bool,
    // Whether images with an http(s) URI are fetched (#289).
    remote_images: RemoteImages,
    // Which markdown viewer's font choices lead the font chains.
    font_preset: FontPreset,
    // Shared text-size class across all presets.
    text_size_class: TextSizeClass,
    watch_enabled: bool,
    error_message: Option<String>,
    is_dragging: bool,
    /// The document area (the `CentralPanel` rect) as of this frame. The
    /// lightbox backdrop and the drop overlay cover exactly this area; egui
    /// 0.33's `Context::available_rect()` returned it after the panels were
    /// laid out, and egui 0.37 removed that method.
    document_rect: egui::Rect,
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
    /// Whether the last explorer watch set was cut to the budget, so the
    /// warning is logged once when it starts rather than on every reconcile.
    explorer_watches_cut: bool,
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
    // Last zoom level handed to set_zoom_factor; skips redundant writes
    last_applied_zoom_level: Option<f32>,
    last_window_title: String,
    selftest_frames: usize,
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
    recent_folders: Vec<PathBuf>,
    // Welcome page: whether the recent list is expanded ("Show more")
    welcome_show_all: bool,
    // MCP bridge for E2E testing
    #[cfg(feature = "mcp")]
    mcp_bridge: McpBridge,
}

/// The inputs of a font setup: picked family, preset, size class.
type FontConfig = (Option<String>, FontPreset, TextSizeClass);

impl MarkdownApp {
    fn new(cc: &eframe::CreationContext<'_>, file: Option<PathBuf>, watch: bool) -> Self {
        // Load persisted state (needed before font setup, which reads the
        // persisted font family, preset, and text-size class).
        let persisted: PersistedState = cc
            .storage
            .and_then(|s| eframe::get_value(s, APP_KEY))
            .unwrap_or_default();
        let selected_font_family = persisted.selected_font_family;
        let font_preset = persisted.font_preset.unwrap_or_default();
        let text_size_class = persisted.text_size_class.unwrap_or_default();

        // Install the base font set: the user's picked family (font picker)
        // or the preset's emulated stack, with its bold and mono faces. The
        // script and emoji fallbacks are added at the end of `new`, either
        // right away when the startup text needs them or on a background
        // thread (devlog 100).
        let mut font_loader = FontLoader::new();
        let base_fonts = font_loader.definitions(
            selected_font_family.as_deref(),
            font_preset,
            FontScope::Base,
        );
        install_fonts(&cc.egui_ctx, base_fonts.clone(), text_size_class);

        // Clear stale egui widget data loaded from disk (scroll offsets, panel sizes, etc.)
        // We don't persist egui memory (see persist_egui_memory), but eframe always
        // loads it if present. This purges the old blob so it doesn't waste startup time/RAM.
        cc.egui_ctx.memory_mut(|mem| mem.data = Default::default());

        // Disable egui's built-in Ctrl+/- zoom — we handle zoom ourselves
        cc.egui_ctx
            .options_mut(|opt| opt.zoom_with_keyboard = false);
        // One mouse-wheel click scrolls twice egui's default (40 pt per line).
        // Up to 0.3.5 that speed came, by accident, from a scroll-during-
        // selection workaround that re-applied every wheel delta on top of
        // egui's own scrolling; egui 0.37 removed the API it used. Keeping the
        // speed users know (devlog 089). Trackpad pixel deltas stay 1x.
        cc.egui_ctx
            .options_mut(|opt| opt.input_options.line_scroll_speed = WHEEL_LINE_SCROLL_SPEED);

        // Set constant styles once at init (never changes at runtime)
        cc.egui_ctx.global_style_mut(|style| {
            // Constant floating-scrollbar geometry (#139).
            //
            // egui's floating bar animates its width between `floating_width`
            // (2) and `bar_width` (10) on hover. A dormant floating bar is
            // hidden by `dormant_*_opacity: 0.0`, not by being thin, so that
            // width governs the bar's *interact* rect and nothing visible.
            // Beside a panel divider the growing and shrinking rect claims and
            // releases the pointer on alternating frames, and at the one pixel
            // just outside the resize handle the two never settle.
            //
            // Measured on Xvfb with the pointer held still: on `main` exactly
            // one x out of 36 cycles through four states, in both the painted
            // pixels and the cursor shape; with the width pinned, zero — and
            // both builds produce an identical state map across x, so this
            // removes the oscillation rather than moving it out of view.
            // `scripts/hover-stability.sh` is that measurement.
            style.spacing.scroll.floating_width = style.spacing.scroll.bar_width;
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

            // egui 0.37 enlarged the default button padding (4,1 -> 8,2) and
            // widget height (18 -> 20). Keep 0.33's compact chrome
            // (devlog 089).
            style.spacing.button_padding = egui::vec2(4.0, 1.0);
            style.spacing.interact_size.y = 18.0;
        });
        cc.egui_ctx
            .replace_widget_theme(ChromeButtonStyle(egui::theme::DefaultStyle));

        let dark_mode = persisted
            .dark_mode
            .unwrap_or_else(|| cc.egui_ctx.global_style().visuals.dark_mode);
        let zoom_level = persisted.zoom_level.unwrap_or(1.0).clamp(0.5, 3.0);
        let show_outline = persisted.show_outline.unwrap_or(true);
        let show_tab_bar = persisted.show_tab_bar.unwrap_or(true);
        let full_width_content = persisted.full_width_content.unwrap_or(true);
        let math_scale = persisted.math_scale.unwrap_or(1.0).clamp(1.0, 2.0);
        let highlight_color = persisted
            .highlight_color
            .map(|[r, g, b, a]| egui::Color32::from_rgba_premultiplied(r, g, b, a));
        let link_color = persisted
            .link_color
            .map(|[r, g, b, a]| egui::Color32::from_rgba_premultiplied(r, g, b, a));
        let smooth_text_rendering = persisted.smooth_text_rendering.unwrap_or(false);
        let remote_images = persisted.remote_images.unwrap_or_default();
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

        let explorer_root = initial_explorer_root(
            persisted.explorer_closed.unwrap_or(false),
            file.as_deref(),
            persisted.explorer_root,
            tabs.first().map(|tab| tab.path.as_path()),
        );

        // Bound the expansion set before the background scan or watcher sees it.
        if let Some(root) = explorer_root {
            if let Some(expanded) = persisted.expanded_dirs {
                if file_explorer.restore_expansion(expanded, Some(&root)) {
                    let message = format!(
                        "Explorer restored only the first {} saved expanded directories.",
                        FileExplorer::BULK_EXPANSION_LIMIT
                    );
                    log::warn!("{message}");
                    startup_error.get_or_insert(message);
                }
            }
            file_explorer.set_root(root);
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
            show_tab_bar,
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
            // Already applied in `new` (base set, then the full set now or
            // from `pending_full_fonts`); last_applied starts in sync so
            // update() doesn't redundantly reload fonts on frame 1.
            last_applied_font_config: Some((
                selected_font_family.clone(),
                font_preset,
                text_size_class,
            )),
            selected_font_family,
            font_families: FontFamilyList::default(),
            pending_full_fonts: None,
            show_font_dialog: false,
            font_filter: String::new(),
            smooth_text_rendering,
            remote_images,
            font_preset,
            text_size_class,
            watch_enabled: watch,
            error_message: startup_error,
            is_dragging: false,
            document_rect: egui::Rect::NOTHING,
            watcher: None,
            watcher_rx: None,
            watcher_retry_count: 0,
            watched_paths: HashSet::new(),
            watched_explorer_dirs: HashSet::new(),
            explorer_watches_cut: false,
            hovered_tab: None,
            file_explorer,
            show_explorer,
            explorer_width,
            outline_width,
            flashing_paths: HashMap::new(),
            is_virtual_display,
            egui_ctx: cc.egui_ctx.clone(),
            last_applied_dark_mode: None,
            last_applied_zoom_level: None,
            last_window_title: String::new(),
            selftest_frames: 0,
            title_dirty: true,
            open_tab_paths: HashSet::new(),
            lightbox: None,
            lightbox_scroll: 0.0,
            lightbox_open_count: 0,
            search: SearchState::default(),
            recent_files: persisted.recent_files.unwrap_or_default(),
            recent_folders: persisted.recent_folders.unwrap_or_default(),
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

        app.finish_startup_fonts(&cc.egui_ctx, font_loader, &base_fonts);

        app
    }

    /// Text the first frames can draw: the active document, tab titles and
    /// paths, and the recent-files list. The explorer tree is scanned
    /// asynchronously and is not available yet; its names are covered once
    /// the full font set lands, a few hundred milliseconds in.
    fn startup_texts(&self) -> Vec<std::borrow::Cow<'_, str>> {
        let mut texts: Vec<std::borrow::Cow<'_, str>> = Vec::new();
        if let Some(tab) = self.tabs.get(self.active_tab) {
            texts.push(tab.content.as_str().into());
        }
        for tab in &self.tabs {
            texts.push(tab.path.to_string_lossy());
        }
        for entry in &self.recent_files {
            texts.push(entry.path.to_string_lossy());
        }
        texts
    }

    /// Complete the font setup `new` started with the base set.
    ///
    /// If the startup text has a character the base faces lack, install the
    /// full set now, so the first frame draws it (the behaviour before
    /// devlog 100). Otherwise build the full set on a background thread and
    /// let `update` install it.
    fn finish_startup_fonts(
        &mut self,
        ctx: &egui::Context,
        mut font_loader: FontLoader,
        base_fonts: &egui::FontDefinitions,
    ) {
        let config: FontConfig = (
            self.selected_font_family.clone(),
            self.font_preset,
            self.text_size_class,
        );
        let covered = {
            let texts = self.startup_texts();
            base_covers(base_fonts, texts.iter().map(|t| t.as_ref()))
        };
        if !covered {
            log::info!("Startup text needs fallback fonts; installing the full set now");
            let full = font_loader.definitions(config.0.as_deref(), config.1, FontScope::Full);
            install_fonts(ctx, full, config.2);
            return;
        }
        drop(font_loader);
        log::info!(
            "Startup text is covered by the base fonts; loading fallbacks in the background"
        );
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("font-fallbacks".to_owned())
            .spawn(move || {
                let full =
                    FontLoader::new().definitions(config.0.as_deref(), config.1, FontScope::Full);
                let _ = tx.send((config, full));
            })
            .is_ok();
        if spawned {
            self.pending_full_fonts = Some(rx);
        } else {
            // No thread: build it here, as before devlog 100.
            let full = FontLoader::new().definitions(
                self.selected_font_family.as_deref(),
                self.font_preset,
                FontScope::Full,
            );
            install_fonts(ctx, full, self.text_size_class);
        }
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
            Ok(Some(path)) => self.open_folder(path),
            Ok(None) => {}
            Err(error) => {
                log::error!("Unable to open folder picker: {error}");
                self.error_message = Some(format!("Unable to open folder picker: {error}"));
            }
        }
    }

    fn open_folder(&mut self, path: PathBuf) {
        if !path.is_dir() {
            self.error_message = Some(format!("Folder is unavailable: {}", path.display()));
            return;
        }
        push_recent_folder(&mut self.recent_folders, &path);
        self.file_explorer.set_root(path);
        self.show_explorer = true;
        if self.watch_enabled {
            self.start_watching();
        }
    }

    fn close_folder(&mut self) {
        self.file_explorer.close_root();
        if self.tabs.is_empty() {
            self.stop_watching();
        } else {
            self.reconcile_explorer_watches();
        }
    }

    fn open_in_new_tab(&mut self, path: PathBuf) {
        self.open_in_new_tab_with_fragment(path, None);
    }

    /// Open a document in its own tab, optionally landing on the heading a
    /// `#fragment` names. An already-open document is focused *and* scrolled
    /// to the fragment — the tab-switch must not silently drop the anchor.
    fn open_in_new_tab_with_fragment(&mut self, path: PathBuf, fragment: Option<String>) {
        let path = path.canonicalize().unwrap_or(path);
        // Check if already open
        if let Some(idx) = self.tabs.iter().position(|t| t.path == path) {
            self.active_tab = idx;
            self.title_dirty = true;
            if let Some(fragment) = &fragment {
                if let Some(tab) = self.tabs.get_mut(idx) {
                    apply_fragment_navigation(tab, fragment);
                }
            }
            return;
        }

        // Add new tab
        let mut tab = match Tab::new(path.clone()) {
            Ok(tab) => tab,
            Err(error) => {
                self.error_message = Some(format!("Unable to open {}: {error}", path.display()));
                return;
            }
        };
        if let Some(fragment) = &fragment {
            apply_fragment_navigation(&mut tab, fragment);
        }
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
        let (explorer_dirs, watch_cut) = self.file_explorer.watch_directories();
        self.note_explorer_watch_cut(watch_cut);
        let has_local = !local_paths.is_empty() || !explorer_dirs.is_empty();
        let has_gvfs = !gvfs_paths.is_empty();

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

    /// Log once when the explorer's watch set starts being cut to the budget,
    /// not on every reconcile while it stays cut.
    fn note_explorer_watch_cut(&mut self, cut: bool) {
        if cut && !self.explorer_watches_cut {
            log::warn!(
                "Explorer will auto-refresh only {} expanded directories. Use Refresh for the whole tree.",
                FileExplorer::BULK_EXPANSION_LIMIT
            );
        }
        self.explorer_watches_cut = cut;
    }

    /// Reconcile the non-recursive explorer-directory watches (root + expanded
    /// dirs) against the live watcher after the expanded set changes. Mirrors
    /// `update_watched_paths`'s incremental diff so expand/collapse doesn't tear
    /// down and rebuild the debouncer + bridge thread.
    fn reconcile_explorer_watches(&mut self) {
        if !self.watch_enabled {
            return;
        }

        let (dirs, watch_cut) = self.file_explorer.watch_directories();
        self.note_explorer_watch_cut(watch_cut);
        let desired: HashSet<_> = dirs.into_iter().collect();

        if let Some(fw) = &mut self.watcher {
            if let Some(w) = fw.inotify_watcher() {
                for path in self.watched_explorer_dirs.difference(&desired) {
                    let _ = w.unwatch(path);
                }
                for path in desired.difference(&self.watched_explorer_dirs) {
                    if let Err(e) = w.watch(path, notify::RecursiveMode::NonRecursive) {
                        log::error!("Failed to watch explorer dir {:?}: {}", path, e);
                    }
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
                    // Never clobber unsaved edit-mode edits with a disk
                    // reload (dirty-reload suppression, mirroring the
                    // dsh-better-sidebar editor). A reload of identical
                    // content (e.g. our own save echoing back through the
                    // watcher) still proceeds.
                    if tab.is_editing && tab.content != tab.saved_content {
                        log::info!("Skipping reload of dirty edit tab: {:?}", path);
                        continue;
                    }
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
    fn render_search_bar(&mut self, ui: &mut egui::Ui) -> SearchBarOutcome {
        let mut outcome = SearchBarOutcome::default();
        if !self.search.is_open {
            return outcome;
        }

        let input_id = egui::Id::unique("search_input");
        let total_matches = self
            .tabs
            .get(self.active_tab)
            .map(|t| t.search_matches.len())
            .unwrap_or(0);
        let active_idx = self.search.active_match_index;

        egui::Panel::top("search_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
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

            // Preview|Edit segmented toggle for the active tab, right-aligned
            // in the tab bar (DSH-better-sidebar editor-header placement).
            // Borderless on purpose: the tab bar strip already carries a
            // border, so only the filled active segment provides the frame.
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut switch_mode: Option<bool> = None; // Some(true) → edit
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(2))
                        .show(ui, |ui| {
                            ui.set_min_height(ui.text_style_height(&egui::TextStyle::Small));
                            ui.horizontal(|ui| {
                                // Added in Edit, Preview order so the
                                // right-to-left layout renders [Preview|Edit].
                                for (label, is_edit) in [("Edit", true), ("Preview", false)] {
                                    let active = tab.is_editing == is_edit;
                                    let text_color = if active {
                                        ui.visuals().text_color()
                                    } else {
                                        ui.visuals().weak_text_color()
                                    };
                                    let response = ui.add(
                                        egui::Button::new(
                                            egui::RichText::new(label).small().color(text_color),
                                        )
                                        .fill(if active {
                                            ui.visuals().widgets.inactive.weak_bg_fill
                                        } else {
                                            egui::Color32::TRANSPARENT
                                        })
                                        .corner_radius(egui::CornerRadius::same(4)),
                                    );
                                    if response.clicked() && !active {
                                        switch_mode = Some(is_edit);
                                    }
                                }
                            });
                        });
                    if let Some(to_edit) = switch_mode {
                        tab.set_edit_mode(to_edit);
                    }
                    if tab.content != tab.saved_content {
                        ui.label(
                            egui::RichText::new("●")
                                .small()
                                .color(ui.visuals().warn_fg_color),
                        )
                        .on_hover_text("Unsaved changes — press Ctrl+S to save");
                    }
                    if let Some((ok, at)) = tab.save_flash {
                        if (at.elapsed().as_millis() as u64) < FLASH_DURATION_MS * 3 {
                            let (label, color) = if ok {
                                ("Saved ✓", egui::Color32::from_rgb(90, 200, 120))
                            } else {
                                ("Save failed", ui.visuals().error_fg_color)
                            };
                            ui.label(egui::RichText::new(label).small().color(color));
                        } else {
                            tab.save_flash = None;
                        }
                    }
                });
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
    fn render_outline(&mut self, ui: &mut egui::Ui) {
        // egui 0.37: panels are shown in the root `Ui`, not on the `Context`.
        let ctx_owned = ui.ctx().clone();
        let ctx = &ctx_owned;
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

        let panel = egui::Panel::right("outline")
            .resizable(true)
            .default_size(self.outline_width)
            .size_range(SIDEBAR_MIN_WIDTH..=f32::INFINITY)
            .frame(
                egui::Frame::side_top_panel(&ctx.global_style()).inner_margin(egui::Margin {
                    left: 8,
                    right: 0,
                    top: 8,
                    bottom: 0,
                }),
            )
            .show(ui, |ui| {
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
        // Right panel: the draggable edge is its left border.
        paint_sidebar_resize_affordance(
            ctx,
            panel.response.rect.left(),
            panel.response.rect.y_range(),
            // Panel::id in egui 0.37: parent scope id combined with the salt.
            ui.scope_id().with_salt(egui::IdSalt::new("outline")),
        );

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
                // A recorded position scrolls at once. A heading the
                // progressive measurement has not reached yet is measured
                // toward over the next frames, and the post-render step jumps
                // when its position is recorded (devlog 115). Jumping to a
                // line-ratio estimate first would ask for an offset past the
                // measured part, which lays out the whole document in one
                // frame.
                if let Some(y_pos) = tab.cache.get_header_position(&key) {
                    tab.pending_scroll_offset = Some((y_pos - 50.0).max(0.0));
                } else {
                    tab.pending_header_jump = Some(PendingHeaderJump::new(header));
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
        let mut remove_recent = None;
        let mut clear_history = false;
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
                    ui.label(egui::RichText::new("Markdown").size(56.0).weak());
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
                        .button(egui::RichText::new("Open File…").size(15.0))
                        .clicked()
                    {
                        do_open_file = true;
                    }
                    ui.add_space(10.0);
                    if ui
                        .button(egui::RichText::new("Open Folder…").size(15.0))
                        .clicked()
                    {
                        do_open_folder = true;
                    }
                });

                if !recents.is_empty() {
                    ui.add_space(32.0);
                    // Center a fixed-width, left-aligned column for the recent list.
                    let avail = ui.available_width();
                    let col_w = (avail - 16.0).clamp(0.0, 560.0);
                    let margin = ((avail - col_w) / 2.0).max(0.0);
                    ui.horizontal(|ui| {
                        ui.add_space(margin);
                        ui.vertical(|ui| {
                            ui.set_width(col_w);
                            ui.horizontal_wrapped(|ui| {
                                ui.strong("Recent Files");
                                clear_history = ui.small_button("Clear History").clicked();
                            });
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
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .small_button("✕")
                                            .on_hover_text("Remove from history")
                                            .clicked()
                                        {
                                            remove_recent = Some(entry.path.clone());
                                        }
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                // Keep long names out of the remove button's hit area.
                                                ui.set_clip_rect(
                                                    ui.clip_rect().intersect(ui.max_rect()),
                                                );
                                                let label =
                                                    egui::RichText::new(name.as_str()).size(14.0);
                                                let label =
                                                    if *exists { label } else { label.weak() };
                                                let resp = ui
                                                    .add_enabled(
                                                        *exists,
                                                        egui::Button::new(label)
                                                            .frame(false)
                                                            .truncate(),
                                                    )
                                                    .on_hover_text(entry.path.to_string_lossy());
                                                if resp.clicked() {
                                                    open_path = Some(entry.path.clone());
                                                }
                                                if ui.available_width() > 0.0 {
                                                    ui.add(
                                                        egui::Label::new(
                                                            egui::RichText::new(format!(
                                                                "{dir}   ·   {when}"
                                                            ))
                                                            .weak()
                                                            .small(),
                                                        )
                                                        .truncate(),
                                                    );
                                                }
                                            },
                                        );
                                    },
                                );
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
        if clear_history {
            self.recent_files.clear();
            self.welcome_show_all = false;
        } else if let Some(path) = remove_recent {
            self.recent_files.retain(|entry| entry.path != path);
        }
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

    /// Render the active tab's content. Returns a path and optional
    /// `#fragment` when the renderer hook requests it be opened in a new tab.
    fn render_tab_content(
        &mut self,
        ui: &mut egui::Ui,
        ctrl_held: bool,
    ) -> Option<(PathBuf, Option<String>)> {
        let mut open_in_new_tab: Option<(PathBuf, Option<String>)> = None;
        let mut navigation_error = None;

        // Snapshot search state before taking a mutable borrow on the active tab
        let search_is_open = self.search.is_open;
        let remote_images = self.remote_images;
        let mut always_load_remote = false;
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

        // Ask mode: offer to load the remote images this document skipped
        // (#289). The count grows as more of the document is laid out.
        let blocked_remote = tab.cache.blocked_remote_image_count();
        if remote_images == RemoteImages::Ask && !tab.remote_images_allowed && blocked_remote > 0 {
            ui.horizontal(|ui| {
                let plural = if blocked_remote == 1 { "" } else { "s" };
                ui.label(
                    egui::RichText::new(format!(
                        "{blocked_remote} remote image{plural} not loaded."
                    ))
                    .weak(),
                );
                let load = ui
                    .small_button("Load images")
                    .on_hover_text("Load the remote images in this document");
                #[cfg(feature = "mcp")]
                self.mcp_bridge
                    .register_widget("Remote Images: Load", "button", &load, None);
                if load.clicked() {
                    tab.remote_images_allowed = true;
                }
                let always = ui
                    .small_button("Always load")
                    .on_hover_text("View → Remote Images → Load automatically");
                #[cfg(feature = "mcp")]
                self.mcp_bridge.register_widget(
                    "Remote Images: Always Load",
                    "button",
                    &always,
                    None,
                );
                if always.clicked() {
                    always_load_remote = true;
                }
            });
        }
        let load_remote_images = remote_images.allows(tab.remote_images_allowed);

        // Content area (no inner CentralPanel needed - we're already in one)
        // Left margin for breathing room, right margin prevents scrollbar/resize-handle overlap jitter
        egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: 8,
                right: CONTENT_RIGHT_RESIZE_GUTTER,
                ..Default::default()
            })
            .show(ui, |ui| {
                let content_rect = ui.available_rect_before_wrap();

                // The renderer owns the ScrollArea now (via show_scrollable),
                // so we configure scroll_source / pending offset / content
                // version through builder methods. The returned ScrollAreaOutput
                // exposes state.offset and inner_rect for the post-render
                // selection-preserving wheel hack below.
                // View mode: CommonMarkViewer
                // Edit mode: scrollable TextEdit for raw markdown
                // (the Preview|Edit toggle lives in the tab bar)

                let mut scroll_output: Option<egui::scroll_area::ScrollAreaOutput<()>> = None;

                if tab.is_editing {
                    // Edit mode: scrollable TextEdit with the raw markdown.
                    // The TextEdit's own ScrollArea handles the wheel natively;
                    // scroll_output stays None so the view-mode hacks below skip.
                    let pending_edit = tab.pending_edit_scroll.take();
                    let mut edit_area =
                        egui::ScrollArea::vertical().id_salt(tab.id.with("edit-scroll"));
                    if let Some(offset) = tab.pending_edit_offset.take() {
                        edit_area = edit_area.vertical_scroll_offset(offset);
                    }
                    let edit_out = edit_area
                        .auto_shrink(false)
                        .max_width(content_rect.width())
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                let frame = egui::Frame::NONE
                                    .inner_margin(egui::Margin::symmetric(4_i8, 2_i8));
                                frame
                                    .show(ui, |ui| {
                                        egui::TextEdit::multiline(&mut tab.content)
                                            .id_salt(ui.scope_id().with("edit"))
                                            .desired_width(content_rect.width() - 8.0)
                                            .desired_rows(24)
                                            .frame(egui::Frame::NONE)
                                            .ui(ui)
                                    })
                                    .inner
                            })
                            .inner
                        });
                    // Drag-selecting past the top or bottom edge scrolls the
                    // text (#258). egui's TextEdit only scrolls to its cursor
                    // when the selection changes, and a pointer held still
                    // below the area changes nothing, so the view never moved.
                    if edit_out.inner.dragged() {
                        if let Some(pointer) = ui.ctx().pointer_latest_pos() {
                            let area = edit_out.inner_rect;
                            let speed = edge_autoscroll_speed(pointer.y, area.top(), area.bottom());
                            if speed != 0.0 {
                                let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.1);
                                let max_offset = (edit_out.content_size.y - area.height()).max(0.0);
                                let offset = (edit_out.state.unclamped_offset().y + speed * dt)
                                    .clamp(0.0, max_offset);
                                tab.pending_edit_offset = Some(offset);
                                ui.ctx().request_repaint();
                            }
                        }
                    }
                    tab.edit_focus.keep_through_scrollbar_press(
                        ui,
                        edit_out.inner.id,
                        edit_out.inner_rect,
                    );
                    tab.scroll_offset = edit_out.state.unclamped_offset().y;
                    tab.last_viewport_height = edit_out.inner_rect.height();
                    tab.last_content_height = edit_out.content_size.y;
                    if let Some(ratio) = pending_edit {
                        tab.pending_edit_offset = Some(ratio * edit_out.content_size.y.max(1.0));
                        ui.ctx().request_repaint();
                    }
                } else {
                    // A heading jump no longer forces a full layout: the
                    // renderer measures toward it instead (devlog 115).
                    let force_full_render = correct_search_this_frame;
                    let pending = tab.pending_scroll_offset.take();
                    let default_width = content_default_width(self.full_width_content);
                    let content_width =
                        content_width_limit(self.full_width_content, content_rect.width());
                    // Line heights follow the emulated viewer (GitHub 1.5/1.45,
                    // VS Code preview 1.6/1.36); the font preset owns the metrics.
                    let (preset_line_height, preset_code_line_height) =
                        self.font_preset.line_heights();
                    scroll_output = Some(
                        CommonMarkViewer::new()
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
                            // Large documents: lay out the first screens in the
                            // first frame and measure the rest over the next
                            // frames, instead of all of it before the first
                            // paint (devlog 103).
                            .progressive_measure(true)
                            // Frontmatter is metadata, not prose: show it as a
                            // key/value table rather than a thematic break plus a
                            // paragraph of raw `key: value` lines (#117).
                            .render_frontmatter(true)
                            .math_scale(self.math_scale)
                            .load_remote_images(load_remote_images)
                            .show_alt_text_on_hover(true)
                            .syntax_theme_dark("base16-ocean.dark")
                            .syntax_theme_light("base16-ocean.light")
                            .line_height(preset_line_height)
                            .code_line_height(preset_code_line_height)
                            .paragraph_spacing(2.0)
                            .heading_spacing_above(2.0)
                            .heading_spacing_below(0.75)
                            .content_version(tab.content_version)
                            .pending_scroll_offset(pending)
                            .force_full_render(force_full_render)
                            .measure_to_source(
                                tab.pending_header_jump.map(|jump| jump.source_start),
                            )
                            .scroll_source(egui::scroll_area::ScrollSource {
                                scroll_bar: true,
                                drag: egui::scroll_area::DragScroll::Never,
                                mouse_wheel: true,
                            })
                            .show_scrollable(tab.id, ui, &mut tab.cache, &tab.content),
                    );

                    if let Some(so) = scroll_output.as_ref() {
                        tab.scroll_offset = so.state.unclamped_offset().y;
                        tab.last_viewport_height = so.inner_rect.height();
                        tab.last_content_height = so.content_size.y;
                    }
                }

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
                        // View-mode only: scroll_output is None while editing.
                        if let Some(so) = scroll_output.as_ref() {
                            let current_scroll = so.state.unclamped_offset().y;
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
                }

                // Pending heading jump (outline click, `#fragment`): scroll once
                // the heading's position is recorded, keep measuring toward it
                // until then, and fall back to the line-ratio estimate if the
                // document is measured past it without a position (devlog 115).
                if let Some(jump) = tab.pending_header_jump {
                    match scroll_output.as_ref() {
                        // Edit mode renders no document; drop the jump.
                        None => tab.pending_header_jump = None,
                        Some(so) => {
                            let position = tab
                                .cache
                                .get_header_position(&header_position_key(jump.source_start));
                            match resolve_header_jump(
                                position,
                                tab.cache.measured_source_end(tab.id),
                                jump.source_start,
                            ) {
                                HeaderJumpStep::ScrollTo(want_scroll) => {
                                    tab.pending_header_jump = None;
                                    let current_scroll = so.state.unclamped_offset().y;
                                    if (want_scroll - current_scroll).abs() > 2.0 {
                                        tab.pending_scroll_offset = Some(want_scroll);
                                    }
                                }
                                HeaderJumpStep::Wait => ui.ctx().request_repaint(),
                                HeaderJumpStep::Estimate => {
                                    tab.pending_header_jump = None;
                                    if tab.last_content_height > 0.0 && tab.content_lines > 0 {
                                        let estimated_y = (jump.line_number as f32
                                            / tab.content_lines as f32)
                                            * tab.last_content_height;
                                        tab.pending_scroll_offset =
                                            Some((estimated_y - 50.0).max(0.0));
                                    }
                                }
                            }
                        }
                    }
                }

                // Request repaint during smooth scrolling
                if ui.ctx().input(|i| i.smooth_scroll_delta.length_sq() > 0.0) {
                    ui.ctx().request_repaint();
                }
            });

        // A click on a remote-image placeholder loads this document's remote
        // images in Ask mode; Block keeps them blocked.
        if tab.cache.take_remote_images_request() && remote_images == RemoteImages::Ask {
            tab.remote_images_allowed = true;
            ui.ctx().request_repaint();
        }

        // Check for clicked links
        if let Some(clicked_link) = tab.check_link_hooks() {
            (open_in_new_tab, navigation_error) =
                handle_local_link_click(tab, &clicked_link, ctrl_held);
        }

        if let Some(error) = navigation_error {
            self.error_message = Some(error);
        }
        if always_load_remote {
            self.remote_images = RemoteImages::Load;
        }

        open_in_new_tab
    }

    /// Render the file explorer sidebar
    /// Returns actions for files to open or close
    fn render_empty_explorer(&mut self, ui: &mut egui::Ui) {
        ui.strong("No folder open");
        if ui.button("Open Folder…").clicked() {
            self.open_folder_dialog();
            return;
        }

        ui.separator();
        ui.strong("Recent Folders");
        if self.recent_folders.is_empty() {
            ui.label(egui::RichText::new("No recent folders yet.").weak());
            return;
        }

        if ui.small_button("Clear History").clicked() {
            self.recent_folders.clear();
            return;
        }

        let mut open = None;
        let mut remove = None;
        egui::ScrollArea::vertical()
            .id_salt("recent_folders")
            .show(ui, |ui| {
                for (index, path) in self.recent_folders.iter().enumerate() {
                    let full_path = path.to_string_lossy();
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy())
                        .unwrap_or_else(|| full_path.clone());
                    let row_size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
                    ui.allocate_ui_with_layout(
                        row_size,
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            // Reserve the remove button's actual width before laying out the name.
                            if ui
                                .small_button("✕")
                                .on_hover_text("Remove from history")
                                .clicked()
                            {
                                remove = Some(index);
                            }
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
                                    if ui
                                        .add(egui::Button::new(name.as_ref()).truncate())
                                        .on_hover_text(full_path.as_ref())
                                        .clicked()
                                    {
                                        open = Some(path.clone());
                                    }
                                },
                            );
                        },
                    );
                }
            });
        if let Some(index) = remove {
            self.recent_folders.remove(index);
        }
        if let Some(path) = open {
            self.open_folder(path);
        }
    }

    fn render_file_explorer(&mut self, ui: &mut egui::Ui) -> ExplorerAction {
        // egui 0.37: panels are shown in the root `Ui`, not on the `Context`.
        let ctx_owned = ui.ctx().clone();
        let ctx = &ctx_owned;
        let mut action = ExplorerAction::default();

        if !self.show_explorer {
            return action;
        }

        let panel = egui::Panel::left("file_explorer")
            .resizable(true)
            .default_size(self.explorer_width)
            .size_range(SIDEBAR_MIN_WIDTH..=f32::INFINITY)
            .frame(
                egui::Frame::side_top_panel(&ctx.global_style()).inner_margin(egui::Margin {
                    left: 8,
                    right: EXPLORER_RIGHT_RESIZE_GUTTER,
                    top: 8,
                    bottom: 8,
                }),
            )
            .show(ui, |ui| {
                // Header with folder name - OUTSIDE ScrollArea
                if let Some(root) = &self.file_explorer.root {
                    let folder_name = root
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| root.display().to_string());
                    ui.strong(&folder_name);
                } else {
                    self.render_empty_explorer(ui);
                    return;
                }

                // Expand/collapse/refresh buttons - OUTSIDE ScrollArea
                ui.horizontal(|ui| {
                    let expand_btn = ui
                        .add_enabled(
                            self.file_explorer.root.is_some()
                                && self.file_explorer.pending_scan.is_none(),
                            egui::Button::new("⊞").small(),
                        )
                        .on_hover_text("Expand all directories");
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Explorer: Expand All",
                        "button",
                        &expand_btn,
                        None,
                    );
                    if expand_btn.clicked() && !self.file_explorer.expand_all() {
                        self.error_message = Some("Unable to start expanding directories.".into());
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

                // Take the tree so its rows can be borrowed while `self`
                // renders them, then put it back.
                let tree = std::mem::take(&mut self.file_explorer.tree);
                // Clone the small set of open tab paths (avoids borrow conflict with &mut self)
                let open_paths = self.open_tab_paths.clone();
                let mut rows = Vec::new();
                let mut unloaded = Vec::new();
                FileExplorer::visible_rows(
                    &tree,
                    &self.file_explorer.expanded_dirs,
                    0,
                    &mut rows,
                    &mut unloaded,
                );
                let row_height = explorer_row_height(ui);

                // Lay out only the rows in view: an expanded tree can list
                // thousands, and laying them all out every frame cost most
                // of a core while the pointer moved (devlog 120).
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .id_salt("file_explorer")
                    .show_rows(ui, row_height, rows.len(), |ui, row_range| {
                        for &(depth, node) in &rows[row_range] {
                            let row_action =
                                self.render_tree_row(ui, node, depth, &open_paths, row_height);
                            if row_action.file_to_open.is_some() {
                                action.file_to_open = row_action.file_to_open;
                            }
                            if row_action.file_to_close.is_some() {
                                action.file_to_close = row_action.file_to_close;
                            }
                            if row_action.dir_to_toggle.is_some() {
                                action.dir_to_toggle = row_action.dir_to_toggle;
                            }
                        }
                    });
                drop(rows);

                // Put tree back and apply deferred toggle
                self.file_explorer.tree = tree;
                // Expanded folders whose children are not loaded yet (a
                // restored or refreshed tree) load now and list next frame.
                if !unloaded.is_empty() {
                    for dir in &unloaded {
                        self.file_explorer.load_children(dir);
                    }
                    ui.ctx().request_repaint();
                }
                if let Some(ref dir_path) = action.dir_to_toggle {
                    self.file_explorer.toggle_expanded(dir_path);
                    // Keep the non-recursive explorer watches in sync with the
                    // newly expanded/collapsed directory.
                    self.reconcile_explorer_watches();
                }
            });
        self.explorer_width = panel.response.rect.width();
        // Left panel: the draggable edge is its right border.
        paint_sidebar_resize_affordance(
            ctx,
            panel.response.rect.right(),
            panel.response.rect.y_range(),
            ui.scope_id().with_salt(egui::IdSalt::new("file_explorer")),
        );

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

    /// Render one Explorer row, exactly `row_height` tall so `show_rows` can
    /// place rows by index. Children are separate rows (`visible_rows`).
    fn render_tree_row(
        &mut self,
        ui: &mut egui::Ui,
        node: &FileTreeNode,
        depth: usize,
        open_paths: &HashSet<PathBuf>,
        row_height: f32,
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
                    ui.set_min_height(row_height);
                    ui.add_space(indent as f32);

                    // File icon (vector-drawn — no emoji in app chrome)
                    paint_file_icon(ui, 14.0);

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
                    ui.set_min_height(row_height);
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

                    // Vector folder icon; expanded state shows in the fill.
                    paint_folder_icon(ui, 14.0, is_expanded);

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

    fn render_font_settings(&mut self, ctx: &egui::Context) {
        if !self.show_font_dialog {
            return;
        }
        // The family list is only needed here, so scan on first open rather
        // than at every startup (devlog 109).
        self.font_families.request(spawn_font_family_scan);

        let mut open = true;
        // Set inside the window closure, applied after `.show()` returns —
        // the list rows borrow `font_families.names` while the closure
        // runs, so a picked name can't mutate `self` in place.
        let mut new_selection: Option<Option<String>> = None;

        // Single font dialog: preset, text size, and family picker in one
        // place. Handlers only mutate state — the `last_applied_font_config`
        // diff gate in update() rebuilds the font chains once per change.
        egui::Window::new("Fonts")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(360.0)
            .show(ctx, |ui| {
                // Markdown font presets. Missing preset faces degrade to the
                // app's own fallback chain.
                ui.label("Preset");
                ui.horizontal_wrapped(|ui| {
                    for preset in FontPreset::ALL {
                        let active = self.font_preset == preset;
                        let preset_btn = ui
                            .selectable_label(active, preset.label())
                            .on_hover_text(preset.description());
                        #[cfg(feature = "mcp")]
                        self.mcp_bridge.register_widget(
                            &format!("Font Dialog: Preset: {}", preset.label()),
                            "button",
                            &preset_btn,
                            Some(if active { "selected" } else { "" }),
                        );
                        if preset_btn.clicked() && !active {
                            self.font_preset = preset;
                        }
                    }
                });
                if self.selected_font_family.is_some() {
                    ui.small(
                        "A picked family leads the body chain; presets still \
                         set the code font and line heights.",
                    );
                }

                // Shared text-size classes: one scale for every preset.
                ui.add_space(2.0);
                ui.label("Text Size");
                ui.horizontal_wrapped(|ui| {
                    for size in TextSizeClass::ALL {
                        let active = self.text_size_class == size;
                        let size_btn = ui
                            .selectable_label(active, size.label())
                            .on_hover_text("Base text size; headings and code scale with it");
                        #[cfg(feature = "mcp")]
                        self.mcp_bridge.register_widget(
                            &format!("Font Dialog: Size: {}", size.label()),
                            "button",
                            &size_btn,
                            Some(if active { "selected" } else { "" }),
                        );
                        if size_btn.clicked() && !active {
                            self.text_size_class = size;
                        }
                    }
                });

                ui.add_space(4.0);
                ui.separator();
                ui.label("Family");

                ui.horizontal(|ui| {
                    ui.label("Search:");
                    let filter_edit = ui.text_edit_singleline(&mut self.font_filter);
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Font Dialog: Search",
                        "textbox",
                        &filter_edit,
                        None,
                    );
                    #[cfg(not(feature = "mcp"))]
                    let _ = filter_edit;
                });

                // One canonical name per body-capable family, scanned once in
                // the background when the dialog first opens. While the scan is
                // running, only "System Default" is offered.
                if self.font_families.scanning() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Scanning installed fonts…");
                    });
                }

                // Only the empty-filter (common) case avoids allocating: it
                // reuses `font_families.names` directly. A non-empty
                // filter needs one Vec<usize> of matching indices, built from
                // the precomputed lowercase names (no per-name allocation).
                let filter = self.font_filter.to_ascii_lowercase();
                let filtered_indices: Option<Vec<usize>> = if filter.is_empty() {
                    None
                } else {
                    Some(
                        self.font_families
                            .names_lower
                            .iter()
                            .enumerate()
                            .filter(|(_, lower)| lower.contains(&filter))
                            .map(|(i, _)| i)
                            .collect(),
                    )
                };
                let match_count = filtered_indices
                    .as_ref()
                    .map_or(self.font_families.names.len(), Vec::len);

                let row_height = ui.spacing().interact_size.y;
                // auto_shrink_x = false: egui's default (TRUE) shrinks the
                // area to its widest row, which paints the floating scrollbar
                // mid-dialog instead of at the window's right edge.
                egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .max_height(300.0)
                    .show_rows(
                        ui,
                        row_height,
                        match_count + 1, // + 1 for the pinned "System Default" row
                        |ui, rows| {
                            for row in rows {
                                if row == 0 {
                                    let default_selected = self.selected_font_family.is_none();
                                    let default_btn =
                                        ui.selectable_label(default_selected, "System Default");
                                    #[cfg(feature = "mcp")]
                                    self.mcp_bridge.register_widget(
                                        "Font Dialog: System Default",
                                        "button",
                                        &default_btn,
                                        Some(if default_selected { "selected" } else { "" }),
                                    );
                                    if default_btn.clicked() {
                                        new_selection = Some(None);
                                    }
                                    continue;
                                }
                                let font_index = filtered_indices
                                    .as_ref()
                                    .map_or(row - 1, |indices| indices[row - 1]);
                                let name = &self.font_families.names[font_index];
                                let is_selected =
                                    self.selected_font_family.as_deref() == Some(name.as_str());
                                if ui.selectable_label(is_selected, name).clicked() {
                                    new_selection = Some(Some(name.clone()));
                                }
                            }
                        },
                    );
            });

        self.show_font_dialog = open;
        if let Some(choice) = new_selection {
            self.selected_font_family = choice;
        }
    }

    fn render_lightbox(&mut self, ctx: &egui::Context) {
        let Some(lightbox) = &mut self.lightbox else {
            return;
        };

        let screen_rect = self.document_rect;
        let mut should_close = false;

        // 1. Semi-transparent backdrop (visual only, drawn below Tooltip layers)
        ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("lightbox_backdrop"),
        ))
        .rect_filled(
            screen_rect,
            0.0,
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200),
        );

        // 2. Full-screen input sink at Tooltip order — blocks all input to lower layers.
        //    Clicking on the sink (outside image/close) closes the lightbox.
        let oid = lightbox.open_id;
        egui::Area::new(egui::Id::unique("lightbox_sink").with(oid))
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

        egui::Area::new(egui::Id::unique("lightbox_image").with(oid))
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
                .id_salt(egui::Id::unique("lightbox_scroll").with(oid))
                .max_width(area_size.x)
                .max_height(area_size.y)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .scroll_source(egui::scroll_area::ScrollSource {
                    // Mouse drag pans the zoomed image (not only on touch).
                    drag: egui::scroll_area::DragScroll::Always,
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
                    lightbox.scroll_offset = scroll_output.state.unclamped_offset();
                }
            });

        // 5. Close button (top-right)
        egui::Area::new(egui::Id::unique("lightbox_close").with(oid))
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
                egui::Id::unique("lightbox_zoom_indicator").with(oid),
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

/// Height of the band inside each edge of the edit area where a drag
/// selection starts scrolling (#258). On a touchpad the pointer often cannot
/// leave the window, so scrolling must begin before the edge.
const EDGE_AUTOSCROLL_ZONE: f32 = 24.0;

/// Scroll speed, in points per second (negative is up), for a drag selection
/// whose pointer is at `pointer_y` in an area spanning `top..bottom`. Zero
/// away from the edges; inside the edge band it starts at a crawl and grows
/// with how far the pointer has gone, up to a cap.
fn edge_autoscroll_speed(pointer_y: f32, top: f32, bottom: f32) -> f32 {
    const MIN_SPEED: f32 = 120.0;
    const SPEED_PER_POINT: f32 = 12.0;
    const MAX_SPEED: f32 = 3000.0;
    let zone = EDGE_AUTOSCROLL_ZONE.min((bottom - top) / 4.0).max(0.0);
    let speed = |depth: f32| (MIN_SPEED + depth * SPEED_PER_POINT).min(MAX_SPEED);
    if pointer_y > bottom - zone {
        speed(pointer_y - (bottom - zone))
    } else if pointer_y < top + zone {
        -speed((top + zone) - pointer_y)
    } else {
        0.0
    }
}

/// The editor's focus and selection as of the end of the previous frame.
///
/// A press on the editor's scrollbar is a press outside the TextEdit: egui
/// takes the focus away, and an unfocused TextEdit collapses its selection to
/// the cursor (`TextEdit::show`, when it does not own the IME events), so the
/// selection was gone after a click on the scrollbar track (#258).
#[derive(Default)]
struct EditorFocus {
    had_focus: bool,
    selection: Option<egui::text::CCursorRange>,
}

impl EditorFocus {
    /// Call after the editor's ScrollArea has been shown. If a press in its
    /// scrollbar strip took the focus this frame, restores the selection and
    /// the focus; then records both for the next frame.
    fn keep_through_scrollbar_press(
        &mut self,
        ui: &egui::Ui,
        edit_id: egui::Id,
        inner_rect: egui::Rect,
    ) {
        let focused = ui.memory(|m| m.has_focus(edit_id));
        if self.had_focus && !focused {
            // A press and release in one frame (a quick click) has no
            // press origin any more; the pointer is still where it pressed.
            let press = ui.input(|i| {
                i.pointer
                    .any_pressed()
                    .then(|| i.pointer.press_origin().or(i.pointer.latest_pos()))
                    .flatten()
            });
            let strip = edit_scrollbar_strip(inner_rect, &ui.spacing().scroll);
            if press.is_some_and(|pos| strip.contains(pos)) {
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), edit_id) {
                    state.cursor.set_char_range(self.selection);
                    state.store(ui.ctx(), edit_id);
                }
                ui.memory_mut(|m| m.request_focus(edit_id));
                ui.ctx().request_repaint();
            }
        }
        self.had_focus = ui.memory(|m| m.has_focus(edit_id));
        if self.had_focus {
            self.selection = egui::TextEdit::load_state(ui.ctx(), edit_id)
                .and_then(|state| state.cursor.char_range());
        }
    }
}

/// The strip along the right edge of the editor's ScrollArea where its
/// vertical scrollbar is drawn and takes presses. Depending on the style the
/// bar lies over the content inside `inner_rect` or just outside it, so the
/// strip covers both sides of the edge.
fn edit_scrollbar_strip(inner_rect: egui::Rect, scroll: &egui::style::ScrollStyle) -> egui::Rect {
    let reach = scroll.bar_inner_margin + scroll.bar_width + scroll.bar_outer_margin;
    egui::Rect::from_x_y_ranges(
        inner_rect.right() - reach..=inner_rect.right() + reach,
        inner_rect.y_range(),
    )
}

/// Frame milestones for the `MDV_SELFTEST` harness: scroll injection and
/// the scroll-walk start at [`SELFTEST_SETTLE_FRAME`]; the geometry dump
/// begins (and the walk ends) at [`SELFTEST_DUMP_FRAME`]; the app exits at
/// [`SELFTEST_EXIT_FRAME`]. The gaps between them are settle time — enough
/// frames for fonts, panels, and async math textures to land before any
/// scroll state is measured.
const SELFTEST_SETTLE_FRAME: usize = 30;
const SELFTEST_DUMP_FRAME: usize = 120;
const SELFTEST_EXIT_FRAME: usize = 140;

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
            show_tab_bar: Some(self.show_tab_bar),
            full_width_content: Some(self.full_width_content),
            math_scale: Some(self.math_scale),
            highlight_color: self.highlight_color.map(|c| [c.r(), c.g(), c.b(), c.a()]),
            link_color: self.link_color.map(|c| [c.r(), c.g(), c.b(), c.a()]),
            selected_font_family: self.selected_font_family.clone(),
            smooth_text_rendering: Some(self.smooth_text_rendering),
            remote_images: Some(self.remote_images),
            font_preset: Some(self.font_preset),
            text_size_class: Some(self.text_size_class),
            open_tabs: Some(self.get_open_tab_paths()),
            active_tab: Some(self.active_tab),
            show_explorer: Some(self.show_explorer),
            explorer_root: self.file_explorer.root.clone(),
            explorer_closed: Some(self.file_explorer.root.is_none()),
            expanded_dirs: Some(self.file_explorer.expanded_dirs.iter().cloned().collect()),
            explorer_sort_order: Some(self.file_explorer.sort_order),
            explorer_width: Some(self.explorer_width),
            outline_width: Some(self.outline_width),
            recent_files: Some(self.recent_files.clone()),
            recent_folders: Some(self.recent_folders.clone()),
        };
        eframe::set_value(storage, APP_KEY, &state);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // eframe 0.37 hands the app the root `Ui`; the ctx-level work below is
        // unchanged and panels are shown in `ui`.
        let ctx_owned = ui.ctx().clone();
        let ctx = &ctx_owned;
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
            if let Some(truncated) = self.file_explorer.poll_pending_scan() {
                log::info!("Explorer directory scan completed");
                if truncated {
                    // The folder or the row budget can be the one that ran out.
                    self.error_message = Some(format!(
                        "Expand All stopped after {} directories. You can expand more individually.",
                        self.file_explorer.expanded_dirs.len()
                    ));
                }
                self.reconcile_explorer_watches();
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

        // Collect the background font-family scan (async phase: poll the
        // channel, never block the UI thread). The list may land while the
        // font dialog is open; repaint so it appears without input.
        if self.font_families.poll() {
            self.egui_ctx.request_repaint();
        }

        // Install the full font set built after a base-set startup, unless the
        // font config changed meanwhile (that change already installed a
        // full set synchronously below, and this result is stale).
        if let Some(rx) = &self.pending_full_fonts {
            match rx.try_recv() {
                Ok((config, definitions)) => {
                    self.pending_full_fonts = None;
                    if self.last_applied_font_config.as_ref() == Some(&config) {
                        log::info!("Installing the background-built full font set");
                        install_fonts(ctx, definitions, config.2);
                        ctx.request_repaint();
                    }
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.pending_full_fonts = None,
            }
        }

        // Reload fonts only when the selected family, preset, or size class
        // actually changes — rescanning the system font collection on every
        // frame would be expensive and is unnecessary (see
        // docs/EGUI_WORKFLOW.md).
        let font_config = (
            self.selected_font_family.clone(),
            self.font_preset,
            self.text_size_class,
        );
        if self.last_applied_font_config.as_ref() != Some(&font_config) {
            self.last_applied_font_config = Some(font_config.clone());
            setup_fonts(
                ctx,
                self.selected_font_family.as_deref(),
                self.font_preset,
                self.text_size_class,
            );
        }

        // Apply display zoom only when it actually changed: set_zoom_factor
        // is not free (it can invalidate glyph caches), and the value only
        // moves on user input or state restore.
        if self.last_applied_zoom_level != Some(self.zoom_level) {
            ctx.set_zoom_factor(self.zoom_level);
            self.last_applied_zoom_level = Some(self.zoom_level);
        }

        // Disabling pixel-snapping lets glyphs render at their exact
        // sub-pixel position instead of each being individually rounded to
        // the physical pixel grid — the rounding is what makes text look
        // unevenly spaced/jagged at the fractional effective pixel ratios
        // this app's zoom levels commonly produce. Trivial cost (one write
        // into egui's own memory), so no last-applied change-gate needed.
        ctx.tessellation_options_mut(|opts| {
            opts.round_text_to_pixels = !self.smooth_text_rendering;
        });

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
        let mut toggle_full_width = false;
        let mut toggle_edit_mode = false;
        let mut quit_app = false;
        let mut zoom_factor_request: Option<f32> = None;
        let mut zoom_reset = false;
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
        let mut save_request = false;

        // Ctrl+/- zoom: applies to lightbox when open, document otherwise.
        // Steps are multiplicative (×1.25) like browsers and the lightbox.
        ctx.input(|i| {
            if i.modifiers.ctrl
                && (i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals))
            {
                zoom_factor_request = Some(ZOOM_STEP);
            }
            if i.modifiers.ctrl && i.key_pressed(egui::Key::Minus) {
                zoom_factor_request = Some(1.0 / ZOOM_STEP);
            }
            if i.modifiers.ctrl && i.key_pressed(egui::Key::Num0) {
                if let Some(lightbox) = &mut self.lightbox {
                    // Reset lightbox zoom only; the document has its own reset.
                    lightbox.zoom = 1.0;
                } else {
                    zoom_reset = true;
                }
            }
        });

        // Apply zoom to lightbox or document
        if let Some(factor) = zoom_factor_request {
            if let Some(lightbox) = &mut self.lightbox {
                lightbox.zoom = (lightbox.zoom * factor).clamp(0.1, 10.0);
            } else {
                self.zoom_level = zoomed(self.zoom_level, factor);
            }
        }
        if zoom_reset && self.lightbox.is_none() {
            self.zoom_level = 1.0;
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
                // Ctrl+E: Toggle Preview/Edit for the active tab. Reachable
                // with the tab bar hidden, where the segmented toggle lives.
                if i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::E) {
                    toggle_edit_mode = true;
                }
                // Ctrl+Shift+W: Toggle full width
                if i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::W) {
                    toggle_full_width = true;
                }
                // Ctrl+W: Close current tab (not Ctrl+Shift+W, which toggles full width)
                if i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::W) {
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
                // Ctrl+S: save the active tab's edit buffer to disk
                if i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::S) {
                    save_request = true;
                }
                // Ctrl + scroll wheel / pinch to zoom (multiplicative, like
                // browsers). The lightbox captures wheel input itself in
                // raw_input_hook, so this only runs for the document.
                let wheel_zoom_factor = i.zoom_delta();
                if wheel_zoom_factor != 1.0 {
                    self.zoom_level = zoomed(self.zoom_level, wheel_zoom_factor);
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
        if toggle_full_width {
            self.full_width_content = !self.full_width_content;
        }
        if toggle_edit_mode {
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                let to_edit = !tab.is_editing;
                tab.set_edit_mode(to_edit);
            }
        }
        if quit_app {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Ctrl+S: persist the active tab's edit buffer. Only meaningful in
        // edit mode with unsaved changes; the flash label in the edit
        // toolbar reports the outcome.
        if save_request {
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                if tab.is_editing && tab.content != tab.saved_content {
                    match tab.save_to_disk() {
                        Ok(()) => {
                            log::info!("Saved tab edits: {:?}", tab.path);
                            tab.save_flash = Some((true, Instant::now()));
                        }
                        Err(error) => {
                            log::error!("Failed to save {:?}: {error}", tab.path);
                            self.error_message =
                                Some(format!("Unable to save {}: {error}", tab.path.display()));
                            tab.save_flash = Some((false, Instant::now()));
                        }
                    }
                }
            }
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
                // egui 0.37: every dropped file has a path (absolute on native).
                let path = file.path();
                if Self::is_markdown_file(path) {
                    self.open_in_new_tab(path.to_path_buf());
                } else {
                    self.error_message = Some(
                        "Unsupported file type. Please drop a markdown file (.md, .markdown, .txt)"
                            .to_string(),
                    );
                }
            }
        });

        // Get ctrl state for link handling
        let ctrl_held = ctx.input(|i| i.modifiers.ctrl || i.modifiers.command);

        // Menu bar
        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui
                        .add(egui::Button::new("Open File in New Tab...").shortcut_text("Ctrl+T"))
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

                    if ui.button("Open Folder...").clicked() {
                        self.open_folder_dialog();
                        ui.close();
                    }

                    if ui
                        .add_enabled(
                            self.file_explorer.root.is_some(),
                            egui::Button::new("Close Folder"),
                        )
                        .clicked()
                    {
                        self.close_folder();
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
                        "☀ Light Mode" // ☀ is text-presentation → monochrome
                    } else {
                        "☾ Dark Mode" // ☾ (U+263E) is a text symbol, not emoji
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

                    let tab_bar_text = if self.show_tab_bar {
                        "✓ Show Tab Bar"
                    } else {
                        "Show Tab Bar"
                    };
                    let tab_bar_btn = ui.add(egui::Button::new(tab_bar_text));
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Menu: View → Show Tab Bar",
                        "button",
                        &tab_bar_btn,
                        Some(if self.show_tab_bar { "on" } else { "off" }),
                    );
                    if tab_bar_btn.clicked() {
                        self.show_tab_bar = !self.show_tab_bar;
                        ui.close();
                    }

                    let editing = self
                        .tabs
                        .get(self.active_tab)
                        .is_some_and(|tab| tab.is_editing);
                    let edit_text = if editing {
                        "✓ Edit Mode"
                    } else {
                        "Edit Mode"
                    };
                    let edit_btn = ui.add_enabled(
                        self.tabs.get(self.active_tab).is_some(),
                        egui::Button::new(edit_text).shortcut_text("Ctrl+E"),
                    );
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Menu: View → Edit Mode",
                        "button",
                        &edit_btn,
                        Some(if editing { "on" } else { "off" }),
                    );
                    if edit_btn.clicked() {
                        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                            tab.set_edit_mode(!editing);
                        }
                        ui.close();
                    }

                    let full_width_text = if self.full_width_content {
                        "✓ Full Width"
                    } else {
                        "Full Width"
                    };
                    let full_width_btn =
                        ui.add(egui::Button::new(full_width_text).shortcut_text("Ctrl+Shift+W"));
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

                    // Single font entry point: opens the Fonts dialog, which
                    // holds the preset, the text size, and the family picker.
                    let fonts_btn = ui.add(egui::Button::new("Fonts…"));
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Menu: View → Fonts",
                        "button",
                        &fonts_btn,
                        self.selected_font_family.as_deref(),
                    );
                    if fonts_btn.clicked() {
                        self.show_font_dialog = true;
                        ui.close();
                    }

                    let smooth_text_text = if self.smooth_text_rendering {
                        "✓ Smooth Text Rendering"
                    } else {
                        "Smooth Text Rendering"
                    };
                    let smooth_text_btn = ui.add(egui::Button::new(smooth_text_text));
                    #[cfg(feature = "mcp")]
                    self.mcp_bridge.register_widget(
                        "Menu: View → Smooth Text Rendering",
                        "button",
                        &smooth_text_btn,
                        Some(if self.smooth_text_rendering {
                            "on"
                        } else {
                            "off"
                        }),
                    );
                    if smooth_text_btn.clicked() {
                        self.smooth_text_rendering = !self.smooth_text_rendering;
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
                        self.zoom_level = zoomed(self.zoom_level, ZOOM_STEP);
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
                        self.zoom_level = zoomed(self.zoom_level, 1.0 / ZOOM_STEP);
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
                    // Images with an http(s) URI are the only thing a
                    // document can make md-viewer fetch (#289).
                    ui.menu_button("Remote Images", |ui| {
                        for policy in RemoteImages::ALL {
                            let selected = self.remote_images == policy;
                            let text = if selected {
                                format!("✓ {}", policy.label())
                            } else {
                                format!("   {}", policy.label())
                            };
                            let btn = ui.add(egui::Button::new(text));
                            #[cfg(feature = "mcp")]
                            self.mcp_bridge.register_widget(
                                &format!("Menu: View → Remote Images → {}", policy.label()),
                                "button",
                                &btn,
                                Some(if selected { "selected" } else { "unselected" }),
                            );
                            if btn.clicked() {
                                self.remote_images = policy;
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
            egui::Panel::top("error_bar").show(ui, |ui| {
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
        let search_outcome = self.render_search_bar(ui);
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
        if tab_bar_visible(
            self.show_tab_bar,
            self.tabs
                .get(self.active_tab)
                .is_some_and(|tab| tab.is_editing),
        ) {
            egui::Panel::top("tab_bar").show(ui, |ui| {
                tab_to_close = self.render_tab_bar(ui);
            });
        }

        // Close tab if requested
        if let Some(idx) = tab_to_close {
            self.close_tab(idx);
        }

        // File explorer (left sidebar)
        let explorer_action = self.render_file_explorer(ui);

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
        self.render_outline(ui);

        // Main content area
        let mut open_in_new_tab: Option<(PathBuf, Option<String>)> = None;
        let path_before_render = self.tabs.get(self.active_tab).map(|tab| tab.path.clone());
        let central = egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.global_style()).inner_margin(egui::Margin::ZERO))
            .show(ui, |ui| {
                open_in_new_tab = self.render_tab_content(ui, ctrl_held);
            });
        self.document_rect = central.response.rect;

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
        if let Some((path, fragment)) = open_in_new_tab {
            self.open_in_new_tab_with_fragment(path, fragment);
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
        // Document font picker window
        self.render_font_settings(ctx);

        // Drag and drop overlay
        if self.is_dragging {
            let screen_rect = self.document_rect;
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::unique("drop_overlay"),
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

        // Live-app self test harness. `MDV_SELFTEST=1` runs the real app —
        // real fonts, zoom, side panels, async math — for a fixed number of
        // frames (milestones: SELFTEST_SETTLE_FRAME, SELFTEST_DUMP_FRAME,
        // SELFTEST_EXIT_FRAME), then dumps the scroll geometry of the active
        // tab to stderr and exits without saving state.
        // `MDV_SELFTEST_SCROLL=<pts>` injects a pending scroll offset at the
        // settle frame so the dump proves the offset can reach the true
        // document bottom (and reveals any clamp that snaps it back short).
        // `MDV_SELFTEST_SCROLLWALK=<from>:<to>:<steps>` instead walks the
        // pending offset frame by frame from the settle frame to the dump
        // frame, simulating a wheel scroll across the given range — this
        // exposes extent behavior that only appears while the viewport moves.
        if std::env::var("MDV_SELFTEST").is_ok() {
            self.selftest_frames += 1;
            let frame = self.selftest_frames;
            if let Ok(target) = std::env::var("MDV_SELFTEST_SCROLL") {
                if frame == SELFTEST_SETTLE_FRAME {
                    if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                        tab.pending_scroll_offset = Some(target.parse().unwrap_or(3000.0));
                    }
                }
            }
            if let Ok(walk) = std::env::var("MDV_SELFTEST_SCROLLWALK") {
                if (SELFTEST_SETTLE_FRAME..SELFTEST_DUMP_FRAME).contains(&frame) {
                    let parts: Vec<f32> = walk.split(':').filter_map(|p| p.parse().ok()).collect();
                    if parts.len() == 3 {
                        let (from, to, steps) = (parts[0], parts[1], parts[2].max(1.0));
                        let t = ((frame - SELFTEST_SETTLE_FRAME) as f32 / steps).min(1.0);
                        let offset = from + (to - from) * t;
                        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                            tab.pending_scroll_offset = Some(offset);
                        }
                    }
                }
            }
            if frame >= SELFTEST_DUMP_FRAME {
                if let Some(tab) = self.tabs.get(self.active_tab) {
                    eprintln!(
                        "SELFTEST f={} content={:.1} viewport={:.1} offset={:.1} max_by_content={:.1}",
                        frame,
                        tab.last_content_height,
                        tab.last_viewport_height,
                        tab.scroll_offset,
                        (tab.last_content_height - tab.last_viewport_height).max(0.0)
                    );
                }
            }
            if frame >= SELFTEST_EXIT_FRAME {
                std::process::exit(0);
            }
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory stand-in for eframe's key/value storage so persistence
    /// roundtrips run through the same RON encode/decode path as the app.
    struct MemoryStorage(HashMap<String, String>);

    impl eframe::Storage for MemoryStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }

    #[test]
    fn remote_image_policy_survives_a_persistence_roundtrip() {
        let mut storage = MemoryStorage(HashMap::new());
        let app_state = PersistedState {
            remote_images: Some(RemoteImages::Ask),
            ..PersistedState::default()
        };
        eframe::set_value(&mut storage, APP_KEY, &app_state);
        let restored: PersistedState = eframe::get_value(&storage, APP_KEY).expect("state decodes");
        assert_eq!(restored.remote_images, Some(RemoteImages::Ask));

        // State written before #289 has no field and keeps loading images.
        let legacy: PersistedState = eframe::get_value(
            &MemoryStorage(HashMap::from([(
                APP_KEY.to_owned(),
                "(dark_mode: Some(true))".to_owned(),
            )])),
            APP_KEY,
        )
        .expect("legacy state decodes");
        assert_eq!(legacy.remote_images.unwrap_or_default(), RemoteImages::Load);
    }

    #[test]
    fn remote_image_policy_decides_per_tab() {
        assert!(RemoteImages::Load.allows(false));
        assert!(!RemoteImages::Ask.allows(false));
        assert!(RemoteImages::Ask.allows(true));
        // Block ignores a tab's earlier "Load images".
        assert!(!RemoteImages::Block.allows(true));
    }

    #[test]
    fn font_preset_survives_a_persistence_roundtrip() {
        let mut storage = MemoryStorage(HashMap::new());
        let app_state = PersistedState {
            font_preset: Some(FontPreset::Vscode),
            text_size_class: Some(TextSizeClass::Large),
            ..PersistedState::default()
        };
        eframe::set_value(&mut storage, APP_KEY, &app_state);

        let restored: PersistedState = eframe::get_value(&storage, APP_KEY).expect("state decodes");

        assert_eq!(restored.font_preset, Some(FontPreset::Vscode));
        assert_eq!(restored.text_size_class, Some(TextSizeClass::Large));

        // Absent field (older state files) falls back to the default preset.
        let legacy: PersistedState = eframe::get_value(
            &MemoryStorage(HashMap::from([(
                APP_KEY.to_owned(),
                "(dark_mode: Some(true))".to_owned(),
            )])),
            APP_KEY,
        )
        .expect("legacy state decodes");
        assert_eq!(legacy.font_preset, None);
        assert_eq!(legacy.font_preset.unwrap_or_default(), FontPreset::Current);
        assert_eq!(legacy.text_size_class, None);
        assert_eq!(
            legacy.text_size_class.unwrap_or_default(),
            TextSizeClass::Normal
        );
    }

    #[test]
    fn document_zoom_steps_are_multiplicative_and_clamped() {
        assert_eq!(zoomed(1.0, ZOOM_STEP), 1.25);
        assert!((zoomed(1.25, 1.0 / ZOOM_STEP) - 1.0).abs() < 1e-6);

        // Same bounds the additive path clamped to before.
        assert_eq!(zoomed(2.9, ZOOM_STEP), ZOOM_MAX);
        assert_eq!(zoomed(0.6, 1.0 / ZOOM_STEP), ZOOM_MIN);

        // Neutral and degenerate factors are no-ops (egui reports 1.0 on
        // frames without a zoom gesture; guards against divide-by-style bugs).
        assert_eq!(zoomed(1.7, 1.0), 1.7);
        assert_eq!(zoomed(1.7, 0.0), 1.7);
        assert_eq!(zoomed(1.7, -2.0), 1.7);
    }

    #[test]
    fn zoom_bounds_match_the_persisted_value_clamp() {
        // PersistedState load clamps with the same constants.
        let persisted_level = 7.5_f32.clamp(ZOOM_MIN, ZOOM_MAX);
        assert_eq!(persisted_level, ZOOM_MAX);
    }

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
    fn recent_folders_are_deduplicated_and_bounded() {
        let mut folders = Vec::new();
        for i in 0..=RECENT_FOLDERS_CAP {
            push_recent_folder(&mut folders, Path::new(&format!("folder-{i}")));
        }
        push_recent_folder(&mut folders, Path::new("folder-1"));

        assert_eq!(folders.len(), RECENT_FOLDERS_CAP);
        assert_eq!(folders[0], Path::new("folder-1"));
        assert_eq!(
            folders[1],
            PathBuf::from(format!("folder-{RECENT_FOLDERS_CAP}"))
        );
        assert!(!folders.contains(&PathBuf::from("folder-0")));
    }

    #[test]
    fn explicitly_closed_explorer_stays_closed_when_opening_a_document() {
        let mut storage = MemoryStorage(HashMap::new());
        let doc = Path::new("project").join("readme.md");
        eframe::set_value(
            &mut storage,
            APP_KEY,
            &PersistedState {
                explorer_closed: Some(true),
                recent_folders: Some(vec![PathBuf::from("project")]),
                ..Default::default()
            },
        );
        let state: PersistedState = eframe::get_value(&storage, APP_KEY).unwrap();
        assert_eq!(
            initial_explorer_root(
                state.explorer_closed.unwrap_or(false),
                Some(&doc),
                state.explorer_root,
                Some(&doc),
            ),
            None
        );
        assert_eq!(state.recent_folders, Some(vec![PathBuf::from("project")]));

        let legacy: PersistedState = eframe::get_value(
            &MemoryStorage(HashMap::from([(
                APP_KEY.to_owned(),
                "(explorer_root:None)".to_owned(),
            )])),
            APP_KEY,
        )
        .unwrap();
        assert_eq!(
            initial_explorer_root(
                legacy.explorer_closed.unwrap_or(false),
                None,
                legacy.explorer_root,
                Some(&doc),
            ),
            Some(PathBuf::from("project"))
        );
    }

    #[test]
    fn closing_explorer_drops_the_tree_and_pending_scan() {
        let root = PathBuf::from("root");
        let child = root.join("docs");
        let (tx, receiver) = mpsc::channel();
        let mut explorer = FileExplorer {
            root: Some(root.clone()),
            expanded_dirs: HashSet::from([child]),
            pending_scan: Some(PendingExplorerScan {
                receiver,
                expands_all: false,
                discarded: false,
            }),
            ..Default::default()
        };
        explorer.close_root();

        assert!(explorer.root.is_none());
        assert!(explorer.tree.is_empty());
        assert!(explorer.expanded_dirs.is_empty());
        assert!(explorer.poll_pending_scan().is_none());
        assert!(explorer.watch_directories().0.is_empty());
        assert!(tx
            .send(ExplorerScanResult {
                root,
                sort_order: SortOrder::NameAsc,
                tree: Vec::new(),
                bulk_expansion: None,
            })
            .is_err());
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

    /// Characterization table for issue #141: one place that says what a
    /// Markdown destination resolves to.
    ///
    /// #188 landed this as a *baseline*, with rows marked `#141-N` recording
    /// behavior that contradicted the semantics the issue proposes. Those rows
    /// are the ones this change flips, and the markers now name what was
    /// decided rather than what was merely observed.
    #[test]
    #[cfg(unix)]
    fn local_destination_resolution_is_characterized_for_every_uri_shape() {
        let dir = Path::new("/base/doc");
        // (destination, is_local_markdown_link, resolve_local_link_path)
        // Err(()) = reports rather than resolving; Ok(None) = nothing to open.
        // One row: the destination, whether the gate accepts it, and what the
        // resolver answers.
        type Row<'a> = (&'a str, bool, Result<Option<&'a str>, ()>);
        let table: &[Row<'_>] = &[
            // 1. relative destinations resolve against the containing document
            ("guide.md", true, Ok(Some("/base/doc/guide.md"))),
            ("sub/guide.md", true, Ok(Some("/base/doc/sub/guide.md"))),
            ("../up.md", true, Ok(Some("/base/doc/../up.md"))),
            // 2. a fragment selects the document; the anchor is dropped
            ("guide.md#section", true, Ok(Some("/base/doc/guide.md"))),
            // #141-2 DECIDED: a query string is stripped like a fragment, and
            // the gate agrees because it tests the extension of the stripped
            // spelling. Before, the gate rejected `md?v=2` while the resolver
            // put `?v=2` into the file name.
            ("guide.md?v=2", true, Ok(Some("/base/doc/guide.md"))),
            ("guide.md?v=2#s", true, Ok(Some("/base/doc/guide.md"))),
            // 3. percent escapes are decoded exactly once
            ("a%20b.md", true, Ok(Some("/base/doc/a b.md"))),
            ("100%25.md", true, Ok(Some("/base/doc/100%.md"))),
            ("bad%zz.md", true, Ok(Some("/base/doc/bad%zz.md"))),
            // an unencoded `#` delimits a fragment, `%23` is a literal
            // character — a relative destination and a file:// URI agree on
            // both, which RFC 3986 requires
            ("a%23b.md", true, Ok(Some("/base/doc/a#b.md"))),
            (
                "file:///base/doc/a%23b.md",
                true,
                Ok(Some("/base/doc/a#b.md")),
            ),
            // `a#b.md` names the file `a` with fragment `b.md`, so it is
            // correctly not a Markdown link and correctly resolves to `a`
            ("a#b.md", false, Ok(Some("/base/doc/a"))),
            // #141-5 DECIDED: a destination that does not decode to UTF-8 is
            // reported instead of resolving to nothing. It still passes the
            // gate, because passing is what makes the error reachable.
            ("bad%FF.md", true, Err(())),
            // 4. platform paths
            ("/abs/guide.md", true, Ok(Some("/abs/guide.md"))),
            ("file:///tmp/guide.md", true, Ok(Some("/tmp/guide.md"))),
            (
                "file://localhost/tmp/guide.md",
                true,
                Ok(Some("/tmp/guide.md")),
            ),
            ("file://server/share/guide.md", false, Ok(None)),
            ("C:/tmp/guide.md", false, Ok(None)),
            // 8. other schemes stay external and are never reinterpreted
            ("https://example.com/guide.md", false, Ok(None)),
            ("http://example.com/guide.md", false, Ok(None)),
            ("data:text/plain,x", false, Ok(None)),
            ("mailto:a@b.c", false, Ok(None)),
            ("weird+scheme:guide.md", false, Ok(None)),
            // #141-8 DECIDED: a protocol-relative URL is external. It carries
            // no scheme, so it used to be read as an absolute filesystem path
            // and was even classified as a local Markdown link.
            ("//evil.com/guide.md", false, Ok(None)),
            // #141 DECIDED: nothing left to name a file with is external,
            // rather than resolving to the containing directory.
            ("#section", false, Ok(None)),
            ("", false, Ok(None)),
            ("?v=2", false, Ok(None)),
        ];

        for (destination, expect_local, expect_resolved) in table {
            assert_eq!(
                is_local_markdown_link(destination),
                *expect_local,
                "is_local_markdown_link({destination:?})"
            );
            let resolved = resolve_local_link_path(destination, dir)
                .map(|path| path.map(|p| p.display().to_string()))
                .map_err(|_| ());
            assert_eq!(
                resolved,
                expect_resolved.map(|p| p.map(str::to_owned)),
                "resolve_local_link_path({destination:?})"
            );
        }
    }

    /// Windows companion to the table above (#141 point 4). Drive-letter,
    /// UNC and `file://host` destinations only mean something on Windows, so
    /// they are characterized where `Path` and `Url::to_file_path` actually
    /// apply Windows rules. Like #188 this pins current behavior rather than
    /// changing it; every mismatch is reported at once, so one CI run shows
    /// the whole Windows answer.
    #[test]
    #[cfg(windows)]
    fn local_destination_resolution_is_characterized_on_windows() {
        let dir = Path::new(r"C:\base\doc");
        type Row<'a> = (&'a str, bool, Result<Option<&'a str>, ()>);
        let table: &[Row<'_>] = &[
            // relative destinations, either separator, join the document dir
            ("guide.md", true, Ok(Some(r"C:\base\doc\guide.md"))),
            (r"sub\guide.md", true, Ok(Some(r"C:\base\doc\sub\guide.md"))),
            ("sub/guide.md", true, Ok(Some(r"C:\base\doc\sub/guide.md"))),
            // a drive letter is a path, not a one-letter URI scheme
            (r"C:\docs\guide.md", true, Ok(Some(r"C:\docs\guide.md"))),
            ("C:/docs/guide.md", true, Ok(Some("C:/docs/guide.md"))),
            // file:// URIs convert through Url::to_file_path
            (
                "file:///C:/docs/guide.md",
                true,
                Ok(Some(r"C:\docs\guide.md")),
            ),
            (
                "file:///C:/docs/a%20b.md",
                true,
                Ok(Some(r"C:\docs\a b.md")),
            ),
            // UNC: a backslash share path is absolute; a file URI with a host
            // names the same share on Windows (on Unix it is external)
            (
                r"\\server\share\guide.md",
                true,
                Ok(Some(r"\\server\share\guide.md")),
            ),
            (
                "file://server/share/guide.md",
                true,
                Ok(Some(r"\\server\share\guide.md")),
            ),
            // the scheme boundary holds on Windows too
            ("//evil.com/guide.md", false, Ok(None)),
            ("https://example.com/guide.md", false, Ok(None)),
            ("data:text/plain,x.md", false, Ok(None)),
        ];

        let mut mismatches = Vec::new();
        for (destination, expect_local, expect_resolved) in table {
            let local = is_local_markdown_link(destination);
            let resolved = resolve_local_link_path(destination, dir)
                .map(|path| path.map(|p| p.display().to_string()))
                .map_err(|_| ());
            let expected = expect_resolved.map(|p| p.map(str::to_owned));
            if local != *expect_local || resolved != expected {
                mismatches.push(format!(
                    "{destination:?}: local={local} (want {expect_local}), \
                     resolved={resolved:?} (want {expected:?})"
                ));
            }
        }
        assert!(mismatches.is_empty(), "\n{}", mismatches.join("\n"));
    }

    /// The invariant that closes the dead-click class: whatever the gate lets
    /// through must be something the resolver either turns into a path or
    /// complains about. It must never answer "nothing to open", because such a
    /// destination renders as an internal link and then does nothing at all
    /// when clicked — the state #188 pinned for `bad%FF.md`.
    ///
    /// This replaces #188's `…disagree_on_four_shapes`, which asserted the two
    /// functions *did* disagree and told its reader to drop the row once #141
    /// decided. They now share one `classify_destination`, so the disagreement
    /// is unrepresentable rather than merely fixed.
    #[test]
    #[cfg(unix)]
    fn nothing_the_gate_accepts_can_resolve_to_silence() {
        let dir = Path::new("/base/doc");
        for destination in [
            "guide.md",
            "guide.md?v=2",
            "guide.md#section",
            "a%20b.md",
            "100%25.md",
            "bad%zz.md",
            "bad%FF.md",
            "/abs/guide.md",
            "file:///tmp/guide.md",
            "file://localhost/tmp/guide.md",
            "a%23b.md",
            "",
            "#section",
            "//evil.com/guide.md",
        ] {
            if !is_local_markdown_link(destination) {
                continue;
            }
            match resolve_local_link_path(destination, dir) {
                Ok(Some(_)) | Err(_) => {}
                Ok(None) => panic!(
                    "{destination:?} passes the gate but resolves to nothing: it \
                     would render as an internal link and do nothing on click"
                ),
            }
        }
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
            tab.resolve_link(encoded).unwrap(),
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
        // Build the URI with `Url::from_file_path`, not `format!("file://{}")`:
        // on Windows `canonicalize` returns a verbatim `\\?\C:\…` path and the
        // space stays unencoded, so a pasted string is not a valid file URI
        // there (it failed the first time this test ran on windows-latest).
        let uri = url::Url::from_file_path(guide.canonicalize().unwrap())
            .unwrap()
            .to_string();
        assert_eq!(
            tab.resolve_link(&uri).unwrap(),
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
    fn slugify_heading_matches_github_anchor_conventions() {
        let cases = [
            ("Hello World", "hello-world"),
            ("Settings & Config", "settings--config"),
            ("C++ & Rust?", "c--rust"),
            ("42. The Meta", "42-the-meta"),
            // Each space becomes its own hyphen; existing hyphens are kept.
            ("A -- B_c", "a----b_c"),
            ("Ünïcode Tïtle", "ünïcode-tïtle"),
        ];
        for (title, expected) in cases {
            assert_eq!(
                slugify_heading(title),
                expected,
                "slug of {title:?} must match the GitHub anchor convention"
            );
        }
    }

    #[test]
    fn find_fragment_target_prefers_document_title_then_outline_order() {
        let headers = vec![
            Header {
                level: 2,
                title: "Installation".to_owned(),
                source_start: 10,
                line_number: 4,
            },
            Header {
                level: 2,
                title: "Settings & Config".to_owned(),
                source_start: 90,
                line_number: 20,
            },
        ];

        // Slug semantics: case, punctuation and spacing differences all
        // collapse to the same anchor.
        assert!(matches!(
            find_fragment_target(Some("My Manual"), &headers, "installation"),
            FragmentTarget::Header(_)
        ));
        assert!(matches!(
            find_fragment_target(Some("My Manual"), &headers, "settings--config"),
            FragmentTarget::Header(_)
        ));

        // The first h1 is excluded from the outline but is still the page's
        // primary anchor; it navigates to the top.
        assert!(matches!(
            find_fragment_target(Some("My Manual"), &headers, "my-manual"),
            FragmentTarget::DocumentTop
        ));

        // An unmatched or empty fragment is a no-op, not an error.
        assert!(matches!(
            find_fragment_target(Some("My Manual"), &headers, "no-such-section"),
            FragmentTarget::Missing
        ));
        assert!(matches!(
            find_fragment_target(Some("My Manual"), &headers, "#"),
            FragmentTarget::Missing
        ));
    }

    #[test]
    fn same_document_fragment_scrolls_without_touching_history() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-141-same-doc-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        fs::create_dir_all(&root).unwrap();
        let document = root.join("single.md");
        fs::write(&document, "# Title\n\ntext\n\n## A Section\n\ntext\n").unwrap();

        let mut tab = Tab::new(document.clone()).unwrap();
        let original_path = tab.path.clone();

        assert!(
            tab.navigate_to_link("#a-section").unwrap(),
            "a matching same-document fragment must navigate"
        );
        assert_eq!(
            tab.path, original_path,
            "same-document navigation stays put"
        );
        assert!(tab.history_back.is_empty(), "no history entry for a scroll");
        assert!(
            tab.pending_header_jump.is_some() || tab.pending_scroll_offset.is_some(),
            "the click must schedule the scroll"
        );

        // An unmatched fragment does nothing at all — no navigation, and the
        // scroll scheduled by the earlier match is left alone.
        let key_before = tab.pending_header_jump;
        assert!(!tab.navigate_to_link("#no-such-anchor").unwrap());
        assert_eq!(tab.pending_header_jump, key_before);
        assert_eq!(tab.path, original_path);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_pending_heading_jump_waits_scrolls_or_falls_back() {
        // Recorded: scroll 50 pt above it, never above the top.
        assert_eq!(
            resolve_header_jump(Some(1200.0), Some(100), 900),
            HeaderJumpStep::ScrollTo(1150.0)
        );
        assert_eq!(
            resolve_header_jump(Some(20.0), None, 0),
            HeaderJumpStep::ScrollTo(0.0)
        );
        // Not recorded and not measured that far: keep measuring.
        assert_eq!(
            resolve_header_jump(None, Some(500), 900),
            HeaderJumpStep::Wait
        );
        assert_eq!(
            resolve_header_jump(None, Some(900), 900),
            HeaderJumpStep::Wait
        );
        // Measured past it, or completely, without a position: estimate, so
        // a heading that never records cannot leave the jump pending.
        assert_eq!(
            resolve_header_jump(None, Some(901), 900),
            HeaderJumpStep::Estimate
        );
        assert_eq!(
            resolve_header_jump(None, None, 900),
            HeaderJumpStep::Estimate
        );
    }

    #[test]
    fn a_jump_to_an_unmeasured_heading_measures_toward_it_instead_of_guessing() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-115-jump-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        fs::create_dir_all(&root).unwrap();
        let document = root.join("long.md");
        fs::write(&document, "# Title\n\ntext\n\n## Far Away\n\ntext\n").unwrap();
        let mut tab = Tab::new(document).unwrap();
        // As after a first frame: a measured height exists, but this
        // heading has no recorded position yet.
        tab.last_content_height = 50_000.0;
        let heading = tab
            .outline_headers
            .iter()
            .find(|header| header.title == "Far Away")
            .expect("outline lists the heading")
            .source_start;

        assert!(apply_fragment_navigation(&mut tab, "far-away"));
        assert_eq!(
            tab.pending_scroll_offset, None,
            "jumping to a line-ratio estimate first asks for an offset past the \
             measured part, which lays out the whole document in one frame"
        );
        assert_eq!(
            tab.pending_header_jump.map(|jump| jump.source_start),
            Some(heading)
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn cross_document_fragment_opens_and_lands_on_heading() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-141-cross-doc-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        fs::create_dir_all(&root).unwrap();
        let home = root.join("home.md");
        let guide = root.join("guide.md");
        fs::write(&home, "# Home\n\n[Setup](guide.md#setup)\n").unwrap();
        fs::write(&guide, "# Guide\n\nintro\n\n## Setup\n\nsteps\n").unwrap();

        let mut tab = Tab::new(home.clone()).unwrap();
        assert!(tab.navigate_to_link("guide.md#setup").unwrap());
        assert_eq!(tab.path, guide.canonicalize().unwrap());
        assert!(
            tab.pending_header_jump.is_some(),
            "a fresh document has no cached heading positions, so the \
             corrective pass must be armed to land on the heading"
        );

        // ctrl+click carries the fragment through to the new-tab request.
        let (new_tab, error_message) = handle_local_link_click(&mut tab, "guide.md#setup", true);
        assert!(error_message.is_none());
        let (path, fragment) = new_tab.expect("ctrl+click resolves to a path");
        assert_eq!(path, guide.canonicalize().unwrap());
        assert_eq!(fragment.as_deref(), Some("setup"));

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
    fn missing_explicit_link_surfaces_errors_without_changing_tab_or_history() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-missing-link-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        fs::create_dir_all(&root).unwrap();
        let document = root.join("index.md");
        fs::write(&document, "[Missing](missing.md)").unwrap();

        assert_eq!(
            parse_local_links("[Missing](missing.md)", &document),
            ["missing.md"]
        );

        let mut tab = Tab::new(document.clone()).unwrap();
        let original_path = tab.path.clone();
        let canonicalize_error = root.join("missing.md").canonicalize().unwrap_err();
        let expected_error = format!("Unable to open missing.md: {canonicalize_error}");

        // Ctrl+click resolves before asking the app to create a new tab. This
        // is the same helper whose error result is assigned to the error bar.
        let (new_tab, error_message) = handle_local_link_click(&mut tab, "missing.md", true);
        assert!(new_tab.is_none());
        assert_eq!(error_message, Some(expected_error.clone()));
        assert_eq!(tab.path, original_path);
        assert!(tab.history_back.is_empty());
        assert!(tab.history_forward.is_empty());

        // Plain click goes through the same production result handler and
        // leaves the current tab intact.
        let (new_tab, error_message) = handle_local_link_click(&mut tab, "missing.md", false);
        assert!(new_tab.is_none());
        assert_eq!(error_message, Some(expected_error));
        assert_eq!(tab.path, original_path);
        assert!(tab.history_back.is_empty());
        assert!(tab.history_forward.is_empty());

        fs::remove_dir_all(root).unwrap();
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
    fn only_file_uris_are_classified_as_local_links() {
        let document_dir = Path::new("/tmp/docs");
        for destination in [
            "data:text/plain,x.md",
            "javascript:void(0)/x.md",
            "unknownscheme://host/a.md",
            "mailto:guide.md",
        ] {
            assert!(!is_local_markdown_link(destination), "{destination}");
            assert_eq!(
                resolve_local_link_path(destination, document_dir).unwrap(),
                None,
                "{destination}"
            );
        }

        assert!(is_local_markdown_link("guide.md"));
        assert!(is_local_markdown_link("docs%20with%20spaces/guide.md"));
        assert_eq!(
            resolve_local_link_path("100%done.md", document_dir).unwrap(),
            Some(document_dir.join("100%done.md"))
        );

        let file_path = std::env::temp_dir().join("md-viewer-link-scheme-guide.md");
        let file_uri = url::Url::from_file_path(&file_path).unwrap().to_string();
        assert!(is_local_markdown_link(&file_uri));
        assert_eq!(
            resolve_local_link_path(&file_uri, document_dir).unwrap(),
            Some(file_path)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_drive_prefixes_are_not_treated_as_uri_schemes() {
        assert_eq!(explicit_uri_scheme(r"C:\docs\guide.md"), None);
        assert_eq!(explicit_uri_scheme("D:/docs/guide.md"), None);
        assert!(is_local_markdown_link(r"C:\docs\guide.md"));
        assert!(is_local_markdown_link("D:/docs/guide.md"));
    }

    #[test]
    fn explicit_non_file_schemes_are_not_registered_as_link_hooks() {
        let document = Path::new("/tmp/index.md");
        let content = concat!(
            "[data](data:text/plain,x.md) ",
            "[script](javascript:void(0)/x.md) ",
            "[unknown](unknownscheme://host/a.md)",
        );

        assert!(parse_local_links(content, document).is_empty());
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
                .unwrap()
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
    fn expand_all_leaves_directories_beyond_the_budget_unloaded() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-wide-explorer-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        let limit = FileExplorer::BULK_EXPANSION_LIMIT;
        for i in 0..=limit {
            fs::create_dir_all(root.join(format!("dir-{i:04}"))).unwrap();
        }
        let mut explorer = FileExplorer {
            root: Some(root.clone()),
            ..Default::default()
        };

        assert!(explorer.expand_all());
        assert!(explorer.expanded_dirs.is_empty());
        let deadline = Instant::now() + Duration::from_secs(5);
        let truncated = loop {
            if let Some(truncated) = explorer.poll_pending_scan() {
                break truncated;
            }
            assert!(
                Instant::now() < deadline,
                "background expansion did not complete"
            );
            std::thread::sleep(Duration::from_millis(1));
        };
        let last_path = root.join(format!("dir-{limit:04}"));
        let beyond_budget = explorer.get_children(&last_path).is_none();
        let bulk_count = explorer.expanded_dirs.len();
        explorer.toggle_expanded(&last_path);
        explorer
            .expanded_dirs
            .insert(root.with_extension("old-root"));
        let (watches, _) = explorer.watch_directories();
        fs::remove_dir_all(root).unwrap();

        assert!(truncated);
        assert_eq!(bulk_count, limit);
        assert!(beyond_budget);
        assert!(explorer.is_expanded(&last_path));
        assert!(explorer.get_children(&last_path).is_some());
        assert_eq!(watches.len(), limit + 1);
        assert!(!watches.contains(&last_path));
    }

    #[test]
    fn refresh_keeps_a_pending_expand_all() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-refresh-expand-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        for i in 0..20 {
            fs::create_dir_all(root.join(format!("dir-{i:02}"))).unwrap();
        }
        let mut explorer = FileExplorer {
            root: Some(root.clone()),
            ..Default::default()
        };

        assert!(explorer.expand_all());
        // Refresh while the Expand All scan may still be running.
        explorer.refresh();
        let deadline = Instant::now() + Duration::from_secs(5);
        while explorer.pending_scan.is_some() {
            explorer.poll_pending_scan();
            assert!(Instant::now() < deadline, "scan did not complete");
            std::thread::sleep(Duration::from_millis(1));
        }
        let expanded = explorer.expanded_dirs.len();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(expanded, 20, "Refresh dropped the pending Expand All");
    }

    #[test]
    fn watch_budget_skips_missing_folders_before_cutting() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-watch-missing-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        let limit = FileExplorer::BULK_EXPANSION_LIMIT;
        let mut explorer = FileExplorer {
            root: Some(root.clone()),
            ..Default::default()
        };
        for i in 0..limit + 4 {
            let dir = root.join(format!("z-dir-{i:04}"));
            fs::create_dir_all(&dir).unwrap();
            explorer.expanded_dirs.insert(dir);
        }
        // Deleted folders that sort before the existing ones.
        for i in 0..limit {
            explorer
                .expanded_dirs
                .insert(root.join(format!("a-gone-{i:04}")));
        }

        let (watches, cut) = explorer.watch_directories();
        let missing = watches.iter().filter(|path| !path.is_dir()).count();
        fs::remove_dir_all(&root).unwrap();

        assert!(cut);
        assert_eq!(missing, 0);
        assert_eq!(watches.len(), limit + 1, "root plus a full budget");
    }

    #[test]
    fn visible_rows_follow_the_expanded_tree_in_display_order() {
        let dir = |path: &str, children: Option<Vec<FileTreeNode>>| FileTreeNode::Directory {
            path: PathBuf::from(path),
            name: path.rsplit('/').next().unwrap().to_owned(),
            modified: None,
            children,
        };
        let file = |path: &str| FileTreeNode::File {
            path: PathBuf::from(path),
            name: path.rsplit('/').next().unwrap().to_owned(),
            modified: None,
        };
        let tree = vec![
            dir(
                "r/a",
                Some(vec![
                    dir("r/a/deep", Some(vec![file("r/a/deep/x.md")])),
                    file("r/a/y.md"),
                ]),
            ),
            // Collapsed: its loaded children are not listed.
            dir("r/b", Some(vec![file("r/b/hidden.md")])),
            // Expanded but not loaded yet: listed, children to load.
            dir("r/c", None),
            file("r/top.md"),
        ];
        let expanded: HashSet<PathBuf> = ["r/a", "r/a/deep", "r/c"]
            .into_iter()
            .map(PathBuf::from)
            .collect();

        let mut rows = Vec::new();
        let mut unloaded = Vec::new();
        FileExplorer::visible_rows(&tree, &expanded, 0, &mut rows, &mut unloaded);
        let listed: Vec<(usize, &str)> = rows
            .iter()
            .map(|(depth, node)| (*depth, node.name()))
            .collect();

        assert_eq!(
            listed,
            [
                (0, "a"),
                (1, "deep"),
                (2, "x.md"),
                (1, "y.md"),
                (0, "b"),
                (0, "c"),
                (0, "top.md")
            ]
        );
        assert_eq!(unloaded, [PathBuf::from("r/c")]);
    }

    #[test]
    fn saved_expansion_is_bounded_and_order_independent() {
        let limit = FileExplorer::BULK_EXPANSION_LIMIT;
        let parent = PathBuf::from("root").join("z-parent");
        let child = parent.join("child");
        let mut paths = (0..limit)
            .map(|i| {
                PathBuf::from("root")
                    .join("a-parent")
                    .join(format!("dir-{i:04}"))
            })
            .collect::<Vec<_>>();
        paths.extend([parent.clone(), child.clone()]);
        let mut explorer = FileExplorer::default();

        assert!(explorer.restore_expansion(paths.clone(), None));
        assert_eq!(explorer.expanded_dirs.len(), limit);
        assert!(explorer.is_expanded(&parent));
        let first = explorer.expanded_dirs.clone();
        paths.reverse();
        assert!(explorer.restore_expansion(paths, None));
        assert_eq!(explorer.expanded_dirs, first);

        assert!(!explorer.restore_expansion(vec![child.clone(), parent.clone()], None));
        assert_eq!(explorer.expanded_dirs, HashSet::from([parent, child]));
    }

    #[test]
    fn saved_expansion_keeps_the_current_root_when_it_is_cut() {
        let limit = FileExplorer::BULK_EXPANSION_LIMIT;
        // A root opened earlier, at the same depth and sorting first: without
        // a preference its paths take every slot.
        let old_root = PathBuf::from("a-old-root");
        let current = PathBuf::from("b-current-root");
        let mut paths: Vec<_> = (0..limit)
            .map(|i| old_root.join(format!("o{i:04}")))
            .collect();
        paths.extend((0..limit).map(|i| current.join(format!("d{i:04}"))));
        let mut explorer = FileExplorer::default();

        assert!(explorer.restore_expansion(paths, Some(&current)));
        assert_eq!(explorer.expanded_dirs.len(), limit);
        let outside: Vec<_> = explorer
            .expanded_dirs
            .iter()
            .filter(|path| !path.starts_with(&current))
            .collect();
        assert!(
            outside.is_empty(),
            "kept paths outside the root: {outside:?}"
        );

        // Under the budget nothing is dropped, so returning to the earlier
        // root still finds its folders open.
        let kept = vec![old_root.join("o0000"), current.join("d0000")];
        assert!(!explorer.restore_expansion(kept.clone(), Some(&current)));
        assert_eq!(explorer.expanded_dirs, HashSet::from_iter(kept));
    }

    #[test]
    fn expand_all_stops_before_the_explorer_lists_too_many_rows() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-rows-explorer-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        // Few enough folders for the directory budget, but each lists 30
        // entries, so expanding them all would list far more rows.
        let (folders, entries) = (100, 30);
        for i in 0..folders {
            for j in 0..entries {
                fs::create_dir_all(root.join(format!("d{i:03}")).join(format!("e{j:02}"))).unwrap();
            }
        }
        let mut explorer = FileExplorer {
            root: Some(root.clone()),
            ..Default::default()
        };

        assert!(explorer.expand_all());
        let deadline = Instant::now() + Duration::from_secs(5);
        let truncated = loop {
            if let Some(truncated) = explorer.poll_pending_scan() {
                break truncated;
            }
            assert!(
                Instant::now() < deadline,
                "background expansion did not complete"
            );
            std::thread::sleep(Duration::from_millis(1));
        };
        let expanded = explorer.expanded_dirs.clone();
        fs::remove_dir_all(&root).unwrap();

        // The top level is always listed; each expanded folder adds its entries.
        let fit = (FileExplorer::BULK_ROW_LIMIT - folders) / entries;
        assert!(truncated);
        assert_eq!(expanded.len(), fit);
        let rows = folders + expanded.len() * entries;
        assert!(rows <= FileExplorer::BULK_ROW_LIMIT, "{rows} rows listed");
        // Still level by level and in order: the first `fit` folders.
        for i in 0..fit {
            assert!(expanded.contains(&root.join(format!("d{i:03}"))));
        }
    }

    #[test]
    fn expand_all_opens_shallow_directories_before_deep_ones() {
        let root = std::env::temp_dir().join(format!(
            "md-viewer-deep-explorer-{}-{}",
            std::process::id(),
            now_epoch_secs()
        ));
        // The first directory in name order holds more directories than the
        // budget; its siblings each hold one.
        for i in 0..30 {
            for j in 0..10 {
                fs::create_dir_all(
                    root.join("a")
                        .join(format!("x{i:02}"))
                        .join(format!("y{j:02}")),
                )
                .unwrap();
            }
        }
        let siblings: Vec<PathBuf> = ('b'..='z').map(|c| root.join(c.to_string())).collect();
        for sibling in &siblings {
            fs::create_dir_all(sibling.join("sub")).unwrap();
        }
        let mut explorer = FileExplorer {
            root: Some(root.clone()),
            ..Default::default()
        };

        assert!(explorer.expand_all());
        let deadline = Instant::now() + Duration::from_secs(5);
        let truncated = loop {
            if let Some(truncated) = explorer.poll_pending_scan() {
                break truncated;
            }
            assert!(
                Instant::now() < deadline,
                "background expansion did not complete"
            );
            std::thread::sleep(Duration::from_millis(1));
        };
        let expanded = explorer.expanded_dirs.clone();
        fs::remove_dir_all(&root).unwrap();

        assert!(truncated);
        assert_eq!(expanded.len(), FileExplorer::BULK_EXPANSION_LIMIT);
        let collapsed: Vec<_> = siblings
            .iter()
            .flat_map(|sibling| [sibling.clone(), sibling.join("sub")])
            .chain((0..30).map(|i| root.join("a").join(format!("x{i:02}"))))
            .filter(|path| !expanded.contains(path))
            .collect();
        assert!(
            collapsed.is_empty(),
            "shallow directories left collapsed: {collapsed:?}"
        );
    }

    #[test]
    fn collapse_all_discards_a_pending_bulk_expansion() {
        let root = PathBuf::from("root");
        let expanded = HashSet::from([root.join("docs")]);
        let (tx, rx) = mpsc::channel();
        tx.send(ExplorerScanResult {
            root: root.clone(),
            sort_order: SortOrder::NameAsc,
            tree: Vec::new(),
            bulk_expansion: Some((expanded.clone(), false)),
        })
        .unwrap();
        let mut explorer = FileExplorer {
            root: Some(root),
            expanded_dirs: expanded,
            pending_scan: Some(PendingExplorerScan {
                receiver: rx,
                expands_all: true,
                discarded: false,
            }),
            ..Default::default()
        };

        explorer.collapse_all();

        assert_eq!(explorer.poll_pending_scan(), None);
        assert!(explorer.pending_scan.is_none());
        assert!(explorer.expanded_dirs.is_empty());
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
            bulk_expansion: None,
        })
        .unwrap();
        let mut explorer = FileExplorer {
            root: Some(root.clone()),
            expanded_dirs: HashSet::from([expanded.clone()]),
            pending_scan: Some(PendingExplorerScan {
                receiver: rx,
                expands_all: false,
                discarded: false,
            }),
            ..Default::default()
        };

        assert_eq!(explorer.poll_pending_scan(), Some(false));
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
    fn tab_bar_shows_unless_hidden_and_always_while_editing() {
        assert!(tab_bar_visible(true, false));
        assert!(tab_bar_visible(true, true));
        assert!(!tab_bar_visible(false, false));
        assert!(tab_bar_visible(false, true));
    }

    #[test]
    fn hidden_tab_bar_survives_a_persistence_roundtrip() {
        let mut storage = MemoryStorage(HashMap::new());
        let app_state = PersistedState {
            show_tab_bar: Some(false),
            ..PersistedState::default()
        };
        eframe::set_value(&mut storage, APP_KEY, &app_state);
        let restored: PersistedState = eframe::get_value(&storage, APP_KEY).expect("state decodes");
        assert_eq!(restored.show_tab_bar, Some(false));

        // A state file written before #208 has no field; the bar stays visible.
        let mut old_storage = MemoryStorage(HashMap::new());
        eframe::Storage::set_string(&mut old_storage, APP_KEY, "()".to_owned());
        let old: PersistedState =
            eframe::get_value(&old_storage, APP_KEY).expect("pre-#208 state decodes");
        assert_eq!(old.show_tab_bar, None);
        assert!(old.show_tab_bar.unwrap_or(true));
    }

    #[test]
    fn persisted_sidebar_widths_are_sanitized() {
        assert_eq!(restored_sidebar_width(Some(320.0), 200.0), 320.0);
        assert_eq!(restored_sidebar_width(Some(20.0), 200.0), SIDEBAR_MIN_WIDTH);
        assert_eq!(restored_sidebar_width(Some(f32::NAN), 200.0), 200.0);
        assert_eq!(restored_sidebar_width(Some(f32::INFINITY), 200.0), 200.0);
        assert_eq!(restored_sidebar_width(None, 200.0), 200.0);
    }

    #[test]
    fn edge_autoscroll_speed_is_zero_inside_and_signed_at_the_edges() {
        let (top, bottom) = (100.0, 700.0);
        assert_eq!(edge_autoscroll_speed(400.0, top, bottom), 0.0);
        assert_eq!(
            edge_autoscroll_speed(top + EDGE_AUTOSCROLL_ZONE + 1.0, top, bottom),
            0.0
        );
        let near_bottom = edge_autoscroll_speed(bottom - 1.0, top, bottom);
        let past_bottom = edge_autoscroll_speed(bottom + 100.0, top, bottom);
        assert!(near_bottom > 0.0, "inside the bottom band scrolls down");
        assert!(past_bottom > near_bottom, "speed grows past the edge");
        assert!(
            edge_autoscroll_speed(top - 50.0, top, bottom) < 0.0,
            "above scrolls up"
        );
        assert_eq!(
            edge_autoscroll_speed(bottom + 1.0e6, top, bottom),
            3000.0,
            "capped"
        );
        // A very short area keeps a usable middle instead of being all edge.
        assert_eq!(edge_autoscroll_speed(110.0, 100.0, 120.0), 0.0);
    }

    /// Drives one frame of an editor laid out like md-viewer's: a vertical
    /// ScrollArea holding a multiline TextEdit, followed by the focus keeper.
    fn editor_frame(
        ctx: &egui::Context,
        text: &mut String,
        focus: &mut EditorFocus,
        events: Vec<egui::Event>,
    ) -> (egui::Id, egui::Rect) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let mut ids = None;
        let mut out = ctx.run_pass(input, |ctx| {
            ctx.root_ui(|root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let out = egui::ScrollArea::vertical()
                        .auto_shrink(false)
                        .show(ui, |ui| ui.add(egui::TextEdit::multiline(text)));
                    focus.keep_through_scrollbar_press(ui, out.inner.id, out.inner_rect);
                    ids = Some((out.inner.id, out.inner_rect));
                });
            });
        });
        out.textures_delta.clear();
        ids.expect("editor shown")
    }

    #[test]
    fn a_click_on_the_editor_scrollbar_keeps_focus_and_selection() {
        let ctx = egui::Context::default();
        let mut text: String = (0..300).map(|i| format!("line {i:03}\n")).collect();
        let mut focus = EditorFocus::default();
        let (id, inner) = editor_frame(&ctx, &mut text, &mut focus, vec![]);

        let selection = egui::text::CCursorRange::two(
            egui::text::CCursor::new(9),
            egui::text::CCursor::new(45),
        );
        ctx.memory_mut(|m| m.request_focus(id));
        let mut state = egui::TextEdit::load_state(&ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(selection));
        state.store(&ctx, id);
        editor_frame(&ctx, &mut text, &mut focus, vec![]);
        editor_frame(&ctx, &mut text, &mut focus, vec![]);
        assert!(ctx.memory(|m| m.has_focus(id)), "setup: editor focused");

        // A quick click on the scrollbar track: press and release in one frame.
        let pos = egui::pos2(inner.right() - 3.0, inner.bottom() - 40.0);
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        editor_frame(
            &ctx,
            &mut text,
            &mut focus,
            vec![egui::Event::PointerMoved(pos), button(true), button(false)],
        );
        editor_frame(&ctx, &mut text, &mut focus, vec![]);
        editor_frame(&ctx, &mut text, &mut focus, vec![]);

        assert!(
            ctx.memory(|m| m.has_focus(id)),
            "editor lost focus to its scrollbar"
        );
        let kept = egui::TextEdit::load_state(&ctx, id).and_then(|s| s.cursor.char_range());
        assert_eq!(kept, Some(selection), "selection collapsed");
    }

    #[test]
    fn a_click_outside_the_scrollbar_still_takes_the_focus() {
        let ctx = egui::Context::default();
        let mut text: String = (0..300).map(|i| format!("line {i:03}\n")).collect();
        let mut focus = EditorFocus::default();
        let (id, _inner) = editor_frame(&ctx, &mut text, &mut focus, vec![]);
        ctx.memory_mut(|m| m.request_focus(id));
        editor_frame(&ctx, &mut text, &mut focus, vec![]);
        editor_frame(&ctx, &mut text, &mut focus, vec![]);
        assert!(ctx.memory(|m| m.has_focus(id)), "setup: editor focused");
        // The editor fills the panel, so press on the panel's margin, outside
        // both the TextEdit and the scrollbar strip.
        let pos = egui::pos2(2.0, 2.0);
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        editor_frame(
            &ctx,
            &mut text,
            &mut focus,
            vec![egui::Event::PointerMoved(pos), button(true), button(false)],
        );
        editor_frame(&ctx, &mut text, &mut focus, vec![]);
        assert!(
            !ctx.memory(|m| m.has_focus(id)),
            "a click elsewhere must still unfocus"
        );
    }

    /// #266: on compositors without server-side decorations (GNOME/mutter on
    /// Wayland, weston) winit draws the title bar itself. Without its
    /// `wayland-csd-adwaita` feature that is sctk's fallback frame, which
    /// ignores double-clicks, so the title bar could not maximize the window.
    /// eframe turns the feature on only through its default features, which
    /// md-viewer disables; `Cargo.toml` asks winit for it directly.
    #[test]
    fn wayland_title_bar_uses_the_adwaita_frame() {
        let lock = include_str!("../Cargo.lock");
        assert!(
            lock.lines().any(|l| l.trim() == "name = \"sctk-adwaita\""),
            "sctk-adwaita is not in the dependency graph: winit falls back to a \
             title bar that cannot maximize on double-click (#266)"
        );
    }

    #[test]
    fn the_font_family_scan_does_not_start_at_startup() {
        let list = FontFamilyList::default();
        assert!(!list.requested, "a new app must not have started the scan");
        assert!(!list.scanning());
    }

    #[test]
    fn the_font_family_scan_starts_once_and_delivers() {
        let spawns = std::cell::Cell::new(0);
        let (tx, rx) = mpsc::channel();
        let mut rx = Some(rx);
        let mut list = FontFamilyList::default();
        list.request(|| {
            spawns.set(spawns.get() + 1);
            rx.take()
        });
        assert!(list.scanning());
        assert!(!list.poll(), "nothing sent yet");

        tx.send(vec!["Noto Sans".to_owned(), "DejaVu Serif".to_owned()])
            .unwrap();
        assert!(list.poll(), "the list arrived");
        assert!(!list.scanning());
        assert_eq!(list.names, ["Noto Sans", "DejaVu Serif"]);
        assert_eq!(list.names_lower, ["noto sans", "dejavu serif"]);

        // Reopening the dialog must not scan again.
        list.request(|| {
            spawns.set(spawns.get() + 1);
            None
        });
        assert_eq!(spawns.get(), 1, "the scan ran more than once");
        assert_eq!(list.names.len(), 2);
    }

    #[test]
    fn a_scan_that_ends_without_a_result_stops_scanning() {
        let mut list = FontFamilyList::default();
        list.request(|| {
            let (tx, rx) = mpsc::channel::<Vec<String>>();
            drop(tx);
            Some(rx)
        });
        assert!(!list.poll());
        assert!(!list.scanning(), "a dead scan must not spin forever");
        assert!(list.names.is_empty());
    }

    /// `is_local_markdown_link` skips classification for a destination
    /// without a `.`; that shortcut must never change an answer.
    #[test]
    fn the_dot_shortcut_never_rejects_a_markdown_destination() {
        let mut corpus: Vec<String> = [
            "guide.md",
            "GUIDE.MD",
            "a/b.markdown",
            "notes.txt",
            "file:///tmp/x.md",
            "file:///tmp/%2Emd",
            "bad%FF.md",
            "guide.md?v=2#s",
            "#anchor",
            "md",
            "%2Emd",
            "dir/",
            "",
            "?q=1",
            "a#b.md",
            "https://e.com/x.md",
            "//host/x.md",
            "C:\\docs\\guide.md",
            "readme",
            "\u{212A}.md",
            "x.m\u{212A}d",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        for path in ["README.md", "CHANGELOG.md", "docs/LESSONS.md"] {
            if let Ok(text) =
                std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/").to_owned() + path)
            {
                corpus.extend(text.split_whitespace().map(str::to_owned));
            }
        }
        assert!(
            corpus.len() > 1000,
            "the corpus did not load: {}",
            corpus.len()
        );
        for destination in &corpus {
            if classified_as_local_markdown(destination) {
                assert!(
                    destination.contains('.'),
                    "{destination:?} names a Markdown file without a dot"
                );
            }
            assert_eq!(
                is_local_markdown_link(destination),
                classified_as_local_markdown(destination),
                "{destination:?}"
            );
        }
    }

    /// `parse_document` gives exactly what the two separate passes give.
    #[test]
    fn one_document_parse_matches_the_separate_outline_and_link_passes() {
        let dir = std::env::temp_dir().join(format!("mdv-parse-doc-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("docs/guide.md"), "# Guide\n").unwrap();
        std::fs::write(dir.join("notes.md"), "# Notes\n").unwrap();
        let document = dir.join("doc.md");
        let fixture = "# Title\n\nSee docs/guide.md and [notes](notes.md), [x](#title).\n\n\
                       ## Dup\n\n## Dup\n\n```\ndocs/guide.md in code\n```\n\n\
                       ### `code` heading with $x^2$ and a footnote[^1]\n\n[^1]: note\n\n\
                       Inline `notes.md` and https://example.com/a.md\n";
        let readme = include_str!("../README.md");
        for content in [fixture, readme] {
            let (headers, links) = parse_document(content, &document);
            let separate = parse_headers(content);
            assert_eq!(headers.document_title, separate.document_title);
            let key = |h: &Header| (h.level, h.title.clone(), h.source_start, h.line_number);
            assert_eq!(
                headers.outline_headers.iter().map(key).collect::<Vec<_>>(),
                separate.outline_headers.iter().map(key).collect::<Vec<_>>()
            );
            assert_eq!(links, parse_local_links(content, &document));
        }
        let (headers, links) = parse_document(fixture, &document);
        assert_eq!(
            headers.outline_headers.len(),
            4,
            "{:?}",
            headers
                .outline_headers
                .iter()
                .map(|h| &h.title)
                .collect::<Vec<_>>()
        );
        assert!(
            links.contains(&"docs/guide.md".to_owned()) && links.contains(&"notes.md".to_owned()),
            "{links:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
