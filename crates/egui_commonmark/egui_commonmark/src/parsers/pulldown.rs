use std::iter::Peekable;
use std::ops::Range;

use crate::{CommonMarkCache, CommonMarkOptions};

use egui::{self, Id, Pos2, TextStyle, Ui};

use crate::List;
use egui_commonmark_backend_extended::elements::{
    blockquote, footnote, footnote_start, heading_end_spacing, heading_start_spacing, newline,
    paragraph_end_spacing, rule, soft_break, ImmutableCheckbox,
};
use egui_commonmark_backend_extended::misc::*;
use egui_commonmark_backend_extended::pulldown::*;
use pulldown_cmark::{CowStr, HeadingLevel};

/// Search-match highlight kind for a single rendered text segment.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum HighlightKind {
    None,
    Match,
    Active,
}

impl HighlightKind {
    fn background_color(self, ui: &Ui) -> Option<egui::Color32> {
        let dark = ui.style().visuals.dark_mode;
        match self {
            HighlightKind::None => None,
            HighlightKind::Match => Some(if dark {
                egui::Color32::from_rgb(102, 92, 46)
            } else {
                egui::Color32::from_rgb(255, 229, 127)
            }),
            HighlightKind::Active => Some(if dark {
                egui::Color32::from_rgb(156, 107, 26)
            } else {
                egui::Color32::from_rgb(255, 167, 38)
            }),
        }
    }
}

/// Split `text` (covering `span` in the source) into segments tagged with the
/// highlight kind that applies to each. Returns at least one segment.
///
/// Assumes `text.len() == span.len()` — caller is responsible for that check
/// (markdown escapes or smart-punct transforms can break it).
fn compute_highlight_segments(
    text: &str,
    span: &Range<usize>,
    ranges: &[Range<usize>],
    active: Option<&Range<usize>>,
) -> Vec<(String, HighlightKind)> {
    let mut overlaps: Vec<(usize, usize, HighlightKind)> = Vec::new();
    for r in ranges {
        let start = r.start.max(span.start);
        let end = r.end.min(span.end);
        if start >= end {
            continue;
        }
        let local_start = start - span.start;
        let local_end = end - span.start;
        let is_active = active.map(|a| a == r).unwrap_or(false);
        let kind = if is_active {
            HighlightKind::Active
        } else {
            HighlightKind::Match
        };
        overlaps.push((local_start, local_end, kind));
    }

    if overlaps.is_empty() {
        return vec![(text.to_string(), HighlightKind::None)];
    }

    let mut segments = Vec::new();
    let mut cursor = 0;
    for (start, end, kind) in overlaps {
        if start > cursor
            && text.is_char_boundary(cursor)
            && text.is_char_boundary(start)
        {
            segments.push((text[cursor..start].to_string(), HighlightKind::None));
        }
        if text.is_char_boundary(start) && text.is_char_boundary(end) {
            segments.push((text[start..end].to_string(), kind));
        }
        cursor = end;
    }
    if cursor < text.len() && text.is_char_boundary(cursor) {
        segments.push((text[cursor..].to_string(), HighlightKind::None));
    }

    segments
}

/// Split a long inline-code token into fixed-size chunks so the row-wrap layout
/// can put each chunk on its own row instead of overflowing the content width.
/// Short tokens (<= MAX) pass through unchanged.
///
/// Blind char-count cut (not break-friendly on `/`, `-`, etc.): variable-length
/// segments can still exceed the column at narrow widths and re-introduce the
/// original clipping bug. Fixed-size chunks always fit.
fn inline_code_wrap_segments(text: &str) -> Vec<String> {
    const MAX_SEGMENT_CHARS: usize = 56;

    if text.chars().count() <= MAX_SEGMENT_CHARS {
        return vec![text.to_owned()];
    }

    let mut segments = Vec::new();
    let mut current = String::with_capacity(MAX_SEGMENT_CHARS * 4);
    let mut current_len = 0;

    for ch in text.chars() {
        current.push(ch);
        current_len += 1;
        if current_len >= MAX_SEGMENT_CHARS {
            segments.push(std::mem::take(&mut current));
            current_len = 0;
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
}

/// Count the visual lines a markdown table cell will occupy when rendered.
/// Used by the `fn table` renderer to compute heterogeneous row heights so
/// long inline-code paths (chunked by `inline_code_wrap_segments`) don't get
/// clipped by a fixed row height. Only `Event::Code` chunking adds visual
/// lines today; other inline events flow on a single line within a cell.
fn cell_visual_lines(cell: &[(pulldown_cmark::Event, Range<usize>)]) -> usize {
    let mut max_lines = 1usize;
    for (event, _) in cell {
        if let pulldown_cmark::Event::Code(text) = event {
            let chunks = inline_code_wrap_segments(text).len();
            if chunks > max_lines {
                max_lines = chunks;
            }
        }
    }
    max_lines
}

/// Heuristic visual-line count for an HTML-table cell (rendered as a plain
/// `RichText` string, not as a markdown event stream). Counts explicit
/// newlines and adds a crude wrap estimate of ~60 chars per visual line.
/// Over-estimates slightly by design — extra row height is preferable to
/// clipping. Exact estimation would require knowing the rendered column
/// width up front, which TableBuilder doesn't expose before render.
fn html_cell_visual_lines(cell: &str) -> usize {
    let explicit_lines = cell.lines().count().max(1);
    let wrap_est = cell.len().saturating_sub(1) / 60;
    explicit_lines.saturating_add(wrap_est).max(1)
}

/// After a nested horizontal `ScrollArea` has rendered (used here to wrap tables
/// that overflow the content column), redirect any pending vertical wheel delta
/// into the area's horizontal offset when the cursor is hovered over it.
///
/// The outer document is wrapped in a `ScrollArea::vertical()`, so plain wheel
/// over a wide table would otherwise scroll the page instead of the table —
/// users were forced to grab the table's bottom scrollbar to see overflowing
/// columns. With this helper, wheel-over-table scrolls the table sideways.
///
/// Edge pass-through: when the table is fully scrolled to either side and the
/// wheel direction would push past the edge, the delta is left untouched so the
/// outer area can keep scrolling the page.
///
/// X-delta (native trackpad horizontal swipe) is already consumed by the inner
/// `ScrollArea::horizontal()` inside its own `.show()` call — only Y is touched
/// here, never X.
fn forward_wheel_to_horizontal_scroll<R>(
    ui: &Ui,
    out: &mut egui::containers::scroll_area::ScrollAreaOutput<R>,
) {
    if !ui.rect_contains_pointer(out.inner_rect) {
        return;
    }
    let dy = ui.ctx().input(|i| i.smooth_scroll_delta.y);
    if dy.abs() < 0.1 {
        return;
    }
    let max_x = (out.content_size.x - out.inner_rect.width()).max(0.0);
    if max_x <= 0.0 {
        return;
    }
    let at_left = out.state.offset.x <= 0.0 && dy > 0.0;
    let at_right = out.state.offset.x >= max_x && dy < 0.0;
    if at_left || at_right {
        return;
    }
    let new_x = (out.state.offset.x - dy).clamp(0.0, max_x);
    if (new_x - out.state.offset.x).abs() > f32::EPSILON {
        out.state.offset.x = new_x;
        out.state.store(ui.ctx(), out.id);
        ui.ctx().input_mut(|i| i.smooth_scroll_delta.y = 0.0);
        ui.ctx().request_repaint();
    }
}

/// Newline logic is constructed by the following:
/// All elements try to insert a newline before them (if they are allowed)
/// and end their own line.
struct Newline {
    /// Whether a newline should not be inserted before a widget. This is only for
    /// the first widget.
    should_not_start_newline_forced: bool,
    /// Whether an element should insert a newline before it
    should_start_newline: bool,
    /// Whether an element should end it's own line using a newline
    /// This will have to be set to false in cases such as when blocks are within
    /// a list.
    should_end_newline: bool,
    /// only false when the widget is the last one.
    should_end_newline_forced: bool,
}

impl Default for Newline {
    fn default() -> Self {
        Self {
            should_not_start_newline_forced: true,
            should_start_newline: true,
            should_end_newline: true,
            should_end_newline_forced: true,
        }
    }
}

impl Newline {
    pub fn can_insert_end(&self) -> bool {
        self.should_end_newline && self.should_end_newline_forced
    }

    pub fn can_insert_start(&self) -> bool {
        self.should_start_newline && !self.should_not_start_newline_forced
    }

    pub fn try_insert_start(&self, ui: &mut Ui) {
        if self.can_insert_start() {
            newline(ui);
        }
    }

    pub fn try_insert_end(&self, ui: &mut Ui) {
        if self.can_insert_end() {
            newline(ui);
        }
    }
}

#[derive(Default)]
struct DefinitionList {
    is_first_item: bool,
    is_def_list_def: bool,
}

pub struct CommonMarkViewerInternal {
    curr_table: usize,
    curr_code_block: usize,
    text_style: Style,
    list: List,
    link: Option<Link>,
    image: Option<Image>,
    line: Newline,
    code_block: Option<CodeBlock>,

    /// Only populated if the html_fn option has been set
    html_block: String,
    is_list_item: bool,
    def_list: DefinitionList,
    is_table: bool,
    is_blockquote: bool,
    checkbox_events: Vec<CheckboxClickEvent>,

    /// Track current heading for position recording
    current_heading_y: Option<f32>,
    current_heading_text: String,
    /// Accumulate heading RichText fragments for single render at end
    current_heading_rich_texts: Vec<egui::RichText>,
    /// Per-render-pass counter: number of headings seen so far with each
    /// normalized title. Used to build composite cache keys that
    /// disambiguate duplicate-titled headers (e.g. multiple `## Installation`).
    /// Reset at the start of each `show*` call so the count restarts at 0.
    heading_occurrence_counts: std::collections::HashMap<String, usize>,
}

pub(crate) struct CheckboxClickEvent {
    pub(crate) checked: bool,
    pub(crate) span: Range<usize>,
}

impl CommonMarkViewerInternal {
    pub fn new() -> Self {
        Self {
            curr_table: 0,
            curr_code_block: 0,
            text_style: Style::default(),
            list: List::default(),
            link: None,
            image: None,
            line: Newline::default(),
            is_list_item: false,
            def_list: Default::default(),
            code_block: None,
            html_block: String::new(),
            is_table: false,
            is_blockquote: false,
            checkbox_events: Vec::new(),
            current_heading_y: None,
            current_heading_text: String::new(),
            current_heading_rich_texts: Vec::new(),
            heading_occurrence_counts: std::collections::HashMap::new(),
        }
    }
}

fn parser_options_math(is_math_enabled: bool) -> pulldown_cmark::Options {
    if is_math_enabled {
        parser_options() | pulldown_cmark::Options::ENABLE_MATH
    } else {
        parser_options()
    }
}

/// Hash the layout-affecting render context.
///
/// `split_points` cache y-positions, which become invalid when anything that
/// affects layout changes. The previous code (parsers/pulldown.rs invalidation
/// block) only watched `available_size`, so zooming (Ctrl++/-) or toggling
/// dark mode would leave stale split_points in place and the viewport-skip
/// math would render the wrong content range.
fn compute_layout_signature(ui: &egui::Ui, options: &CommonMarkOptions) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    // Width drives wrap and is the dominant layout input.
    ui.available_width().to_bits().hash(&mut h);
    // Body text height already encodes egui's zoom factor and any explicit
    // font-size override, so we don't need to read pixels_per_point separately.
    ui.text_style_height(&egui::TextStyle::Body)
        .to_bits()
        .hash(&mut h);
    ui.text_style_height(&egui::TextStyle::Monospace)
        .to_bits()
        .hash(&mut h);
    // Theme doesn't change widget heights, but it does change the resolved
    // syntect theme — invalidating here keeps split_points and the syntect
    // cache (added later) coherent.
    ui.style().visuals.dark_mode.hash(&mut h);
    // Caller-configured constraints that affect block widths.
    options.default_width.hash(&mut h);
    options.indentation_spaces.hash(&mut h);
    h.finish()
}

/// Whether a TagEnd marks a safe block-level boundary for viewport-skip.
///
/// At a block end the renderer's transient inline state (heading rich-text
/// accumulator, list nesting, emphasis flags) is neutral, so a future frame
/// can start rendering from the next event without losing context.
///
/// Inline ends (Emphasis, Strong, Link, Image, Superscript, Subscript) are
/// rejected — splitting mid-paragraph would orphan inline formatting state.
/// Table-internal ends (TableHead, TableRow, TableCell) are rejected because
/// tables are pre-parsed and rendered as a single atomic unit.
fn is_block_end_tag(tag: &pulldown_cmark::TagEnd) -> bool {
    use pulldown_cmark::TagEnd;
    matches!(
        tag,
        TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::BlockQuote(_)
            | TagEnd::CodeBlock
            | TagEnd::List(_)
            | TagEnd::Item
            | TagEnd::FootnoteDefinition
            | TagEnd::Table
            | TagEnd::HtmlBlock
            | TagEnd::MetadataBlock(_)
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
    )
}

/// Detect if text parsed as inline math (`$...$`) is actually NOT a real LaTeX
/// formula. Returns true for currency amounts and other false positives like:
/// - `$17.57` → parsed as InlineMath("17.57")
/// - `$3,000–$4,000` → parsed as InlineMath("3,000–")
/// - `$/t is worse because...total_usd...` → long English sentence with `_` in identifiers
///
/// The approach: real LaTeX math ALWAYS contains structural syntax (backslash
/// commands, superscripts, subscripts, braces, or known math operators).
/// Anything without these markers is almost certainly a misparse.
/// Additionally, very long "math" containing multiple English words is almost
/// certainly a false positive from `$` being used as currency.
fn is_likely_currency(tex: &str) -> bool {
    let trimmed = tex.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Real LaTeX commands (\frac, \sum, etc.) are the strongest signal
    let has_backslash_cmd = trimmed.contains('\\');
    if has_backslash_cmd {
        return false;
    }

    // Braces are strong LaTeX indicators (grouping: {x+1}, subscript: _{n})
    let has_braces = trimmed.contains('{') || trimmed.contains('}');
    if has_braces {
        return false;
    }

    // For ^ and _, only trust them as math if the content is short and
    // doesn't look like English prose. Long text with underscores from
    // identifiers (total_usd, miss_cost) is a false positive.
    let has_sub_super = trimmed.contains('^') || trimmed.contains('_');
    if has_sub_super {
        // Count whitespace-separated words — real inline math rarely has >5 words
        let word_count = trimmed.split_whitespace().count();
        if word_count > 5 {
            return true; // Too many words — this is prose, not math
        }
        // Short content with ^ or _ is likely real math (e.g., x_i, a^2)
        return false;
    }

    // No math syntax found — this is almost certainly a currency/misparse
    true
}

impl CommonMarkViewerInternal {
    /// Compute a hash of the text content for event cache lookup.
    fn hash_content(text: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }

    /// Be aware that this acquires egui::Context internally.
    /// If split Id is provided then split points will be populated
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        text: &str,
        split_points_id: Option<Id>,
    ) -> (egui::InnerResponse<()>, Vec<CheckboxClickEvent>) {
        let max_width = options.max_width(ui);
        let layout = egui::Layout::left_to_right(egui::Align::BOTTOM).with_main_wrap(true);

        // Compute content hash and ensure events are cached
        let content_hash = Self::hash_content(text);
        if cache.get_cached_events(content_hash).is_none() {
            let math_enabled = options.math_fn.is_some() || cfg!(feature = "math");
            let owned_events: Vec<(pulldown_cmark::Event<'static>, Range<usize>)> =
                pulldown_cmark::Parser::new_ext(text, parser_options_math(math_enabled))
                    .into_offset_iter()
                    .map(|(event, range)| (event.into_static(), range))
                    .collect();
            cache.set_cached_events(content_hash, owned_events);
        }

        let re = ui.allocate_ui_with_layout(egui::vec2(max_width, 0.0), layout, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let height = ui.text_style_height(&TextStyle::Body);
            ui.set_row_height(height);

            // Use cached events — clone the Vec reference data for iteration
            // (events are 'static so this is cheap pointer copies, not re-parsing)
            let events_data = cache.get_cached_events(content_hash)
                .expect("events just cached")
                .to_vec();
            let mut events = events_data
                .into_iter()
                .enumerate()
                .peekable();

            while let Some((index, (e, src_span))) = events.next() {
                let start_position = ui.next_widget_position();
                // Add a viewport-skip waypoint at every block-level end (not
                // just list-internal ends as the original code did). Without
                // this, docs whose content is mostly headings + paragraphs
                // produce empty split_points, the viewport-skip math falls
                // back to Pos2::ZERO, and rendered content overlaps. This is
                // the root cause of the "buggy in scenarios more complex
                // than the example application" warning on show_scrollable.
                let should_add_split_point = matches!(
                    &e,
                    pulldown_cmark::Event::End(end) if is_block_end_tag(end)
                );

                if events.peek().is_none() {
                    self.line.should_end_newline_forced = false;
                }

                self.process_event(ui, &mut events, e, src_span, cache, options, max_width);

                if let Some(source_id) = split_points_id {
                    if should_add_split_point {
                        let scroll_cache = scroll_cache(cache, &source_id);
                        let end_position = ui.next_widget_position();

                        let split_point_exists = scroll_cache
                            .split_points
                            .iter()
                            .any(|(i, _, _)| *i == index);

                        if !split_point_exists {
                            scroll_cache
                                .split_points
                                .push((index, start_position, end_position));
                        }
                    }
                }

                if index == 0 {
                    self.line.should_not_start_newline_forced = false;
                }
            }

            if let Some(source_id) = split_points_id {
                scroll_cache(cache, &source_id).page_size =
                    Some(ui.next_widget_position().to_vec2());
            }
        });

        (re, std::mem::take(&mut self.checkbox_events))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn show_scrollable(
        &mut self,
        source_id: Id,
        ui: &mut egui::Ui,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        text: &str,
        content_version: Option<u64>,
        pending_scroll_offset: Option<f32>,
        scroll_source: Option<egui::scroll_area::ScrollSource>,
    ) -> egui::scroll_area::ScrollAreaOutput<()> {
        let available_size = ui.available_size();
        let scroll_id = source_id.with("_scroll_area");
        let layout_sig = compute_layout_signature(ui, options);

        // Ensure parsed events are cached on the ScrollableCache, keyed by a
        // content version. The caller can provide a monotonic version (bumped
        // on every reload) — when omitted we fall back to hashing the content,
        // which still beats reparsing but is O(N) per frame for the hash.
        // The big win either way is avoiding pulldown_cmark::Parser::new_ext +
        // collect on every frame (~52 ms at 100k lines).
        let version = content_version.unwrap_or_else(|| Self::hash_content(text));
        {
            let sc = scroll_cache(cache, &source_id);
            if sc.events.is_empty() || sc.content_version != version {
                sc.events = pulldown_cmark::Parser::new_ext(
                    text,
                    parser_options_math(options.math_fn.is_some()),
                )
                .into_offset_iter()
                .map(|(e, r)| (e.into_static(), r))
                .collect();
                sc.content_version = version;
                // Content changed — cached split_points y-coords are no
                // longer valid for this content. Drop them so the first
                // post-change frame falls into the bootstrap branch below.
                sc.page_size = None;
                sc.split_points.clear();
            }
            // Width/zoom/theme change: y-coordinates are invalid for the
            // new layout, even though parsed events are still good.
            if sc.layout_signature != layout_sig {
                sc.layout_signature = layout_sig;
                sc.page_size = None;
                sc.split_points.clear();
                sc.available_size = available_size;
            }
            // When the caller wants to jump to a specific scroll position
            // (outline click, search-jump), we must paint *every* event
            // this frame — not just the viewport-clipped subset. Otherwise
            // a far target's block doesn't paint, the cache.active_search_y
            // / header_position never gets recorded, and the two-stage
            // corrective scroll (src/main.rs:scroll_to_active_match) can't
            // snap to the precise y. Forcing the bootstrap branch costs one
            // full-paint frame (~100 ms at 100k lines) per jump, which is
            // acceptable for a one-off action.
            if pending_scroll_offset.is_some() {
                sc.page_size = None;
                sc.split_points.clear();
            }
        }

        // Helper: build the renderer-owned ScrollArea with caller config.
        let make_scroll_area = || {
            let mut sa = egui::ScrollArea::vertical()
                .id_salt(scroll_id)
                .auto_shrink([false, true]);
            if let Some(offset) = pending_scroll_offset {
                sa = sa.vertical_scroll_offset(offset);
            }
            if let Some(src) = scroll_source {
                sa = sa.scroll_source(src);
            }
            sa
        };

        let page_size_opt = scroll_cache(cache, &source_id).page_size;
        let Some(page_size) = page_size_opt else {
            let out = make_scroll_area().show(ui, |ui| {
                // The inner show() runs inside a scrolled ScrollArea. The
                // cursor is viewport-relative, so we add the *current scroll
                // offset* — not 0 — to convert to content-relative y when
                // recording header / active-search positions. A non-zero
                // pending_scroll_offset means the caller jumped to that
                // position this frame; without this, the bootstrap records
                // every position shifted by -pending, corrupting the cache.
                cache.set_scroll_offset(pending_scroll_offset.unwrap_or(0.0));
                self.show(ui, cache, options, text, Some(source_id));
            });
            // Prevent repopulating points twice at startup
            scroll_cache(cache, &source_id).available_size = available_size;
            return out;
        };

        // Clone owned events out of the cache so we can iterate while
        // process_event mutably borrows the cache for syntect/header state.
        // The clone is O(events) but uses Event<'static>'s cheap refcounted
        // CowStr internals — measured ~11 ms at 100k lines vs ~52 ms parse.
        let events = scroll_cache(cache, &source_id).events.clone();

        let num_rows = events.len();

        make_scroll_area()
            .show_viewport(ui, |ui, viewport| {
                ui.set_height(page_size.y);
                // ui.cursor().top() inside show_viewport is viewport-relative;
                // record_header_position and record_active_search_y_viewport
                // add this offset to recover content-relative y.
                cache.set_scroll_offset(viewport.min.y);
                let layout = egui::Layout::left_to_right(egui::Align::BOTTOM).with_main_wrap(true);

                let max_width = options.max_width(ui);
                ui.allocate_ui_with_layout(egui::vec2(max_width, 0.0), layout, |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let scroll_cache = scroll_cache(cache, &source_id);

                    // split_points are populated in event order, which matches
                    // top-to-bottom layout order, so y-coords are monotonic
                    // non-decreasing. Binary-search instead of linear filter:
                    // O(log N) vs the old O(N) at 15k+ split points (100k-line doc).

                    // First waypoint: the second-to-last split point whose
                    // end.y is still above the viewport. Picking "second-to-last"
                    // gives us a safety frame above the viewport top to avoid
                    // clipping inline-flow content that started just above.
                    let above = scroll_cache
                        .split_points
                        .partition_point(|(_, _, end)| end.y < viewport.min.y);
                    let (first_event_index, _, first_end_position) = if above >= 2 {
                        scroll_cache.split_points[above - 2]
                    } else {
                        (0, Pos2::ZERO, Pos2::ZERO)
                    };

                    // Last waypoint: the second split point whose start.y is
                    // strictly below the viewport bottom. Same safety idea on
                    // the bottom edge.
                    let below = scroll_cache
                        .split_points
                        .partition_point(|(_, start, _)| start.y <= viewport.max.y);
                    let last_event_index = scroll_cache
                        .split_points
                        .get(below + 1)
                        .map(|(index, _, _)| *index)
                        .unwrap_or(num_rows);

                    ui.allocate_space(first_end_position.to_vec2());

                    // only rendering the elements that are inside the viewport
                    let mut events = events
                        .into_iter()
                        .enumerate()
                        .skip(first_event_index)
                        .take(last_event_index - first_event_index)
                        .peekable();

                    while let Some((i, (e, src_span))) = events.next() {
                        if events.peek().is_none() {
                            self.line.should_end_newline_forced = false;
                        }

                        self.process_event(ui, &mut events, e, src_span, cache, options, max_width);

                        if i == 0 {
                            self.line.should_not_start_newline_forced = false;
                        }
                    }
                });
            })
        // No trailing invalidation needed — layout_signature is checked at
        // the top of show_scrollable, so a width/zoom/theme change in the
        // same frame falls into the bootstrap branch above immediately
        // instead of one frame later.
    }

    #[allow(clippy::too_many_arguments)]
    fn process_event<'e>(
        &mut self,
        ui: &mut Ui,
        events: &mut Peekable<impl Iterator<Item = EventIteratorItem<'e>>>,
        event: pulldown_cmark::Event,
        src_span: Range<usize>,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        max_width: f32,
    ) {
        self.event(ui, event, src_span, cache, options, max_width);

        self.def_list_def_wrapping(events, max_width, cache, options, ui);
        self.item_list_wrapping(events, max_width, cache, options, ui);
        self.table(events, cache, options, ui, max_width);
        self.blockquote(events, max_width, cache, options, ui);
    }

    fn def_list_def_wrapping<'e>(
        &mut self,
        events: &mut Peekable<impl Iterator<Item = EventIteratorItem<'e>>>,
        max_width: f32,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        ui: &mut Ui,
    ) {
        if self.def_list.is_def_list_def {
            self.def_list.is_def_list_def = false;

            let item_events = delayed_events(events, |tag| {
                matches!(tag, pulldown_cmark::TagEnd::DefinitionListDefinition)
            });

            let mut events_iter = item_events.into_iter().enumerate().peekable();

            self.line.try_insert_start(ui);

            // Proccess a single event separately so that we do not insert spaces where we do not
            // want them
            self.line.should_start_newline = false;
            if let Some((_, (e, src_span))) = events_iter.next() {
                self.process_event(ui, &mut events_iter, e, src_span, cache, options, max_width);
            }

            ui.label(" ".repeat(options.indentation_spaces));
            self.line.should_start_newline = true;
            self.line.should_end_newline = false;
            // Required to ensure that the content is aligned with the identation
            ui.horizontal_wrapped(|ui| {
                while let Some((_, (e, src_span))) = events_iter.next() {
                    self.process_event(
                        ui,
                        &mut events_iter,
                        e,
                        src_span,
                        cache,
                        options,
                        max_width,
                    );
                }
            });
            self.line.should_end_newline = true;

            // Only end the definition items line if it is not the last element in the list
            if !matches!(
                events.peek(),
                Some((
                    _,
                    (
                        pulldown_cmark::Event::End(pulldown_cmark::TagEnd::DefinitionList),
                        _
                    )
                ))
            ) {
                self.line.try_insert_end(ui);
            }
        }
    }

    fn item_list_wrapping<'e>(
        &mut self,
        events: &mut impl Iterator<Item = EventIteratorItem<'e>>,
        max_width: f32,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        ui: &mut Ui,
    ) {
        if self.is_list_item {
            self.is_list_item = false;

            let item_events = delayed_events_list_item(events);
            let mut events_iter = item_events.into_iter().enumerate().peekable();

            // Required to ensure that the content of the list item is aligned with
            // the * or - when wrapping
            ui.horizontal_wrapped(|ui| {
                while let Some((_, (e, src_span))) = events_iter.next() {
                    self.process_event(
                        ui,
                        &mut events_iter,
                        e,
                        src_span,
                        cache,
                        options,
                        max_width,
                    );
                }
            });
        }
    }

    fn blockquote<'e>(
        &mut self,
        events: &mut Peekable<impl Iterator<Item = EventIteratorItem<'e>>>,
        max_width: f32,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        ui: &mut Ui,
    ) {
        if self.is_blockquote {
            let mut collected_events = delayed_events(events, |tag| {
                matches!(tag, pulldown_cmark::TagEnd::BlockQuote(_))
            });
            self.line.try_insert_start(ui);

            // Currently the blockquotes are made in such a way that they need a newline at the end
            // and the start so when this is the first element in the markdown the newline must be
            // manually enabled
            self.line.should_not_start_newline_forced = false;
            if let Some(alert) = parse_alerts(&options.alerts, &mut collected_events) {
                egui_commonmark_backend_extended::alert_ui(alert, ui, |ui| {
                    for (event, src_span) in collected_events {
                        self.event(ui, event, src_span, cache, options, max_width);
                    }
                })
            } else {
                blockquote(ui, ui.visuals().weak_text_color(), |ui| {
                    self.text_style.quote = true;
                    for (event, src_span) in collected_events {
                        self.event(ui, event, src_span, cache, options, max_width);
                    }
                    self.text_style.quote = false;
                });
            }

            if events.peek().is_none() {
                self.line.should_end_newline_forced = false;
            }

            self.line.try_insert_end(ui);
            self.is_blockquote = false;
        }
    }

    fn table<'e>(
        &mut self,
        events: &mut Peekable<impl Iterator<Item = EventIteratorItem<'e>>>,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        ui: &mut Ui,
        max_width: f32,
    ) {
        if self.is_table {
            self.line.try_insert_start(ui);

            let id = ui.id().with("_table").with(self.curr_table);
            self.curr_table += 1;

            // Consume events into header/rows up front so we know the column count
            // (TableBuilder requires the column count declared before rendering).
            // `header` is a Vec<Cell> for a single header row, so `header.len()` is
            // the column count. Each row in `rows` is itself a Vec<Cell>.
            let Table { header, rows } = parse_table(events);
            // Drop trailing empty rows that pulldown_cmark sometimes appends.
            let rows: Vec<_> = rows.into_iter().filter(|r| !r.is_empty()).collect();
            let num_cols = if !header.is_empty() {
                header.len()
            } else {
                rows.first().map(|r| r.len()).unwrap_or(0)
            };
            let line_h = ui.text_style_height(&egui::TextStyle::Body);

            if num_cols == 0 {
                self.is_table = false;
                if events.peek().is_none() {
                    self.line.should_end_newline_forced = false;
                }
                self.line.try_insert_end(ui);
                return;
            }
            // Per-line cell height; rows grow taller when cells contain multi-chunk
            // inline-code wraps (computed below via `cell_visual_lines`).
            let cell_h = line_h * 1.5;
            // Header is one row; its height grows if any header cell has wrapped code.
            let header_lines = header
                .iter()
                .map(|c| cell_visual_lines(c))
                .max()
                .unwrap_or(1);
            let header_h = cell_h * header_lines as f32;
            // Pre-compute per-body-row height so multi-chunk cells aren't clipped.
            let body_heights: Vec<f32> = rows
                .iter()
                .map(|row| {
                    let max_lines = row
                        .iter()
                        .map(|c| cell_visual_lines(c))
                        .max()
                        .unwrap_or(1);
                    cell_h * max_lines as f32
                })
                .collect();
            // Outer ScrollArea::horizontal handles the case where columns
            // (auto-sized to content) total wider than the parent ui; without it,
            // narrow windows clip the rightmost columns. Capture the output so
            // `forward_wheel_to_horizontal_scroll` can redirect vertical wheel
            // deltas into the inner area's horizontal offset when hovered.
            // ui.vertical(...) is essential: TableBuilder's body() positions itself
            // relative to the parent's cursor, but the parent here is a horizontal-
            // flow Ui from the markdown renderer. Without the vertical scope the
            // body's first row overlaps the header row.
            let mut scroll_out = egui::ScrollArea::horizontal()
                .id_salt(id.with("_scroll"))
                .max_width(max_width)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            let table = egui_extras::TableBuilder::new(ui)
                                .id_salt(id)
                                .striped(true)
                                .resizable(true)
                                .vscroll(false)
                                .auto_shrink([false, true])
                                .min_scrolled_height(0.0)
                                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                                .columns(
                                    egui_extras::Column::auto().resizable(true).at_least(40.0),
                                    num_cols,
                                )
                                .header(header_h, |mut row| {
                                    for col in header {
                                        row.col(|ui| {
                                            let col_w = ui.available_width();
                                            for (e, src_span) in col {
                                                let tmp_start = std::mem::replace(
                                                    &mut self.line.should_start_newline,
                                                    false,
                                                );
                                                let tmp_end = std::mem::replace(
                                                    &mut self.line.should_end_newline,
                                                    false,
                                                );
                                                self.event(
                                                    ui, e, src_span, cache, options, col_w,
                                                );
                                                self.line.should_start_newline = tmp_start;
                                                self.line.should_end_newline = tmp_end;
                                            }
                                        });
                                    }
                                });
                            table.body(|mut body| {
                                for (row_idx, row) in rows.into_iter().enumerate() {
                                    let h = body_heights
                                        .get(row_idx)
                                        .copied()
                                        .unwrap_or(cell_h);
                                    body.row(h, |mut row_ui| {
                                        for col in row {
                                            row_ui.col(|ui| {
                                                let col_w = ui.available_width();
                                                for (e, src_span) in col {
                                                    let tmp_start = std::mem::replace(
                                                        &mut self.line.should_start_newline,
                                                        false,
                                                    );
                                                    let tmp_end = std::mem::replace(
                                                        &mut self.line.should_end_newline,
                                                        false,
                                                    );
                                                    self.event(
                                                        ui, e, src_span, cache, options, col_w,
                                                    );
                                                    self.line.should_start_newline = tmp_start;
                                                    self.line.should_end_newline = tmp_end;
                                                }
                                            });
                                        }
                                    });
                                }
                            });
                        });
                    });
                });
            forward_wheel_to_horizontal_scroll(ui, &mut scroll_out);

            self.is_table = false;
            if events.peek().is_none() {
                self.line.should_end_newline_forced = false;
            }

            self.line.try_insert_end(ui);
        }
    }

    fn event(
        &mut self,
        ui: &mut Ui,
        event: pulldown_cmark::Event,
        src_span: Range<usize>,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        max_width: f32,
    ) {
        match event {
            pulldown_cmark::Event::Start(tag) => self.start_tag(ui, tag, options),
            pulldown_cmark::Event::End(tag) => self.end_tag(ui, tag, cache, options, max_width),
            pulldown_cmark::Event::Text(text) => {
                self.event_text_with_highlights(text, &src_span, cache, ui, options);
            }
            pulldown_cmark::Event::Code(text) => {
                self.text_style.code = true;
                let segments = inline_code_wrap_segments(&text);
                let wrap = segments.len() > 1;
                // For non-wrapped inline code, derive an interior span (strip equal
                // backticks on each side) so search highlights line up with the visible
                // code text. Wrapped (>56 char) code skips highlighting in v1.
                let interior_span = if !wrap && src_span.len() >= text.len() {
                    let delim_total = src_span.len() - text.len();
                    if delim_total > 0 && delim_total % 2 == 0 {
                        let bt = delim_total / 2;
                        Some((src_span.start + bt)..(src_span.end - bt))
                    } else {
                        None
                    }
                } else {
                    None
                };
                for segment in segments {
                    if let Some(ref span) = interior_span {
                        self.event_text_with_highlights(
                            segment.into(),
                            span,
                            cache,
                            ui,
                            options,
                        );
                    } else {
                        self.event_text(segment.into(), ui, options);
                    }
                    if wrap {
                        ui.end_row();
                    }
                }
                self.text_style.code = false;
            }
            pulldown_cmark::Event::InlineHtml(text) => {
                self.event_text(text, ui, options);
            }

            pulldown_cmark::Event::Html(text) => {
                // Always accumulate HTML blocks for table detection
                self.html_block.push_str(&text);
            }
            pulldown_cmark::Event::FootnoteReference(footnote) => {
                footnote_start(ui, &footnote);
            }
            pulldown_cmark::Event::SoftBreak => {
                soft_break(ui);
            }
            pulldown_cmark::Event::HardBreak => newline(ui),
            pulldown_cmark::Event::Rule => {
                self.line.try_insert_start(ui);
                rule(ui, self.line.can_insert_end());
            }
            pulldown_cmark::Event::TaskListMarker(mut checkbox) => {
                if options.mutable {
                    if ui
                        .add(egui::Checkbox::without_text(&mut checkbox))
                        .clicked()
                    {
                        self.checkbox_events.push(CheckboxClickEvent {
                            checked: checkbox,
                            span: src_span,
                        });
                    }
                } else {
                    ui.add(ImmutableCheckbox::without_text(&mut checkbox));
                }
            }
            pulldown_cmark::Event::InlineMath(tex) => {
                if is_likely_currency(&tex) {
                    // Render as plain text with $ prefix instead of math
                    let text: CowStr = format!("${tex}").into();
                    self.event_text(text, ui, options);
                } else {
                    #[cfg(feature = "math")]
                    {
                        crate::render_math(ui, cache, &tex, true);
                    }
                    #[cfg(not(feature = "math"))]
                    if let Some(math_fn) = options.math_fn {
                        math_fn(ui, &tex, true);
                    }
                }
            }
            pulldown_cmark::Event::DisplayMath(tex) => {
                #[cfg(feature = "math")]
                {
                    crate::render_math(ui, cache, &tex, false);
                }
                #[cfg(not(feature = "math"))]
                if let Some(math_fn) = options.math_fn {
                    math_fn(ui, &tex, false);
                }
            }
        }
    }

    fn event_text(&mut self, text: CowStr, ui: &mut Ui, options: &CommonMarkOptions) {
        self.emit_text(text, HighlightKind::None, ui, options);
    }

    /// Like `event_text`, but applies a search-highlight background color when `hl` is not `None`.
    fn emit_text(
        &mut self,
        text: CowStr,
        hl: HighlightKind,
        ui: &mut Ui,
        options: &CommonMarkOptions,
    ) {
        let bg = hl.background_color(ui);
        let mut rich_text = if bg.is_some() && self.text_style.code {
            // egui's RichText renderer overrides `background_color` with the theme's
            // `code_bg_color` whenever `.code()` is set (widget_text.rs:421). To make
            // our search highlight visible inside inline code, build the RichText
            // manually with a monospace font instead of calling `.code()` — that gives
            // the visual effect of code (monospace + slightly larger weight) while
            // letting our background_color survive.
            let mut t = egui::RichText::new(text.as_ref())
                .text_style(egui::TextStyle::Monospace);
            if self.text_style.strong {
                t = t.strong();
            }
            if self.text_style.emphasis {
                t = t.italics();
            }
            if self.text_style.strikethrough {
                t = t.strikethrough();
            }
            if self.text_style.quote {
                t = t.weak();
            }
            t
        } else {
            self.text_style
                .to_richtext_with_typography(ui, &text, Some(&options.typography))
        };
        if let Some(bg) = bg {
            rich_text = rich_text.background_color(bg);
        }
        if let Some(image) = &mut self.image {
            image.alt_text.push(rich_text);
        } else if let Some(block) = &mut self.code_block {
            // Code blocks render via syntect after end_tag; highlight inside code
            // blocks is a v2 feature (would need syntect integration). Just collect text.
            block.content.push_str(&text);
        } else if let Some(link) = &mut self.link {
            link.text.push(rich_text);
        } else if self.text_style.heading.is_some() {
            // Accumulate heading text for position tracking
            self.current_heading_text.push_str(&text);
            // Accumulate RichText - will render all at once in end_tag(Heading)
            self.current_heading_rich_texts.push(rich_text);
        } else {
            ui.label(rich_text);
        }
    }

    /// Split `text` at search-match boundaries (using `span` to locate matches in the
    /// source content) and emit each segment with the appropriate highlight. Falls back
    /// to plain `event_text` when there are no ranges or `text.len() != span.len()`
    /// (markdown escapes or smart-punct transforms can break the 1:1 byte mapping).
    fn event_text_with_highlights(
        &mut self,
        text: CowStr,
        span: &Range<usize>,
        cache: &mut CommonMarkCache,
        ui: &mut Ui,
        options: &CommonMarkOptions,
    ) {
        let ranges = cache.search_ranges();
        if ranges.is_empty() || text.len() != span.len() {
            self.event_text(text, ui, options);
            return;
        }
        let segments =
            compute_highlight_segments(&text, span, ranges, cache.active_search_range());
        for (segment_text, hl) in segments {
            // Record the cursor y for the Active segment BEFORE emitting it, so the
            // app can scroll to the active match's actual position (line-ratio
            // estimates are unreliable in image-heavy docs).
            if hl == HighlightKind::Active {
                let vy = ui.cursor().top();
                cache.record_active_search_y_viewport(vy);
            }
            self.emit_text(segment_text.into(), hl, ui, options);
        }
    }

    fn start_tag(&mut self, ui: &mut Ui, tag: pulldown_cmark::Tag, options: &CommonMarkOptions) {
        match tag {
            pulldown_cmark::Tag::Paragraph => {
                self.line.try_insert_start(ui);
            }
            pulldown_cmark::Tag::Heading { level, .. } => {
                // End current row to ensure heading starts at left edge
                ui.end_row();
                // Record position BEFORE spacing for scroll navigation
                self.current_heading_y = Some(ui.cursor().top());
                self.current_heading_text.clear();
                // Add extra spacing above headings if configured
                heading_start_spacing(ui, &options.typography);
                self.text_style.heading = Some(match level {
                    HeadingLevel::H1 => 0,
                    HeadingLevel::H2 => 1,
                    HeadingLevel::H3 => 2,
                    HeadingLevel::H4 => 3,
                    HeadingLevel::H5 => 4,
                    HeadingLevel::H6 => 5,
                });
            }

            // deliberately not using the built in alerts from pulldown-cmark as
            // the markdown itself cannot be localized :( e.g: [!TIP]
            pulldown_cmark::Tag::BlockQuote(_) => {
                self.is_blockquote = true;
            }
            pulldown_cmark::Tag::CodeBlock(c) => {
                match c {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => {
                        self.code_block = Some(crate::CodeBlock {
                            lang: Some(lang.to_string()),
                            content: "".to_string(),
                        });
                    }
                    pulldown_cmark::CodeBlockKind::Indented => {
                        self.code_block = Some(crate::CodeBlock {
                            lang: None,
                            content: "".to_string(),
                        });
                    }
                }
                self.line.try_insert_start(ui);
            }

            pulldown_cmark::Tag::List(point) => {
                if !self.list.is_inside_a_list() && self.line.can_insert_start() {
                    newline(ui);
                }

                if let Some(number) = point {
                    self.list.start_level_with_number(number);
                } else {
                    self.list.start_level_without_number();
                }
                self.line.should_start_newline = false;
                self.line.should_end_newline = false;
            }

            pulldown_cmark::Tag::Item => {
                self.is_list_item = true;
                self.list.start_item(ui, options);
            }

            pulldown_cmark::Tag::FootnoteDefinition(note) => {
                self.line.try_insert_start(ui);

                self.line.should_start_newline = false;
                self.line.should_end_newline = false;
                footnote(ui, &note);
            }
            pulldown_cmark::Tag::Table(_) => {
                self.is_table = true;
            }
            pulldown_cmark::Tag::TableHead => {}
            pulldown_cmark::Tag::TableRow => {}
            pulldown_cmark::Tag::TableCell => {}
            pulldown_cmark::Tag::Emphasis => {
                self.text_style.emphasis = true;
            }
            pulldown_cmark::Tag::Strong => {
                self.text_style.strong = true;
            }
            pulldown_cmark::Tag::Strikethrough => {
                self.text_style.strikethrough = true;
            }
            pulldown_cmark::Tag::Link { dest_url, .. } => {
                self.link = Some(crate::Link {
                    destination: dest_url.to_string(),
                    text: Vec::new(),
                });
            }
            pulldown_cmark::Tag::Image { dest_url, .. } => {
                self.image = Some(crate::Image::new(&dest_url, options));
            }
            pulldown_cmark::Tag::HtmlBlock => {
                self.line.try_insert_start(ui);
            }
            pulldown_cmark::Tag::MetadataBlock(_) => {}

            pulldown_cmark::Tag::DefinitionList => {
                self.line.try_insert_start(ui);
                self.def_list.is_first_item = true;
            }
            pulldown_cmark::Tag::DefinitionListTitle => {
                // we disable newline as the first title should not insert a newline
                // as we have already done that upon the DefinitionList Tag
                if !self.def_list.is_first_item {
                    self.line.try_insert_start(ui)
                } else {
                    self.def_list.is_first_item = false;
                }
            }
            pulldown_cmark::Tag::DefinitionListDefinition => {
                self.def_list.is_def_list_def = true;
            }
            // Not yet supported
            pulldown_cmark::Tag::Superscript | pulldown_cmark::Tag::Subscript => {}
        }
    }

    fn end_tag(
        &mut self,
        ui: &mut Ui,
        tag: pulldown_cmark::TagEnd,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        max_width: f32,
    ) {
        match tag {
            pulldown_cmark::TagEnd::Paragraph => {
                self.line.try_insert_end(ui);
                // Add extra paragraph spacing if configured
                paragraph_end_spacing(ui, &options.typography);
            }
            pulldown_cmark::TagEnd::Heading { .. } => {
                // Render all accumulated heading fragments at once, positioned at left edge
                if !self.current_heading_rich_texts.is_empty() {
                    let available = ui.available_rect_before_wrap();
                    let left_edge = ui.min_rect().left();
                    let heading_rect = egui::Rect::from_min_size(
                        egui::pos2(left_edge, available.top()),
                        egui::vec2(available.width() + (available.left() - left_edge), available.height()),
                    );
                    let rich_texts = std::mem::take(&mut self.current_heading_rich_texts);
                    ui.allocate_ui_at_rect(heading_rect, |ui| {
                        for rt in rich_texts {
                            ui.label(rt);
                        }
                    });
                }
                // Record header position for scroll navigation. Composite key
                // is `normalized_title` for the 0th occurrence and
                // `normalized_title#N` for the Nth duplicate (matches the key
                // built by the app's `header_position_key` helper), so multiple
                // headings with the same title get distinct cache entries.
                if let Some(y) = self.current_heading_y.take() {
                    if !self.current_heading_text.is_empty() {
                        let normalized = self.current_heading_text.trim().to_lowercase();
                        let nth = self
                            .heading_occurrence_counts
                            .entry(normalized.clone())
                            .or_insert(0);
                        let key = if *nth == 0 {
                            normalized.clone()
                        } else {
                            format!("{normalized}#{nth}")
                        };
                        *nth += 1;
                        cache.record_header_position(&key, y);
                    }
                }
                self.current_heading_text.clear();
                // Add extra spacing below headings if configured
                heading_end_spacing(ui, &options.typography);
                self.line.try_insert_end(ui);
                self.text_style.heading = None;
            }
            pulldown_cmark::TagEnd::BlockQuote(_) => {}
            pulldown_cmark::TagEnd::CodeBlock => {
                self.end_code_block(ui, cache, options, max_width);
            }

            pulldown_cmark::TagEnd::List(_) => {
                if self.list.is_last_level() {
                    self.line.should_start_newline = true;
                    self.line.should_end_newline = true;
                }

                self.list.end_level(ui, self.line.can_insert_end());

                if !self.list.is_inside_a_list() {
                    // Reset all the state and make it ready for the next list that occurs
                    self.list = List::default();
                }
            }
            pulldown_cmark::TagEnd::Item => {}
            pulldown_cmark::TagEnd::FootnoteDefinition => {
                self.line.should_start_newline = true;
                self.line.should_end_newline = true;
                self.line.try_insert_end(ui);
            }
            pulldown_cmark::TagEnd::Table => {}
            pulldown_cmark::TagEnd::TableHead => {}
            pulldown_cmark::TagEnd::TableRow => {}
            pulldown_cmark::TagEnd::TableCell => {
                // Ensure space between cells
                ui.label("  ");
            }
            pulldown_cmark::TagEnd::Emphasis => {
                self.text_style.emphasis = false;
            }
            pulldown_cmark::TagEnd::Strong => {
                self.text_style.strong = false;
            }
            pulldown_cmark::TagEnd::Strikethrough => {
                self.text_style.strikethrough = false;
            }
            pulldown_cmark::TagEnd::Link => {
                if let Some(link) = self.link.take() {
                    link.end(ui, cache);
                }
            }
            pulldown_cmark::TagEnd::Image => {
                if let Some(image) = self.image.take() {
                    image.end(ui, cache, options);
                }
            }
            pulldown_cmark::TagEnd::HtmlBlock => {
                if !self.html_block.is_empty() {
                    if let Some(table) = egui_commonmark_backend_extended::html_table::parse_html_table(&self.html_block) {
                        self.render_html_table(ui, &table, options, max_width);
                    } else if let Some(html_fn) = options.html_fn {
                        html_fn(ui, &self.html_block);
                    } else {
                        // Render non-table HTML as plain text (existing fallback)
                        let text: pulldown_cmark::CowStr = std::mem::take(&mut self.html_block).into();
                        self.event_text(text, ui, options);
                    }
                    self.html_block.clear();
                }
            }

            pulldown_cmark::TagEnd::MetadataBlock(_) => {}

            pulldown_cmark::TagEnd::DefinitionList => self.line.try_insert_end(ui),
            pulldown_cmark::TagEnd::DefinitionListTitle
            | pulldown_cmark::TagEnd::DefinitionListDefinition => {}
            pulldown_cmark::TagEnd::Superscript | pulldown_cmark::TagEnd::Subscript => {}
        }
    }

    fn end_code_block(
        &mut self,
        ui: &mut Ui,
        cache: &mut CommonMarkCache,
        options: &CommonMarkOptions,
        max_width: f32,
    ) {
        if let Some(block) = self.code_block.take() {
            let id = ui.id().with("_code_block").with(self.curr_code_block);
            self.curr_code_block += 1;
            block.end(ui, cache, options, max_width, id);
            self.line.try_insert_end(ui);
        }
    }

    fn render_html_table(
        &mut self,
        ui: &mut Ui,
        table: &egui_commonmark_backend_extended::html_table::HtmlTable,
        options: &CommonMarkOptions,
        max_width: f32,
    ) {
        let id = ui.id().with("_html_table").with(self.curr_table);
        self.curr_table += 1;

        let num_cols = table
            .header
            .first()
            .or(table.rows.first())
            .map(|r| r.len())
            .unwrap_or(0);

        let line_h = ui.text_style_height(&egui::TextStyle::Body);
        let cell_h = line_h * 1.5;

        if num_cols == 0 {
            self.line.try_insert_end(ui);
            return;
        }

        // Heuristic per-row heights: count explicit newlines + crude wrap est at
        // ~60 chars/visual-line. Over-estimates slightly (extra row height is
        // preferable to clipping). Header rows use the same heuristic.
        let row_height_for = |cells: &[String]| -> f32 {
            let max_lines = cells
                .iter()
                .map(|c| html_cell_visual_lines(c))
                .max()
                .unwrap_or(1);
            cell_h * max_lines as f32
        };
        let header_h = table
            .header
            .first()
            .map(|row| row_height_for(row))
            .unwrap_or(cell_h);
        let extra_header_heights: Vec<f32> = table
            .header
            .iter()
            .skip(1)
            .map(|row| row_height_for(row))
            .collect();
        let body_heights: Vec<f32> = table.rows.iter().map(|row| row_height_for(row)).collect();

        // Outer ScrollArea::horizontal handles wide tables that exceed parent width;
        // ui.vertical() prevents the header/body Y-overlap quirk. Capture the output
        // so `forward_wheel_to_horizontal_scroll` can redirect wheel deltas.
        let mut scroll_out = egui::ScrollArea::horizontal()
            .id_salt(id.with("_scroll"))
            .max_width(max_width)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        let builder = egui_extras::TableBuilder::new(ui)
                            .id_salt(id)
                            .striped(true)
                            .resizable(true)
                            .vscroll(false)
                            .auto_shrink([false, true])
                            .min_scrolled_height(0.0)
                            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                            .columns(
                                egui_extras::Column::auto().resizable(true).at_least(40.0),
                                num_cols,
                            );

                        let render_cell_strong = |ui: &mut Ui, cell: &str| {
                            egui::Frame::NONE
                                .inner_margin(egui::Margin::symmetric(8, 4))
                                .show(ui, |ui| {
                                    ui.strong(cell);
                                });
                        };

                        if let Some(first_header) = table.header.first() {
                            builder
                                .header(header_h, |mut row| {
                                    for cell in first_header {
                                        row.col(|ui| render_cell_strong(ui, cell));
                                    }
                                })
                                .body(|mut body| {
                                    // Extra header rows after the first render as bold
                                    // body rows (TableBuilder has only one native header row).
                                    for (idx, extra) in table.header.iter().skip(1).enumerate() {
                                        let h = extra_header_heights
                                            .get(idx)
                                            .copied()
                                            .unwrap_or(cell_h);
                                        body.row(h, |mut row_ui| {
                                            for cell in extra {
                                                row_ui.col(|ui| render_cell_strong(ui, cell));
                                            }
                                        });
                                    }
                                    for (row_idx, row) in table.rows.iter().enumerate() {
                                        let h = body_heights
                                            .get(row_idx)
                                            .copied()
                                            .unwrap_or(cell_h);
                                        body.row(h, |mut row_ui| {
                                            for cell in row {
                                                row_ui.col(|ui| {
                                                    egui::Frame::NONE
                                                        .inner_margin(egui::Margin::symmetric(
                                                            8, 4,
                                                        ))
                                                        .show(ui, |ui| {
                                                            let rich_text = self
                                                                .text_style
                                                                .to_richtext_with_typography(
                                                                    ui,
                                                                    cell,
                                                                    Some(&options.typography),
                                                                );
                                                            ui.label(rich_text);
                                                        });
                                                });
                                            }
                                        });
                                    }
                                });
                        } else {
                            builder.body(|mut body| {
                                for (row_idx, row) in table.rows.iter().enumerate() {
                                    let h = body_heights
                                        .get(row_idx)
                                        .copied()
                                        .unwrap_or(cell_h);
                                    body.row(h, |mut row_ui| {
                                        for cell in row {
                                            row_ui.col(|ui| {
                                                egui::Frame::NONE
                                                    .inner_margin(egui::Margin::symmetric(8, 4))
                                                    .show(ui, |ui| {
                                                        let rich_text = self
                                                            .text_style
                                                            .to_richtext_with_typography(
                                                                ui,
                                                                cell,
                                                                Some(&options.typography),
                                                            );
                                                        ui.label(rich_text);
                                                    });
                                            });
                                        }
                                    });
                                }
                            });
                        }
                    });
                });
            });
        forward_wheel_to_horizontal_scroll(ui, &mut scroll_out);

        self.line.try_insert_end(ui);
    }
}
