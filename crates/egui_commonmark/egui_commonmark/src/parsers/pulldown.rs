use std::collections::HashMap;
use std::fmt::Write;
use std::hash::{Hash, Hasher};
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
use unicode_segmentation::UnicodeSegmentation;

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

/// One borrowed visible segment plus its exact identity in raw Markdown source.
#[derive(Clone, Debug, PartialEq, Eq)]
struct EmojiTextSegment<'a> {
    rendered: &'a str,
    source_range: Range<usize>,
    raw: &'a str,
    replaced: bool,
}

/// Visit recognized `:shortcode:` replacements without allocating unchanged text.
fn visit_emoji_text_segments<'a>(
    text: &'a str,
    span: &Range<usize>,
    mut visit: impl FnMut(EmojiTextSegment<'a>),
) {
    // Transformed pulldown text cannot be mapped safely back to original source bytes.
    if text.len() != span.len() {
        visit(EmojiTextSegment {
            rendered: text,
            source_range: span.clone(),
            raw: text,
            replaced: false,
        });
        return;
    }

    let mut plain_start = 0usize;
    let mut search_from = 0usize;
    let mut replaced_any = false;

    while let Some(open_rel) = text[search_from..].find(':') {
        let open = search_from + open_rel;
        let name_start = open + 1;
        let Some(close_rel) = text[name_start..].find(':') else {
            break;
        };
        let close = name_start + close_rel;
        let name = &text[name_start..close];

        if !name.is_empty() {
            if let Some(emoji) = emojis::get_by_shortcode(name) {
                if plain_start < open {
                    visit(EmojiTextSegment {
                        rendered: &text[plain_start..open],
                        source_range: span.start + plain_start..span.start + open,
                        raw: &text[plain_start..open],
                        replaced: false,
                    });
                }

                let raw_end = close + 1;
                visit(EmojiTextSegment {
                    rendered: emoji.as_str(),
                    source_range: span.start + open..span.start + raw_end,
                    raw: &text[open..raw_end],
                    replaced: true,
                });
                replaced_any = true;
                plain_start = raw_end;
                search_from = raw_end;
                continue;
            }
        }

        // Keep unknown syntax literal while searching later openers in this event.
        search_from = name_start;
    }

    if !replaced_any {
        visit(EmojiTextSegment {
            rendered: text,
            source_range: span.clone(),
            raw: text,
            replaced: false,
        });
    } else if plain_start < text.len() {
        visit(EmojiTextSegment {
            rendered: &text[plain_start..],
            source_range: span.start + plain_start..span.end,
            raw: &text[plain_start..],
            replaced: false,
        });
    }
}

/// Only ordinary visible text may expand; image alt text and code blocks stay literal.
fn emoji_expansion_is_eligible(in_image: bool, in_code_block: bool) -> bool {
    !in_image && !in_code_block
}

/// Resolve one indivisible replacement against source-authoritative search ranges.
fn highlight_for_source_span(
    source_span: &Range<usize>,
    ranges: &[Range<usize>],
    active: Option<&Range<usize>>,
) -> HighlightKind {
    let overlaps =
        |range: &Range<usize>| range.start < source_span.end && source_span.start < range.end;

    if active.is_some_and(overlaps) {
        HighlightKind::Active
    } else if ranges.iter().any(overlaps) {
        HighlightKind::Match
    } else {
        HighlightKind::None
    }
}

/// Visit borrowed text slices tagged with exact source-range highlighting.
/// Assumes non-overlapping source ranges, matching app search production.
fn visit_highlight_segments<'a>(
    text: &'a str,
    span: &Range<usize>,
    ranges: &[Range<usize>],
    active: Option<&Range<usize>>,
    mut visit: impl FnMut(&'a str, HighlightKind),
) {
    let mut cursor = 0usize;
    let mut found = false;

    for range in ranges {
        let start = range.start.max(span.start);
        let end = range.end.min(span.end);
        if start >= end {
            continue;
        }

        let local_start = start - span.start;
        let local_end = end - span.start;
        if !text.is_char_boundary(local_start) || !text.is_char_boundary(local_end) {
            continue;
        }

        found = true;
        if local_start > cursor && text.is_char_boundary(cursor) {
            visit(&text[cursor..local_start], HighlightKind::None);
        }
        let kind = if active == Some(range) {
            HighlightKind::Active
        } else {
            HighlightKind::Match
        };
        visit(&text[local_start..local_end], kind);
        cursor = local_end;
    }

    if !found {
        visit(text, HighlightKind::None);
    } else if cursor < text.len() && text.is_char_boundary(cursor) {
        visit(&text[cursor..], HighlightKind::None);
    }
}

/// Split a long inline-code token into fixed-size chunks so the row-wrap layout
/// can put each chunk on its own row instead of overflowing the content width.
/// Short tokens (<= MAX) pass through unchanged.
///
/// Blind char-count cut (not break-friendly on `/`, `-`, etc.): variable-length
/// segments can still exceed the column at narrow widths and re-introduce the
/// original clipping bug. Fixed-size chunks always fit.
/// Split a YAML frontmatter block into top-level key/value pairs.
///
/// Deliberately not a YAML parser. VS Code renders frontmatter as a flat
/// two-column table and does not descend into nested structures; matching that
/// keeps a de-facto-standard block readable without taking on a YAML
/// dependency and its error modes. Anything that is not a top-level
/// `key: value` line — nested mappings, sequence items, folded scalars — is
/// appended to the preceding value verbatim, so no source text is dropped.
fn parse_frontmatter_pairs(raw: &str) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = Vec::new();

    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }

        // A continuation line is indented, or is a sequence item, or has no
        // colon at all. Only an unindented `key:` starts a new row.
        let is_continuation = line.starts_with(char::is_whitespace) || line.trim_start().starts_with('-');
        let split = if is_continuation {
            None
        } else {
            line.find(':')
        };

        match split {
            Some(idx) => {
                let key = line[..idx].trim().to_owned();
                let value = line[idx + 1..].trim().to_owned();
                pairs.push((key, value));
            }
            None => {
                if let Some((_, value)) = pairs.last_mut() {
                    if !value.is_empty() {
                        value.push(' ');
                    }
                    value.push_str(line.trim());
                } else {
                    // Leading junk before any key: keep it visible rather than
                    // silently dropping it.
                    pairs.push((String::new(), line.trim().to_owned()));
                }
            }
        }
    }

    pairs
}

/// Paint a frontmatter block as a two-column key/value table.
fn render_frontmatter_table(
    ui: &mut Ui,
    raw: &str,
    options: &CommonMarkOptions,
    max_width: f32,
) {
    let pairs = parse_frontmatter_pairs(raw);
    if pairs.is_empty() {
        return;
    }

    let _ = options;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_max_width(max_width);
        egui::Grid::new(ui.next_auto_id())
            .num_columns(2)
            .spacing(egui::vec2(ui.spacing().item_spacing.x, 4.0))
            .striped(true)
            .show(ui, |ui| {
                for (key, value) in pairs {
                    // The gap has to be produced *inside* the key cell: the
                    // grid sizes column one to its widest entry, so a long key
                    // otherwise ends flush against its value ("authorJane Doe").
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(key).strong());
                        ui.add_space(12.0);
                    });
                    ui.label(value);
                    ui.end_row();
                }
            });
    });
}

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

/// Measure a table cell's visible labels in their rendering order. The cell
/// uses a wrapping horizontal layout: every label continues the current row,
/// then uses the full column width for subsequent rows. Long code chunks call
/// `ui.end_row()` in production, so they also finish a measurement row here.
fn cell_visual_lines(
    cell: &[(pulldown_cmark::Event, Range<usize>)],
    ui: &Ui,
    column_width: f32,
) -> usize {
    // `RichText::code().size(selected_font_size)` renders code with the body
    // size and monospace family, not with the (usually smaller) Monospace text
    // style. Keep measurement on that exact font-size path.
    let body_font = egui::TextStyle::Body.resolve(ui.style());
    let code_font = egui::FontId::new(body_font.size, egui::FontFamily::Monospace);
    let wrap_width = (column_width - 8.0).max(1.0);
    let item_spacing = ui.spacing().item_spacing.x;
    let mut remaining_width = wrap_width;
    let mut completed_lines = 0usize;
    let measure_label = |text: &str, font_id: &egui::FontId, used_width: f32| {
        let mut job = egui::text::LayoutJob::simple(
            text.to_owned(),
            font_id.clone(),
            egui::Color32::WHITE,
            wrap_width,
        );
        if let Some(first_section) = job.sections.first_mut() {
            first_section.leading_space = used_width;
        }
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    };

    let add_label = |text: &str,
                     font_id: &egui::FontId,
                     remaining_width: &mut f32,
                     completed_lines: &mut usize| {
        let used_width = wrap_width - *remaining_width;
        let galley = measure_label(text, font_id, used_width);
        *completed_lines += galley.rows.len().saturating_sub(1);
        let last_row_width = galley
            .rows
            .last()
            .map_or(0.0, |row| row.rect().width())
            .min(wrap_width);
        *remaining_width = (wrap_width - last_row_width - item_spacing).max(0.0);
    };

    for (event, _) in cell {
        match event {
            pulldown_cmark::Event::Text(text)
            | pulldown_cmark::Event::InlineHtml(text)
            | pulldown_cmark::Event::Html(text)
            | pulldown_cmark::Event::FootnoteReference(text) => {
                add_label(text, &body_font, &mut remaining_width, &mut completed_lines);
            }
            pulldown_cmark::Event::Code(code) => {
                let segments = inline_code_wrap_segments(code);
                let force_rows = segments.len() > 1;
                for segment in segments {
                    add_label(
                        &segment,
                        &code_font,
                        &mut remaining_width,
                        &mut completed_lines,
                    );
                    if force_rows {
                        completed_lines += 1;
                        remaining_width = wrap_width;
                    }
                }
            }
            pulldown_cmark::Event::SoftBreak | pulldown_cmark::Event::HardBreak => {
                add_label(" ", &body_font, &mut remaining_width, &mut completed_lines);
            }
            pulldown_cmark::Event::TaskListMarker(checked) => {
                add_label(
                    if *checked { "[x] " } else { "[ ] " },
                    &body_font,
                    &mut remaining_width,
                    &mut completed_lines,
                );
            }
            _ => {}
        }
    }

    (completed_lines + usize::from(remaining_width < wrap_width)).max(1)
}

fn table_cell_height(
    cell: &[(pulldown_cmark::Event, Range<usize>)],
    line_height: f32,
    cache: &CommonMarkCache,
    ui: &Ui,
    column_width: f32,
    options: &CommonMarkOptions,
) -> f32 {
    let text = markdown_cell_text(cell);
    let mut height = wrapped_text_height(ui, &text, column_width, line_height).max(
        line_height * cell_visual_lines(cell, ui, column_width) as f32
            + ui.spacing().item_spacing.y,
    );
    let has_visible_text = cell.iter().any(|(event, _)| {
        matches!(
            event,
            pulldown_cmark::Event::Text(_)
                | pulldown_cmark::Event::Code(_)
                | pulldown_cmark::Event::InlineHtml(_)
                | pulldown_cmark::Event::Html(_)
                | pulldown_cmark::Event::FootnoteReference(_)
                | pulldown_cmark::Event::TaskListMarker(_)
        )
    });
    let mut inline_math_height = 0.0;
    let mut inline_math_count = 0usize;
    for (event, _) in cell {
        // Images contribute their painted height. The URI has to be resolved
        // exactly the way the painting path does it (`Image::new` applies the
        // scheme rules), otherwise the cache lookup misses and the row silently
        // stays text-height.
        if let pulldown_cmark::Event::Start(pulldown_cmark::Tag::Image { dest_url, .. }) = event {
            let uri = crate::Image::new(dest_url, options).uri;
            let image_height = match cache.observed_image_size(&uri) {
                // Painted at least once: the width is already capped by the
                // cell, so the recorded height is what the row needs.
                Some(size) if size.y > 0.0 => size.y,
                // Not yet loaded. Reserving nothing collapses the row and the
                // image is clipped on every frame until it loads; reserving a
                // full cell width of height over-reserves for a wide thin
                // image. A square-ish guess bounded by the column keeps the
                // first frame usable, and `observe_image_size` marks the layout
                // dirty once the real size arrives so this is re-measured.
                _ => column_width.min(line_height * 8.0),
            };
            height = height.max(image_height);
        }

        if let pulldown_cmark::Event::InlineMath(_tex) = event {
            let conservative = line_height * 2.0;
            #[cfg(feature = "math")]
            let formula_height = crate::cached_inline_math_height(ui, cache, _tex, options)
                .map(|exact| exact + line_height * 0.5)
                .unwrap_or(conservative);
            #[cfg(not(feature = "math"))]
            let formula_height = {
                let _ = (cache, ui, options);
                conservative
            };
            inline_math_height += formula_height;
            inline_math_count += 1;
        }
    }
    if inline_math_count == 1 && !has_visible_text {
        height = height.max(inline_math_height);
    } else if inline_math_count > 0 {
        // Formula widgets participate in the same horizontal wrapping flow as
        // labels. Without cached formula widths we cannot know whether each
        // formula shares a row, so reserve their heights cumulatively whenever
        // other visible content (or another formula) can force a row break.
        height += inline_math_height
            + (ui.spacing().item_spacing.y + 1.0) * inline_math_count as f32;
    }

    height
}

fn markdown_cell_text(cell: &[(pulldown_cmark::Event, Range<usize>)]) -> String {
    let mut text = String::new();
    for (event, _) in cell {
        match event {
            pulldown_cmark::Event::Text(value)
            | pulldown_cmark::Event::Code(value)
            | pulldown_cmark::Event::InlineHtml(value)
            | pulldown_cmark::Event::Html(value)
            | pulldown_cmark::Event::FootnoteReference(value)
            | pulldown_cmark::Event::InlineMath(value)
            | pulldown_cmark::Event::DisplayMath(value) => text.push_str(value),
            pulldown_cmark::Event::SoftBreak | pulldown_cmark::Event::HardBreak => text.push('\n'),
            pulldown_cmark::Event::TaskListMarker(checked) => {
                text.push_str(if *checked { "[x] " } else { "[ ] " });
            }
            _ => {}
        }
    }
    text
}

fn markdown_table_digest(rows: &[Vec<Vec<(pulldown_cmark::Event, Range<usize>)>>]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for row in rows {
        row.len().hash(&mut hasher);
        for cell in row {
            cell.len().hash(&mut hasher);
            for (event, source) in cell {
                // Preserve formatting identity as well as visible text: the same
                // bytes can wrap differently as prose, inline code, or a link.
                let _ = write!(HasherWriter(&mut hasher), "{event:?}");
                source.start.hash(&mut hasher);
                source.end.hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

struct HasherWriter<'a, H>(&'a mut H);

impl<H: Hasher> std::fmt::Write for HasherWriter<'_, H> {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        self.0.write(value.as_bytes());
        Ok(())
    }
}

fn html_table_digest(rows: &[(bool, &[String])]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (is_header, row) in rows {
        is_header.hash(&mut hasher);
        row.hash(&mut hasher);
    }
    hasher.finish()
}

fn natural_text_width(ui: &Ui, text: &str) -> f32 {
    let font_id = egui::TextStyle::Body.resolve(ui.style());
    text.lines()
        .map(|line| {
            ui.painter()
                .layout_no_wrap(line.to_owned(), font_id.clone(), egui::Color32::WHITE)
                .size()
                .x
        })
        .fold(0.0, f32::max)
        + 16.0
}

/// Width needed to keep the widest Unicode word on one line.
///
/// Body content may wrap aggressively, but a short table header such as
/// `Required` should not be forced into `Require` / `d`. Unicode word
/// boundaries keep CJK headers breakable while treating shaped scripts and
/// combining sequences as words rather than splitting scalar values.
fn unbreakable_text_width(ui: &Ui, text: &str) -> f32 {
    text.unicode_words()
        .map(|word| natural_text_width(ui, word))
        .fold(40.0, f32::max)
}

fn wrapped_text_height(ui: &Ui, text: &str, column_width: f32, line_height: f32) -> f32 {
    let font_id = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout(
        text.to_owned(),
        font_id,
        egui::Color32::WHITE,
        (column_width - 8.0).max(1.0),
    );
    line_height * galley.rows.len().max(1) as f32
}

fn body_line_height(ui: &Ui, options: &CommonMarkOptions) -> f32 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let natural = ui
        .text_style_height(&egui::TextStyle::Body)
        .max(font.size);
    options
        .typography
        .resolve_line_height(font.size)
        // A configured line box can be smaller than the glyphs, but a fixed
        // table row must still reserve their natural painted height.
        .map_or(natural, |configured| configured.max(natural))
}

/// Blend max-min fairness with proportional content demand while fitting the
/// available width. The fair component prevents one outlier from starving its
/// neighbors; the proportional component preserves meaningful differences
/// between wider columns. If hard minimums do not fit, horizontal scrolling
/// remains available.
fn fit_column_widths(desired: &[f32], available: f32, minimums: &[f32]) -> Vec<f32> {
    if desired.is_empty() {
        return Vec::new();
    }
    assert_eq!(desired.len(), minimums.len());
    let minimums: Vec<f64> = minimums
        .iter()
        .map(|width| width.max(40.0) as f64)
        .collect();
    let desired: Vec<f64> = desired
        .iter()
        .zip(&minimums)
        .map(|(width, minimum)| (*width as f64).max(*minimum))
        .collect();
    let available = available as f64;
    let desired_total = desired.iter().sum::<f64>();
    if desired_total <= available {
        return desired.into_iter().map(|width| width as f32).collect();
    }
    let minimum_total = minimums.iter().sum::<f64>();
    if available <= minimum_total {
        return minimums.into_iter().map(|width| width as f32).collect();
    }

    // Reserve every header floor first, then distribute the remaining space
    // fairly across each column's unmet demand. With equal floors this is the
    // original max-min water filling translated by that common floor.
    let headrooms: Vec<f64> = desired
        .iter()
        .zip(&minimums)
        .map(|(wanted, minimum)| wanted - minimum)
        .collect();
    let surplus = available - minimum_total;
    let mut sorted = headrooms.clone();
    sorted.sort_by(f64::total_cmp);
    let mut remaining = surplus;
    let mut active = sorted.len();
    let mut fair_cap = 0.0;
    for wanted in sorted {
        let equal_share = remaining / active as f64;
        if wanted <= equal_share {
            remaining -= wanted;
            active -= 1;
            fair_cap = wanted;
        } else {
            fair_cap = equal_share;
            break;
        }
    }

    // Proportional allocation above the hard minimum is continuous and keeps
    // meaningful differences in unmet demand. Blend mostly toward fairness so
    // a very large outlier cannot dominate.
    const FAIR_WEIGHT: f64 = 0.6;
    let proportional_scale = surplus / (desired_total - minimum_total);

    minimums
        .into_iter()
        .zip(headrooms)
        .map(|(minimum, headroom)| {
            let fair = minimum + headroom.min(fair_cap);
            let proportional = minimum + proportional_scale * headroom;
            (FAIR_WEIGHT * fair + (1.0 - FAIR_WEIGHT) * proportional) as f32
        })
        .collect()
}

#[derive(Clone, Debug)]
struct HeightAwareTableLayout {
    key: u64,
    widths: Vec<f32>,
}

#[derive(Clone, Copy, Debug)]
struct TableHeightScore {
    row_max_total: f32,
    cell_total: f32,
}

fn table_height_score(
    widths: &[f32],
    row_count: usize,
    measurements: &mut HashMap<(usize, u32), Vec<f32>>,
    measurement_limit: usize,
    measure_column: &mut impl FnMut(usize, f32) -> Vec<f32>,
) -> Option<TableHeightScore> {
    let mut row_maxima = vec![0.0_f32; row_count];
    let mut cell_total = 0.0;
    for (column, &width) in widths.iter().enumerate() {
        let measurement_key = (column, width.to_bits());
        if !measurements.contains_key(&measurement_key)
            && measurements.len() >= measurement_limit
        {
            return None;
        }
        let heights = measurements
            .entry(measurement_key)
            .or_insert_with(|| measure_column(column, width));
        for (row, &height) in heights.iter().enumerate().take(row_count) {
            row_maxima[row] = row_maxima[row].max(height);
            cell_total += height;
        }
    }
    Some(TableHeightScore {
        row_max_total: row_maxima.into_iter().sum(),
        cell_total,
    })
}

fn reduces_table_height(candidate: TableHeightScore, current: TableHeightScore) -> bool {
    const MEANINGFUL_HEIGHT: f32 = 0.25;
    candidate.row_max_total < current.row_max_total - MEANINGFUL_HEIGHT
}

fn better_table_height(candidate: TableHeightScore, current: TableHeightScore) -> bool {
    const MEANINGFUL_HEIGHT: f32 = 0.25;
    reduces_table_height(candidate, current)
        || ((candidate.row_max_total - current.row_max_total).abs() <= MEANINGFUL_HEIGHT
            && candidate.cell_total < current.cell_total - MEANINGFUL_HEIGHT)
}

fn columns_at_row_max(
    widths: &[f32],
    row_count: usize,
    measurements: &HashMap<(usize, u32), Vec<f32>>,
) -> Vec<bool> {
    const MEANINGFUL_HEIGHT: f32 = 0.25;
    let mut row_maxima = vec![0.0_f32; row_count];
    for (column, width) in widths.iter().enumerate() {
        let heights = &measurements[&(column, width.to_bits())];
        for (row, height) in heights.iter().enumerate().take(row_count) {
            row_maxima[row] = row_maxima[row].max(*height);
        }
    }

    widths
        .iter()
        .enumerate()
        .map(|(column, width)| {
            measurements[&(column, width.to_bits())]
                .iter()
                .enumerate()
                .take(row_count)
                .any(|(row, height)| *height >= row_maxima[row] - MEANINGFUL_HEIGHT)
        })
        .collect()
}

/// Refine the deterministic natural-width allocation using the row heights
/// that the production table renderer will actually reserve. Width moves are
/// bounded and quantized, so the work stays predictable even for large tables.
fn optimize_fitted_widths(
    baseline: &[f32],
    desired: &[f32],
    minimums: &[f32],
    row_count: usize,
    mut measure_column: impl FnMut(usize, f32) -> Vec<f32>,
) -> Vec<f32> {
    const WIDTH_STEP: f32 = 8.0;
    const MAX_PASSES: usize = 8;
    const MAX_PAIR_STEPS: usize = 8;
    const MAX_MEASURED_CELLS: usize = 4_096;

    if baseline.len() < 2
        || baseline.len() != desired.len()
        || baseline.len() != minimums.len()
        || row_count == 0
    {
        return baseline.to_vec();
    }

    let can_transfer = (0..baseline.len()).any(|donor| {
        baseline[donor] > minimums[donor] + 0.01
            && (0..baseline.len()).any(|receiver| {
                receiver != donor
                    && baseline[receiver] + 0.01
                        < desired[receiver].max(minimums[receiver])
            })
    });
    if !can_transfer {
        return baseline.to_vec();
    }

    let mut widths = baseline.to_vec();
    let mut measurements = HashMap::new();
    let measurement_limit = MAX_MEASURED_CELLS / row_count;
    if measurement_limit < baseline.len() {
        return baseline.to_vec();
    }
    let mut score = table_height_score(
        &widths,
        row_count,
        &mut measurements,
        measurement_limit,
        &mut measure_column,
    )
    .expect("the baseline fits the checked measurement budget");
    let mut best_widths = widths.clone();
    let mut used_neutral_move = false;

    for _ in 0..MAX_PASSES {
        // Widening a column that is below every current row maximum cannot
        // reduce table height. Recompute after each accepted move so a column
        // that becomes the new maximum remains eligible on the next pass.
        let relevant_receivers = columns_at_row_max(&widths, row_count, &measurements);
        let mut best: Option<(TableHeightScore, usize, usize, f32)> = None;
        for donor in 0..widths.len() {
            let donor_room = widths[donor] - minimums[donor];
            if donor_room <= 0.01 {
                continue;
            }
            for receiver in 0..widths.len() {
                if donor == receiver || !relevant_receivers[receiver] {
                    continue;
                }
                let receiver_room = desired[receiver].max(minimums[receiver]) - widths[receiver];
                let max_transfer = donor_room.min(receiver_room);
                if max_transfer <= 0.01 {
                    continue;
                }

                // Wrapping height is a staircase: one quantum can sit on a
                // flat section even though a later quantum removes a line.
                // Check every 8 px step through the bounded 64 px window.
                for step in 1..=MAX_PAIR_STEPS {
                    let transfer = WIDTH_STEP * step as f32;
                    if transfer > max_transfer + 0.01 {
                        break;
                    }
                    let mut candidate_widths = widths.clone();
                    candidate_widths[donor] -= transfer;
                    candidate_widths[receiver] += transfer;
                    let Some(candidate) = table_height_score(
                        &candidate_widths,
                        row_count,
                        &mut measurements,
                        measurement_limit,
                        &mut measure_column,
                    ) else {
                        return best_widths;
                    };
                    if !better_table_height(candidate, score) {
                        continue;
                    }
                    let replace =
                        best.is_none_or(|(best_score, best_donor, best_receiver, _)| {
                            better_table_height(candidate, best_score)
                                || (!better_table_height(best_score, candidate)
                                    && (donor, receiver) < (best_donor, best_receiver))
                        });
                    if replace {
                        best = Some((candidate, donor, receiver, transfer));
                    }
                }
            }
        }

        let Some((next_score, donor, receiver, transfer)) = best else {
            break;
        };
        let reduces_rows = reduces_table_height(next_score, score);
        // Cross at most one flat step (enough for two tied row maxima). If the
        // following move still does not reduce the sum of row maxima, stop and
        // return the last primary improvement. Exhausting the measurement
        // budget follows the same rollback path above. Wider plateaus remain
        // deliberately out of scope for this bounded Phase 1 heuristic.
        if !reduces_rows && used_neutral_move {
            break;
        }
        widths[donor] -= transfer;
        widths[receiver] += transfer;
        score = next_score;
        if reduces_rows {
            best_widths.clone_from(&widths);
            used_neutral_move = false;
        } else {
            used_neutral_move = true;
        }
    }
    best_widths
}

fn table_layout_key(
    ui: &Ui,
    desired: &[f32],
    minimums: &[f32],
    table_bound: f32,
    line_height: f32,
    content_digest: u64,
    layout_revision: u64,
    math_scale: f32,
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    desired.len().hash(&mut hasher);
    for width in desired {
        width.to_bits().hash(&mut hasher);
    }
    for width in minimums {
        width.to_bits().hash(&mut hasher);
    }
    table_bound.round().to_bits().hash(&mut hasher);
    line_height.to_bits().hash(&mut hasher);
    let body_font = egui::TextStyle::Body.resolve(ui.style());
    let monospace_font = egui::TextStyle::Monospace.resolve(ui.style());
    body_font.size.to_bits().hash(&mut hasher);
    let _ = write!(HasherWriter(&mut hasher), "{:?}", body_font.family);
    monospace_font.size.to_bits().hash(&mut hasher);
    let _ = write!(HasherWriter(&mut hasher), "{:?}", monospace_font.family);
    ui.ctx().pixels_per_point().to_bits().hash(&mut hasher);
    content_digest.hash(&mut hasher);
    layout_revision.hash(&mut hasher);
    math_scale.to_bits().hash(&mut hasher);
    hasher.finish()
}

fn cached_height_aware_widths(
    ui: &Ui,
    table_id: Id,
    key: u64,
    baseline: &[f32],
    desired: &[f32],
    minimums: &[f32],
    row_count: usize,
    measure_column: impl FnMut(usize, f32) -> Vec<f32>,
) -> (Vec<f32>, bool) {
    let cache_id = table_id.with("_height_aware_widths");
    let previous = ui.data(|data| data.get_temp::<HeightAwareTableLayout>(cache_id));
    if let Some(cached) = &previous {
        if cached.key == key {
            return (cached.widths.clone(), false);
        }
    }
    let had_previous_layout = previous.is_some();

    let widths = optimize_fitted_widths(
        baseline,
        desired,
        minimums,
        row_count,
        measure_column,
    );
    ui.data_mut(|data| {
        data.insert_temp(
            cache_id,
            HeightAwareTableLayout {
                key,
                widths: widths.clone(),
            },
        );
    });
    (widths, had_previous_layout)
}

/// Fit columns inside the table's outer width contract.
///
/// The group frame contributes padding and a stroke on both sides. Those are
/// part of the table's visible width, so only the remaining space belongs to
/// columns and their separators.
fn framed_table_widths(
    ui: &Ui,
    desired: &[f32],
    minimums: &[f32],
    table_bound: f32,
) -> (egui::Frame, Vec<f32>) {
    let frame = egui::Frame::group(ui.style());
    let column_space = ui.spacing().item_spacing.x * desired.len().saturating_sub(1) as f32;
    let frame_width = frame.total_margin().sum().x;
    let column_budget = (table_bound - frame_width - column_space).max(0.0);
    let widths = fit_column_widths(desired, column_budget, minimums);
    (frame, widths)
}

/// Remember the width contract used to initialize a resizable table.
///
/// `egui_extras::TableBuilder` deliberately keeps user-resized column widths,
/// which also means later `Column::initial` values are ignored. Invalidate that
/// cached state only when the table's layout bound changes; while the bound is
/// stable, manual column resizing remains intact.
fn table_layout_bound_changed(ui: &Ui, table_id: Id, table_bound: f32) -> bool {
    let state_id = table_id.with("_layout_bound");
    let current = table_bound.max(0.0).round() as usize;
    ui.data_mut(|data| {
        let previous = data.get_temp::<usize>(state_id);
        data.insert_temp(state_id, current);
        previous.is_some_and(|previous| previous != current)
    })
}

/// Redirect Shift+vertical-wheel over a hovered wide-table into its inner
/// horizontal scroll offset. Plain vertical wheel is left untouched so the
/// outer document scroller keeps scrolling the page (this is the behavior
/// users expect; the unconditional redirect from #4 caused #22).
///
/// The Shift modifier acts as an explicit opt-in for sideways table scrolling
/// without dragging the bottom scrollbar. Native horizontal trackpad input is
/// already consumed by `ScrollArea::horizontal()` inside its `.show()` call,
/// so this helper only ever touches the Y delta.
///
/// Edge pass-through: when the table is at either side and the wheel direction
/// would push past the edge, the delta is left for the outer scroller.
fn forward_shift_wheel_to_horizontal_scroll<R>(
    ui: &Ui,
    out: &mut egui::containers::scroll_area::ScrollAreaOutput<R>,
) {
    if !ui.rect_contains_pointer(out.inner_rect) {
        return;
    }
    if !ui.ctx().input(|i| i.modifiers.shift) {
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

/// Diagnostic instrument for issue #140 (intermittent one-frame blank pane).
///
/// Inert unless `MDV_DIAG_SLICE` is set: the variable is read once and cached,
/// so a normal build pays one relaxed bool load per painted slice.
///
/// It reports, for every frame, where the viewport slice is *placed*
/// (`first_end_y`) against the viewport that frame actually shows. That is the
/// shape the issue's candidate path would produce — a slice selected correctly
/// from a stale scroll offset but positioned at its pre-shrink coordinate, so
/// nothing lands on screen.
///
/// It deliberately reports **every** frame rather than only suspicious ones.
/// A probe that prints only on failure cannot distinguish "did not happen"
/// from "was not running", and the value of this instrument is that its
/// silence means something.
///
/// This asserts nothing about the cause. It is an observation channel for the
/// next real reproduction; the product fix belongs to whatever it captures.
fn diag_report_slice(first_end_y: f32, viewport_top: f32, viewport_bottom: f32, events: usize) {
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    if !*ENABLED.get_or_init(|| std::env::var_os("MDV_DIAG_SLICE").is_some()) {
        return;
    }
    // A slice legitimately starts above the viewport: it resumes at the last
    // complete block before it, which on a long document can be far up. Only a
    // start below the viewport, or absurdly far above it, indicates the slice
    // was placed outside what the frame shows.
    const IMPLAUSIBLE_LEAD: f32 = 20_000.0;
    let off_screen = first_end_y > viewport_bottom || first_end_y < viewport_top - IMPLAUSIBLE_LEAD;
    eprintln!(
        "DIAG slice first_end_y={first_end_y:.0} viewport=[{viewport_top:.0},{viewport_bottom:.0}] \
events={events}{}",
        if off_screen { " OFF-SCREEN" } else { "" }
    );
}

fn markdown_table_id(source_id: Id, source_start: usize) -> Id {
    source_id.with("_markdown_table").with(source_start)
}

fn content_relative_y(screen_y: f32, render_origin_y: f32, slice_start_y: f32) -> f32 {
    slice_start_y + screen_y - render_origin_y
}

fn record_active_search_content_y(
    cache: &mut CommonMarkCache,
    screen_y: f32,
    render_origin_y: f32,
    slice_start_y: f32,
) {
    cache.record_active_search_content_y(content_relative_y(
        screen_y,
        render_origin_y,
        slice_start_y,
    ));
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
    source_id: Option<Id>,
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
    current_heading_source_start: Option<usize>,
    current_heading_text: String,
    /// Accumulate heading RichText fragments for single render at end
    current_heading_rich_texts: Vec<egui::RichText>,
    /// Content-space Y of the first event rendered by the current slice.
    slice_start_y: f32,
    /// Screen-space Y of the root UI for the current full or sliced render.
    /// Nested table/list/blockquote UIs must not replace this origin when
    /// converting navigation positions into document coordinates.
    render_origin_y: f32,
    /// Raw text of the frontmatter block being collected. `Some` only between
    /// `Tag::MetadataBlock` and its end, so ordinary text is unaffected.
    frontmatter: Option<String>,
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
            source_id: None,
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
            frontmatter: None,
            current_heading_y: None,
            current_heading_source_start: None,
            current_heading_text: String::new(),
            current_heading_rich_texts: Vec::new(),
            slice_start_y: 0.0,
            render_origin_y: 0.0,
        }
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
    // Width drives wrap and is the dominant layout input. Quantize to the
    // nearest pixel so sub-pixel float jitter from per-frame egui rounding
    // (very common during image/font async loading) doesn't invalidate the
    // cache. Real width changes (resize, zoom) flip the int bucket; tiny
    // float fluctuations don't.
    (ui.available_width().round() as i32).hash(&mut h);
    // Body / monospace text heights — quantize to 0.1 px for the same
    // reason. A real font/zoom change shifts heights by multiple px; sub-
    // pixel rounding from per-frame ppp resolution stays in one bucket.
    ((ui.text_style_height(&egui::TextStyle::Body) * 10.0).round() as i32).hash(&mut h);
    ((ui.text_style_height(&egui::TextStyle::Monospace) * 10.0).round() as i32).hash(&mut h);
    // Theme doesn't change widget heights, but it does change the resolved
    // syntect theme — invalidating here keeps split_points and the syntect
    // cache (added later) coherent.
    ui.style().visuals.dark_mode.hash(&mut h);
    // Caller-configured constraints that affect block widths.
    options.default_width.hash(&mut h);
    options.indentation_spaces.hash(&mut h);
    // Formula size changes rendered formula extents, so split_points measured
    // at the old scale are stale. Quantized like the heights above.
    ((options.math_scale * 100.0).round() as i32).hash(&mut h);
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
/// The approach: real LaTeX math contains structural syntax (backslash
/// commands, braces, sub/superscripts) OR math operators/grouping (`= < >`,
/// parens, brackets) OR is a signed number / short variable. Anything with
/// none of those markers is almost certainly a misparse. Additionally, very
/// long "math" containing multiple English words is almost certainly a false
/// positive from `$` being used as currency.
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

    // Relational / grouping operators that a closed `$...$` currency amount
    // never contains: `w(z)`, `f(R)`, `D>0`, `p=P`, `[-1.1,-1.0]`, `=0`.
    // Their presence means real math, not a `$5`-style misparse.
    if trimmed.contains(|c: char| matches!(c, '=' | '<' | '>' | '(' | ')' | '[' | ']')) {
        return false;
    }

    // A signed number is math (`-1.38`, `+2.6`); closed currency is unsigned
    // (`$5`, never `$-5$`). Leading `+`/`-` followed by a digit.
    let mut leading = trimmed.chars();
    if matches!(leading.next(), Some('+' | '-'))
        && leading.next().is_some_and(|c| c.is_ascii_digit())
    {
        return false;
    }

    // A short all-letters token is a variable name (`G`, `w`, `D`).
    if trimmed.len() <= 3 && trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }

    // A clean numeric literal with no internal whitespace is an intentional
    // `$number$` (e.g. a χ² value `$8.5$`), not currency. Currency only reaches
    // InlineMath by spanning two `$` across prose, so its mis-parsed content
    // carries spaces or dashes (`"8.5 to "`, `"3,000–"`) — which fail this test
    // and fall through to the currency branch below.
    if trimmed.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',')
        && trimmed.chars().any(|c| c.is_ascii_digit())
    {
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

/// Find source-visible references that the host registered as local links.
/// The host can therefore restrict auto-linking to paths that actually exist.
fn registered_auto_link_ranges(
    text: &str,
    hooks: &std::collections::HashMap<String, bool>,
) -> Vec<(Range<usize>, String)> {
    let is_path_char = |ch: char| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/');
    let mut matches = Vec::new();

    for destination in hooks.keys() {
        if destination.is_empty() || destination.starts_with('#') {
            continue;
        }
        for (start, _) in text.match_indices(destination) {
            let end = start + destination.len();
            let left_ok = text[..start].chars().next_back().is_none_or(|ch| !is_path_char(ch));
            let right_ok = text[end..].chars().next().is_none_or(|ch| !is_path_char(ch));
            if left_ok && right_ok {
                matches.push((start..end, destination.clone()));
            }
        }
    }

    // Prefer a longer registered path when two candidates start at the same
    // byte, then discard any remaining overlap.
    matches.sort_by(|(left_range, _), (right_range, _)| {
        left_range
            .start
            .cmp(&right_range.start)
            .then_with(|| right_range.len().cmp(&left_range.len()))
    });
    let mut last_end = 0;
    matches.retain(|(range, _)| {
        if range.start < last_end {
            false
        } else {
            last_end = range.end;
            true
        }
    });
    matches
}

fn registered_exact_auto_link(
    text: &str,
    hooks: &std::collections::HashMap<String, bool>,
) -> Option<String> {
    (!text.is_empty() && !text.starts_with('#') && hooks.contains_key(text))
        .then(|| text.to_owned())
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
        self.slice_start_y = 0.0;
        let max_width = options.max_width(ui);
        let layout = egui::Layout::left_to_right(egui::Align::BOTTOM).with_main_wrap(true);

        // Compute content hash and ensure events are cached
        let content_hash = Self::hash_content(text);
        if cache.get_cached_events(content_hash).is_none() {
            let math_enabled = options.math_fn.is_some() || cfg!(feature = "math");
            // LaTeX-style \(...\) / \[...\] support (#60): delimiters are
            // rewritten pre-parse on an in-memory copy and ranges are mapped
            // back, so they stay valid against the original text.
            let owned_events: Vec<(pulldown_cmark::Event<'static>, Range<usize>)> =
                super::latex_delimiters::parse_events(text, math_enabled, options.render_frontmatter);
            cache.set_cached_events(content_hash, owned_events);
        }

        // Left edge of the scroll area's content ui. The content column's own
        // left edge is recorded relative to this so a viewport slice can be
        // placed at the same column (see `ContentGeometry`).
        let scroll_area_left = ui.max_rect().left();
        let re = ui.allocate_ui_with_layout(egui::vec2(max_width, 0.0), layout, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let height = ui.text_style_height(&TextStyle::Body);
            ui.set_row_height(height);
            let content_origin_y = ui.next_widget_position().y;
            let content_origin_x = ui.max_rect().left();
            self.render_origin_y = content_origin_y;

            // Use cached events — clone the Vec reference data for iteration
            // (events are 'static so this is cheap pointer copies, not re-parsing)
            let events_data = cache.get_cached_events(content_hash)
                .expect("events just cached")
                .to_vec();
            let event_count = events_data.len();
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
                let is_block_end = matches!(
                    &e,
                    pulldown_cmark::Event::End(end) if is_block_end_tag(end)
                );
                // `table()` consumes the complete table, including its End
                // event, from `events`. The outer loop therefore never sees
                // TagEnd::Table and must record that block boundary from its
                // Start event after processing finishes.
                let is_atomic_table = matches!(
                    &e,
                    pulldown_cmark::Event::Start(pulldown_cmark::Tag::Table(_))
                );

                if events.peek().is_none() {
                    self.line.should_end_newline_forced = false;
                }

                self.process_event(ui, &mut events, e, src_span, cache, options, max_width);

                // Defense in depth: only add a split point when we're at a
                // block end (or just consumed an atomic table) AND outside any
                // stateful container (list, table, blockquote). The
                // viewport-skip path in `show_scrollable`
                // recreates the renderer with `CommonMarkViewerInternal::new`
                // each frame, so the transient state of `self.list`,
                // `self.is_table`, and `self.is_blockquote` is *not* replayed
                // when iteration jumps in via `skip(first_event_index)`. A
                // split point inside one of those containers would land
                // iteration mid-state — for lists this fires
                // `List::start_item` on an empty stack and panics
                // (`lib.rs:566 unreachable!()`); for tables / blockquotes it
                // would visually corrupt rendering. The container-state
                // check below must run after `process_event` (above) since
                // that's where the start/end of these containers updates
                // `self.list` / `self.is_table` / `self.is_blockquote`.
                let safe_for_split = (is_block_end || is_atomic_table)
                    && !self.list.is_inside_a_list()
                    && !self.is_table
                    && !self.is_blockquote;

                if let Some(source_id) = split_points_id {
                    if safe_for_split {
                        let scroll_cache = scroll_cache(cache, &source_id);
                        let end_position = ui.next_widget_position();

                        let split_index = if is_atomic_table {
                            events
                                .peek()
                                .map(|(next_index, _)| *next_index)
                                .unwrap_or(event_count)
                        } else {
                            index.saturating_add(1)
                        };
                        let split_point_exists = scroll_cache
                            .split_points
                            .iter()
                            .any(|(i, _, _)| *i == split_index);

                        if !split_point_exists {
                            let relative_start = egui::pos2(
                                start_position.x,
                                (start_position.y - content_origin_y).max(0.0),
                            );
                            let relative_end = egui::pos2(
                                end_position.x,
                                (end_position.y - content_origin_y).max(0.0),
                            );
                            // Resume after this complete block. Starting at
                            // its End tag would omit the matching Start state.
                            scroll_cache.split_points.push((
                                split_index,
                                relative_start,
                                relative_end,
                            ));
                        }
                    }
                }

                if index == 0 {
                    self.line.should_not_start_newline_forced = false;
                }
            }

            if let Some(source_id) = split_points_id {
                let content_height = (ui.next_widget_position().y - content_origin_y).max(0.0);
                let scroll_cache = scroll_cache(cache, &source_id);
                scroll_cache.page_size = Some(egui::vec2(max_width, content_height));
                // Capture the column this pass wrapped at, so slices reproduce
                // it instead of deriving a different width from their own ui.
                scroll_cache.content_geometry = Some(ContentGeometry {
                    width: max_width,
                    left_offset: content_origin_x - scroll_area_left,
                });
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
        force_full_render: bool,
        scroll_source: Option<egui::scroll_area::ScrollSource>,
    ) -> egui::scroll_area::ScrollAreaOutput<()> {
        self.source_id = Some(source_id);
        let available_size = ui.available_size();
        let scroll_id = source_id.with("_scroll_area");
        let layout_sig = compute_layout_signature(ui, options);
        let layout_revision = cache.layout_revision();

        // Ensure parsed events are cached on the ScrollableCache, keyed by a
        // content version. The caller can provide a monotonic version (bumped
        // on every reload) — when omitted we fall back to hashing the content,
        // which still beats reparsing but is O(N) per frame for the hash.
        // The big win either way is avoiding pulldown_cmark::Parser::new_ext +
        // collect on every frame (~52 ms at 100k lines).
        let version = content_version.unwrap_or_else(|| Self::hash_content(text));
        let mut layout_invalidated = false;
        {
            let sc = scroll_cache(cache, &source_id);
            if sc.events.is_empty() || sc.content_version != version {
                layout_invalidated = true;
                // Must mirror `show()`'s `math_enabled` derivation
                // (parsers/pulldown.rs in this file: `options.math_fn.is_some()
                // || cfg!(feature = "math")`). The bootstrap branch below
                // calls `self.show()` which parses again with `cfg!(feature =
                // "math")` included; if our parse here omits it, the two
                // event streams diverge for any document containing `$…$`
                // (currency, regex, env vars). split_points are then indexed
                // off `cache.cached_events` (with-math) but consumed against
                // `sc.events` (without-math), so the viewport-skip path lands
                // iteration at an unrelated event — often `Tag::Item` with no
                // matching `Tag::List` start → `List::start_item` panics
                // (`lib.rs:566 unreachable!()`). See docs/devlog/027.
                let math_enabled =
                    options.math_fn.is_some() || cfg!(feature = "math");
                // Must produce byte-identical events to the cache-fill parse
                // above — including any LaTeX delimiter rewrite (#60) — or
                // split_points index into an unrelated stream (see devlog 027).
                sc.events = super::latex_delimiters::parse_events(text, math_enabled, options.render_frontmatter);
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
                layout_invalidated = true;
                sc.layout_signature = layout_sig;
                sc.page_size = None;
                sc.split_points.clear();
                sc.available_size = available_size;
            }
            if sc.layout_revision != layout_revision {
                layout_invalidated = true;
                sc.layout_revision = layout_revision;
                sc.page_size = None;
                sc.split_points.clear();
            }
            // An unknown navigation target may require painting every event
            // so its precise position can be measured. Scrolling to an
            // already cached Y must not take this path: nested virtualized
            // widgets can report different off-screen heights during a
            // nonzero-offset full paint and overwrite valid coordinates.
            //
            // Keep the already-valid split points: this bootstrap is needed
            // to paint every event for the jump, not to recompute geometry.
            // The push site deduplicates by event index, so the full render
            // can still refresh page_size without rebuilding the split list.
            if force_full_render {
                sc.page_size = None;
            }
        }
        // Header positions are content-keyed; new content means the cached
        // y values point at the wrong headings. Done outside the `sc` borrow
        // scope above so `cache` is reborrowable.
        if layout_invalidated {
            cache.clear_header_positions();
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

        // Bootstrap once after content/layout invalidation. It records safe
        // top-level block boundaries and content-relative positions. Normal
        // frames then paint only the viewport slice between clean boundaries.
        if scroll_cache(cache, &source_id).page_size.is_none() {
            let out = make_scroll_area().show(ui, |ui| {
                cache.set_scroll_offset(pending_scroll_offset.unwrap_or(0.0));
                self.show(ui, cache, options, text, Some(source_id));
            });
            let sc = scroll_cache(cache, &source_id);
            if let Some(page_size) = &mut sc.page_size {
                // The ScrollArea output is the canonical extent. Nested
                // widgets such as tables may advance the inner cursor beyond
                // the space actually allocated by their outer response.
                page_size.y = out.content_size.y;
            }
            sc.available_size = available_size;
            return out;
        }
        let page_size_opt = scroll_cache(cache, &source_id).page_size;
        let Some(page_size) = page_size_opt else {
            unreachable!()
        };

        let num_rows = scroll_cache(cache, &source_id).events.len();

        let out = make_scroll_area().show_viewport(ui, |ui, viewport| {
            ui.set_height(page_size.y);
            // The cursor inside show_viewport is viewport-relative; adding
            // this offset recovers content-relative heading/search positions.
            cache.set_scroll_offset(viewport.min.y);
            let layout = egui::Layout::left_to_right(egui::Align::BOTTOM).with_main_wrap(true);
            // Lay the slice out at the column the bootstrap pass measured.
            // Deriving it from this ui instead yields a different available
            // width — the bootstrap and viewport passes reserve scrollbar
            // space differently — so the slice wrapped at a different column
            // than the pass that produced `page_size` and `split_points`.
            let recorded_geometry = scroll_cache(cache, &source_id).content_geometry;
            let max_width = recorded_geometry
                .map(|geometry| geometry.width)
                .unwrap_or_else(|| options.max_width(ui));
            let content_left =
                ui.max_rect().left() + recorded_geometry.map_or(0.0, |g| g.left_offset);

            let (first_event_index, first_end_y, events_range,
                 diag_viewport_min_y, diag_viewport_max_y) = {
                let scroll_cache = scroll_cache(cache, &source_id);

                // Resume after the last complete block above the viewport.
                // Re-rendering an additional fully off-screen table here is
                // unsafe: egui_extras virtualizes all of its heterogeneous
                // rows and can report a collapsed height for that table.
                let above = scroll_cache
                    .split_points
                    .partition_point(|(_, _, end)| end.y < viewport.min.y);
                let (first_event_index, _, first_end_position) = if above >= 1 {
                    scroll_cache.split_points[above - 1]
                } else {
                    (0, Pos2::ZERO, Pos2::ZERO)
                };

                let below = scroll_cache
                    .split_points
                    .partition_point(|(_, start, _)| start.y <= viewport.max.y);
                let last_split = scroll_cache.split_points.get(below + 1);
                let last_event_index = last_split
                    .map(|(index, _, _)| *index)
                    .unwrap_or(num_rows);

                let range_end = last_event_index.min(scroll_cache.events.len());
                let events_range = if first_event_index < range_end {
                    scroll_cache.events[first_event_index..range_end].to_vec()
                } else {
                    Vec::new()
                };

                (first_event_index, first_end_position.y, events_range,
                 viewport.min.y, viewport.max.y)
            };

            // Match egui's show_rows strategy: size the parent to the full
            // document, then place only the visible slice in an absolute child.
            //
            // The rect is zero-height on purpose, mirroring the bootstrap's
            // `allocate_ui_with_layout(vec2(max_width, 0.0), ..)`: the child
            // grows downward from `slice_top` with its content. Bounding it at
            // the slice's recorded end instead made content that needed more
            // room than the bound overflow the ui, which inflated the reported
            // extent and let the scroll offset run past the real document.
            let content_top = ui.max_rect().top();
            let slice_top = content_top + first_end_y;
            diag_report_slice(first_end_y, diag_viewport_min_y, diag_viewport_max_y, events_range.len());

            let slice_rect = egui::Rect::from_min_size(
                egui::pos2(content_left, slice_top),
                egui::vec2(max_width, 0.0),
            );

            ui.scope_builder(
                egui::UiBuilder::new().max_rect(slice_rect).layout(layout),
                |ui| {
                    self.slice_start_y = first_end_y;
                    self.render_origin_y = ui.min_rect().top();
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.set_row_height(ui.text_style_height(&TextStyle::Body));

                    let mut events = events_range
                        .into_iter()
                        .enumerate()
                        .map(|(offset, event)| (offset + first_event_index, event))
                        .peekable();

                    while let Some((index, (event, src_span))) = events.next() {
                        if events.peek().is_none() {
                            self.line.should_end_newline_forced = false;
                        }
                        self.process_event(
                            ui,
                            &mut events,
                            event,
                            src_span,
                            cache,
                            options,
                            max_width,
                        );
                        // Mirror the bootstrap pass, which clears this after
                        // its own first event. A slice starting part-way into
                        // the document never sees index 0, so the flag stayed
                        // set and the slice's first block did not open its own
                        // row — the block was placed after the leading inline
                        // space instead, which is why a table rendered
                        // horizontally offset from where the bootstrap
                        // measured it.
                        if index == first_event_index {
                            self.line.should_not_start_newline_forced = false;
                        }
                    }
                },
            );
        });
        // The absolutely positioned slice preserves the bootstrap content extent.

        // Scroll-overshoot clamp.
        let real_max_scroll = (page_size.y - out.inner_rect.height()).max(0.0);
        let clamped = out.state.offset.y > real_max_scroll;
        if clamped {
            let mut state = out.state;
            state.offset.y = real_max_scroll;
            state.store(ui.ctx(), out.id);
            ui.ctx().request_repaint();
        }
        let _ = clamped;
        out
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
        let table_source_start = matches!(
            &event,
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Table(_))
        )
        .then_some(src_span.start);
        self.event(ui, event, src_span, cache, options, max_width);

        self.def_list_def_wrapping(events, max_width, cache, options, ui);
        self.item_list_wrapping(events, max_width, cache, options, ui);
        self.table(
            events,
            cache,
            options,
            ui,
            max_width,
            table_source_start,
        );
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
        source_start: Option<usize>,
    ) {
        if self.is_table {
            self.line.try_insert_start(ui);

            let id = markdown_table_id(
                self.source_id.unwrap_or_else(|| ui.id()),
                source_start.unwrap_or(self.curr_table),
            );
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
            let line_h = body_line_height(ui, options);

            if num_cols == 0 {
                self.is_table = false;
                if events.peek().is_none() {
                    self.line.should_end_newline_forced = false;
                }
                self.line.try_insert_end(ui);
                return;
            }
            let cell_h = line_h + ui.spacing().item_spacing.y;
            // Outer ScrollArea::horizontal handles the case where columns
            // (auto-sized to content) total wider than the parent ui; without it,
            // narrow windows clip the rightmost columns. Plain vertical wheel
            // remains with the outer document scroller (#22); Shift+wheel opts in
            // to horizontal table scrolling via `forward_shift_wheel_to_horizontal_scroll`.
            // ui.vertical(...) is essential: TableBuilder's body() positions itself
            // relative to the parent's cursor, but the parent here is a horizontal-
            // flow Ui from the markdown renderer. Without the vertical scope the
            // body's first row overlaps the header row.
            //
            // The caller chooses whether `table_max_width` is the capped reading
            // width or the full content pane. Horizontal scrolling keeps columns
            // reachable when their minimum widths exceed that bound (#64, #110).
            let table_bound = options
                .table_max_width
                .map(|w| w as f32)
                .unwrap_or(max_width);
            let minimum_widths: Vec<f32> = (0..num_cols)
                .map(|column| {
                    header
                        .get(column)
                        .map(|cell| unbreakable_text_width(ui, &markdown_cell_text(cell)))
                        .unwrap_or(40.0)
                })
                .collect();
            let mut table_rows = Vec::with_capacity(rows.len() + usize::from(!header.is_empty()));
            if !header.is_empty() {
                table_rows.push(header);
            }
            table_rows.extend(rows);
            let desired_widths: Vec<f32> = (0..num_cols)
                .map(|column| {
                    table_rows
                        .iter()
                        .filter_map(|row| row.get(column))
                        .map(|cell| natural_text_width(ui, &markdown_cell_text(cell)))
                        .fold(40.0, f32::max)
                })
                .collect();
            let (table_frame, baseline_widths) =
                framed_table_widths(ui, &desired_widths, &minimum_widths, table_bound);
            let layout_key = table_layout_key(
                ui,
                &desired_widths,
                &minimum_widths,
                table_bound,
                line_h,
                markdown_table_digest(&table_rows),
                cache.layout_revision(),
                options.math_scale,
            );
            let (initial_widths, height_layout_changed) = cached_height_aware_widths(
                ui,
                id,
                layout_key,
                &baseline_widths,
                &desired_widths,
                &minimum_widths,
                table_rows.len(),
                |column, width| {
                    table_rows
                        .iter()
                        .map(|row| {
                            row.get(column).map_or(0.0, |cell| {
                                table_cell_height(cell, line_h, cache, ui, width, options)
                            })
                        })
                        .collect()
                },
            );
            // The document ui is allocated at the prose width, and a child ui
            // can never exceed its parent's allocation — so a wider viewport
            // must be carved out explicitly. egui does not clamp an explicit
            // child max_rect to the parent, which lets the table scope span
            // the full pane while prose keeps the reading width. Anchor at
            // the current cursor, not max_rect, or the table would repaint on
            // top of everything above it.
            let mut table_scope_rect = ui.cursor();
            table_scope_rect.max.x = table_scope_rect.min.x + table_bound;
            // Reserve the table's own height rather than "everything below the
            // cursor".
            //
            // egui_extras accounts for the rows it skips only once its loop
            // reaches the first *visible* row (`heterogeneous_rows` ->
            // `add_buffer`). A table lying entirely above the visible range
            // never gets there and reserves nothing, collapsing to zero height.
            // Every block below it then lays out too high, so a paint made
            // while scrolled measures the document shorter — and any position
            // recorded during such a paint (heading y for outline clicks) is
            // compressed by the same factor.
            let reserved_row_heights: Vec<f32> = table_rows
                .iter()
                .map(|row| {
                    row.iter()
                        .enumerate()
                        .map(|(column, cell)| {
                            table_cell_height(
                                cell,
                                line_h,
                                cache,
                                ui,
                                initial_widths.get(column).copied().unwrap_or(40.0),
                                options,
                            )
                        })
                        .fold(cell_h, f32::max)
                })
                .collect();
            let group_frame = egui::Frame::group(ui.style());
            let reserved_height = reserved_row_heights.iter().sum::<f32>()
                + ui.spacing().item_spacing.y
                    * reserved_row_heights.len().saturating_sub(1) as f32
                + f32::from(group_frame.inner_margin.top)
                + f32::from(group_frame.inner_margin.bottom)
                + group_frame.stroke.width * 2.0;
            table_scope_rect.max.y = table_scope_rect.min.y + reserved_height;
            let _ = ui
                .scope_builder(egui::UiBuilder::new().max_rect(table_scope_rect), |ui| {
                    let mut scroll_out = egui::ScrollArea::horizontal()
                        .id_salt(id.with("_scroll"))
                        .max_width(table_bound)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                table_frame.show(ui, |ui| {
                                    let reset_column_widths =
                                        table_layout_bound_changed(ui, id, table_bound)
                                            || height_layout_changed;
                                    let mut builder = egui_extras::TableBuilder::new(ui)
                                        .id_salt(id.with("_wrapped"))
                                        .striped(true)
                                        .resizable(true)
                                        .vscroll(false)
                                        // Shrink horizontally to the columns' content so a
                                        // table narrower than the panel hugs its columns
                                        // instead of stretching the bordered frame full width
                                        // with an empty gap after the last column (#47). The
                                        // outer ScrollArea still bounds wide tables at
                                        // max_width and provides horizontal scroll.
                                        .auto_shrink([true, true])
                                        .min_scrolled_height(0.0)
                                        .cell_layout(egui::Layout::left_to_right(egui::Align::Min));
                                    for (column, width) in initial_widths.into_iter().enumerate() {
                                        builder = builder.column(
                                            egui_extras::Column::initial(width)
                                                .resizable(true)
                                                .clip(true)
                                                .at_least(minimum_widths[column]),
                                        );
                                    }
                                    if reset_column_widths {
                                        builder.reset();
                                    }
                                    builder.body(|mut body| {
                                        let widths = body.widths().to_vec();
                                        let heights: Vec<f32> = {
                                            let measure_ui = body.ui_mut();
                                            table_rows
                                                .iter()
                                                .map(|row| {
                                                    row.iter()
                                                        .enumerate()
                                                        .map(|(column, cell)| {
                                                            table_cell_height(
                                                                cell,
                                                                line_h,
                                                                cache,
                                                                measure_ui,
                                                                widths
                                                                    .get(column)
                                                                    .copied()
                                                                    .unwrap_or(40.0),
                                                                options,
                                                            )
                                                        })
                                                        .fold(cell_h, f32::max)
                                                })
                                                .collect()
                                        };
                                        body.heterogeneous_rows(heights.into_iter(), |mut row_ui| {
                                            let row = &table_rows[row_ui.index()];
                                                for col in row {
                                                    row_ui.col(|ui| {
                                                        ui.style_mut().wrap_mode =
                                                            Some(egui::TextWrapMode::Wrap);
                                                        ui.set_width(ui.max_rect().width());
                                                        egui::Frame::NONE
                                                            .inner_margin(egui::Margin::symmetric(4, 0))
                                                            .show(ui, |ui| {
                                                                let col_w = ui.available_width();
                                                                ui.set_width(col_w);
                                                                // Isolate the wrapping cursor from the
                                                                // preallocated TableBuilder row height.
                                                                // Otherwise egui uses that entire height as
                                                                // the first text line's minimum height and
                                                                // later inline widgets overflow the row.
                                                                ui.horizontal_wrapped(|ui| {
                                                                    for (e, src_span) in col {
                                                                        let tmp_start =
                                                                            std::mem::replace(
                                                                                &mut self
                                                                                    .line
                                                                                    .should_start_newline,
                                                                                false,
                                                                            );
                                                                        let tmp_end =
                                                                            std::mem::replace(
                                                                                &mut self
                                                                                    .line
                                                                                    .should_end_newline,
                                                                                false,
                                                                            );
                                                                        self.event(
                                                                            ui,
                                                                            e.clone(),
                                                                            src_span.clone(),
                                                                            cache,
                                                                            options,
                                                                            col_w,
                                                                        );
                                                                        self.line
                                                                            .should_start_newline =
                                                                            tmp_start;
                                                                        self.line.should_end_newline =
                                                                            tmp_end;
                                                                    }
                                                                });
                                                            });
                                                    });
                                                }
                                        });
                                    });
                                });
                            });
                        });
                    forward_shift_wheel_to_horizontal_scroll(ui, &mut scroll_out);
                    scroll_out
                })
                .inner;
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
            pulldown_cmark::Event::Start(tag) => {
                self.start_tag(ui, tag, src_span.start, options)
            }
            pulldown_cmark::Event::End(tag) => self.end_tag(ui, tag, cache, options, max_width),
            pulldown_cmark::Event::Text(text) => {
                // Inside a frontmatter block the text is metadata, not prose:
                // collect it and paint the whole block at TagEnd instead.
                if let Some(buffer) = self.frontmatter.as_mut() {
                    buffer.push_str(&text);
                } else {
                    self.event_text_with_highlights(text, &src_span, cache, ui, options);
                }
            }
            pulldown_cmark::Event::Code(text) => {
                // A bare local Markdown filename is often written as inline code.
                // Preserve the code styling, but give an exact registered path the
                // same click behavior as its plain-text counterpart.
                let auto_link = self
                    .link
                    .is_none()
                    .then(|| registered_exact_auto_link(&text, cache.link_hooks()))
                    .flatten();
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
                    if let Some(destination) = auto_link.as_ref() {
                        self.link = Some(crate::Link {
                            destination: destination.clone(),
                            text: Vec::new(),
                        });
                    }
                    if let Some(ref span) = interior_span {
                        // Inline code stays source-literal while retaining byte-range highlights.
                        self.event_literal_text_with_highlights(
                            segment.into(),
                            span,
                            cache,
                            ui,
                            options,
                        );
                    } else {
                        self.event_text(segment.into(), ui, options);
                    }
                    if auto_link.is_some() {
                        if let Some(link) = self.link.take() {
                            link.end(ui, cache);
                        }
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
                        if self.is_table {
                            crate::render_math_in_table(ui, cache, &tex, options);
                        } else {
                            crate::render_math(ui, cache, &tex, true, options);
                        }
                    }
                    #[cfg(not(feature = "math"))]
                    if let Some(math_fn) = options.math_fn {
                        math_fn(ui, &tex, true);
                    }
                }
            }
            pulldown_cmark::Event::DisplayMath(tex) => {
                // Display math (`$$…$$`) is a block: force it onto its own line
                // even when the source keeps it in the same paragraph as the
                // preceding text (`obeys\n$$…$$`). Without the breaks it flows to
                // the right of that text and, being taller than a line, gets
                // pushed down by the row's bottom-alignment.
                newline(ui);
                #[cfg(feature = "math")]
                {
                    crate::render_math(ui, cache, &tex, false, options);
                }
                #[cfg(not(feature = "math"))]
                if let Some(math_fn) = options.math_fn {
                    math_fn(ui, &tex, false);
                }
                newline(ui);
            }
        }
    }

    fn event_text(&mut self, text: CowStr, ui: &mut Ui, options: &CommonMarkOptions) {
        self.emit_text(text, None, HighlightKind::None, ui, options);
    }

    /// Render text while optionally retaining a different raw spelling for heading identity.
    fn emit_text(
        &mut self,
        text: CowStr,
        raw_heading_text: Option<&str>,
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
            self.text_style.to_richtext_with_options(ui, &text, options)
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
            self.current_heading_text
                .push_str(raw_heading_text.unwrap_or(&text));
            // Accumulate RichText - will render all at once in end_tag(Heading)
            self.current_heading_rich_texts.push(rich_text);
        } else if self.is_table {
            ui.add(egui::Label::new(rich_text).wrap());
        } else {
            ui.label(rich_text);
        }
    }

    /// Render source-literal text while preserving fine-grained search highlighting.
    fn event_literal_text_with_highlights(
        &mut self,
        text: CowStr,
        span: &Range<usize>,
        cache: &mut CommonMarkCache,
        ui: &mut Ui,
        options: &CommonMarkOptions,
    ) {
        // Emit borrowed slices directly; record captured active Y after cache borrows end.
        let mut active_y = None;
        visit_highlight_segments(
            &text,
            span,
            cache.search_ranges(),
            cache.active_search_range(),
            |segment_text, hl| {
                if hl == HighlightKind::Active {
                    active_y = Some(ui.cursor().top());
                }
                self.emit_text(segment_text.into(), None, hl, ui, options);
            },
        );
        if let Some(y) = active_y {
            record_active_search_content_y(cache, y, self.render_origin_y, self.slice_start_y);
        }
    }

    /// Expand eligible emoji shortcodes, then preserve source-based search semantics.
    fn event_text_with_highlights(
        &mut self,
        text: CowStr,
        span: &Range<usize>,
        cache: &mut CommonMarkCache,
        ui: &mut Ui,
        options: &CommonMarkOptions,
    ) {
        // Existing Markdown links already own their text, while headings and
        // image alt text have specialized accumulation semantics. Auto-link
        // only ordinary visible prose whose bytes map exactly to source.
        if self.link.is_none()
            && self.image.is_none()
            && self.code_block.is_none()
            && self.text_style.heading.is_none()
            && text.len() == span.len()
        {
            let links = registered_auto_link_ranges(&text, cache.link_hooks());
            if !links.is_empty() {
                let mut cursor = 0;
                for (range, destination) in links {
                    if cursor < range.start {
                        self.event_text_segment_with_highlights(
                            (&text[cursor..range.start]).into(),
                            &(span.start + cursor..span.start + range.start),
                            cache,
                            ui,
                            options,
                        );
                    }

                    self.link = Some(crate::Link {
                        destination,
                        text: Vec::new(),
                    });
                    self.event_text_segment_with_highlights(
                        (&text[range.clone()]).into(),
                        &(span.start + range.start..span.start + range.end),
                        cache,
                        ui,
                        options,
                    );
                    if let Some(link) = self.link.take() {
                        link.end(ui, cache);
                    }
                    cursor = range.end;
                }
                if cursor < text.len() {
                    self.event_text_segment_with_highlights(
                        (&text[cursor..]).into(),
                        &(span.start + cursor..span.end),
                        cache,
                        ui,
                        options,
                    );
                }
                return;
            }
        }

        self.event_text_segment_with_highlights(text, span, cache, ui, options);
    }

    /// Expand emoji shortcodes and apply source-based highlights to one plain
    /// or auto-linked visible text segment.
    fn event_text_segment_with_highlights(
        &mut self,
        text: CowStr,
        span: &Range<usize>,
        cache: &mut CommonMarkCache,
        ui: &mut Ui,
        options: &CommonMarkOptions,
    ) {
        if !emoji_expansion_is_eligible(self.image.is_some(), self.code_block.is_some()) {
            self.event_text(text, ui, options);
            return;
        }

        // Emit borrowed source slices and static emoji strings directly.
        let mut active_y = None;
        visit_emoji_text_segments(&text, span, |segment| {
            if segment.replaced {
                // Replacement glyphs are indivisible, but overlap uses raw source range.
                let hl = highlight_for_source_span(
                    &segment.source_range,
                    cache.search_ranges(),
                    cache.active_search_range(),
                );
                if hl == HighlightKind::Active {
                    active_y = Some(ui.cursor().top());
                }
                self.emit_text(segment.rendered.into(), Some(segment.raw), hl, ui, options);
                return;
            }

            // Plain source-preserving segments retain exact highlight splitting.
            visit_highlight_segments(
                segment.rendered,
                &segment.source_range,
                cache.search_ranges(),
                cache.active_search_range(),
                |segment_text, hl| {
                    if hl == HighlightKind::Active {
                        active_y = Some(ui.cursor().top());
                    }
                    self.emit_text(segment_text.into(), None, hl, ui, options);
                },
            );
        });
        if let Some(y) = active_y {
            record_active_search_content_y(cache, y, self.render_origin_y, self.slice_start_y);
        }
    }

    fn start_tag(
        &mut self,
        ui: &mut Ui,
        tag: pulldown_cmark::Tag,
        source_start: usize,
        options: &CommonMarkOptions,
    ) {
        match tag {
            pulldown_cmark::Tag::Paragraph => {
                self.line.try_insert_start(ui);
            }
            pulldown_cmark::Tag::Heading { level, .. } => {
                // End current row to ensure heading starts at left edge
                ui.end_row();
                // Record position BEFORE spacing for scroll navigation
                self.current_heading_y = Some(ui.cursor().top());
                self.current_heading_source_start = Some(source_start);
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
                // List items render in one horizontal_wrapped row; end it before a block widget.
                if self.list.is_inside_a_list() {
                    ui.end_row();
                }

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
            pulldown_cmark::Tag::MetadataBlock(_) => {
                self.line.try_insert_start(ui);
                self.frontmatter = Some(String::new());
            }

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
                    ui.scope_builder(egui::UiBuilder::new().max_rect(heading_rect), |ui| {
                        for rt in rich_texts {
                            ui.label(rt);
                        }
                    });
                }
                // Record under a source-stable key shared with the Outline parser.
                if let Some(y) = self.current_heading_y.take() {
                    if let Some(source_start) = self.current_heading_source_start.take() {
                        let key = egui_commonmark_backend_extended::misc::header_position_key(
                            source_start,
                        );
                        // `y` (== `ui.cursor().top()` at heading start) is a
                        // SCREEN-y coordinate. The click handler uses the
                        // cached value with `ScrollArea::vertical_scroll_offset(N)`,
                        // which interprets N as a CONTENT-y (where 0 is the
                        // top of the ScrollArea's content layout). Subtract the
                        // root render origin, which tracks the current scroll
                        // offset but stays stable across nested table, list,
                        // and blockquote UIs. This cancels out both the panel
                        // chrome and any active scroll offset. A viewport slice
                        // starts at `slice_start_y` rather than document y=0,
                        // so add that origin back before updating the cache.
                        //
                        // Empirical verification on Recent-Changes.md:
                        // - At scroll=0: title cursor=323, render origin=44
                        //   → content_y = 279
                        // - After click to scroll=273: cursor=50,
                        //   render origin=-229 → content_y = 279
                        // - Same heading, same content_y, regardless of scroll
                        //
                        // Previously stored `cur_offset + cursor.y` which gave
                        // 323 (off by 44 = panel chrome height), so scrolling
                        // to (323-50)=273 landed 44 px past the heading.
                        let content_y = content_relative_y(
                            y,
                            self.render_origin_y,
                            self.slice_start_y,
                        );
                        // Always refresh with current layout, not first-paint
                        // value. First-paint pinning produced increasing
                        // overshoot for deeper headers — the first frame
                        // renders before async font fallbacks (Noto) finish
                        // loading; once fonts settle, line widths shrink/grow
                        // by a few px per line, and the cumulative drift
                        // moves every heading's true y by an amount that
                        // scales linearly with its depth in the doc. The
                        // pinned cache then sends outline-clicks to the
                        // stale (under-shot) position. Updating each paint
                        // keeps the click target in sync with the current
                        // rendered layout.
                        cache.record_header_content_y(&key, content_y);
                    }
                }
                self.current_heading_source_start = None;
                self.current_heading_text.clear();
                // Add extra spacing below headings if configured
                heading_end_spacing(ui, &options.typography);
                self.line.try_insert_end(ui);
                self.text_style.heading = None;
            }
            pulldown_cmark::TagEnd::BlockQuote(_) => {}
            pulldown_cmark::TagEnd::CodeBlock => {
                self.end_code_block(ui, cache, options, max_width);

                // Keep any following list-item text below the completed block widget.
                if self.list.is_inside_a_list() {
                    ui.end_row();
                }
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
                    // A linked image has already rendered its image widget and
                    // leaves no text in the surrounding Link. Rendering an
                    // empty Label here resets egui's wrapped-row cursor to the
                    // row start, so the next inline image is painted on top of
                    // the previous one. Empty links have no visible widget to
                    // render; keep the cursor where the image left it.
                    if !link.text.is_empty() {
                        link.end(ui, cache);
                    }
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

            pulldown_cmark::TagEnd::MetadataBlock(_) => {
                if let Some(raw) = self.frontmatter.take() {
                    render_frontmatter_table(ui, &raw, options, max_width);
                    self.line.try_insert_end(ui);
                }
            }

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

        let line_h = body_line_height(ui, options);
        let cell_h = line_h + ui.spacing().item_spacing.y;

        if num_cols == 0 {
            self.line.try_insert_end(ui);
            return;
        }

        // Outer ScrollArea::horizontal handles wide tables that exceed parent width;
        // ui.vertical() prevents the header/body Y-overlap quirk. Plain vertical wheel
        // stays with the outer document scroller (#22); Shift+wheel opts in to
        // horizontal table scrolling via `forward_shift_wheel_to_horizontal_scroll`.
        // Bound at `table_max_width` over the prose cap (#64), same as markdown tables.
        let table_bound = options
            .table_max_width
            .map(|w| w as f32)
            .unwrap_or(max_width);
        let table_rows: Vec<(bool, &[String])> = table
            .header
            .iter()
            .map(|row| (true, row.as_slice()))
            .chain(table.rows.iter().map(|row| (false, row.as_slice())))
            .collect();
        let minimum_widths: Vec<f32> = (0..num_cols)
            .map(|column| {
                table
                    .header
                    .iter()
                    .filter_map(|row| row.get(column))
                    .map(|cell| unbreakable_text_width(ui, cell))
                    .fold(40.0, f32::max)
            })
            .collect();
        let desired_widths: Vec<f32> = (0..num_cols)
            .map(|column| {
                table_rows
                    .iter()
                    .filter_map(|(_, row)| row.get(column))
                    .map(|cell| natural_text_width(ui, cell))
                    .fold(40.0, f32::max)
            })
            .collect();
        let (table_frame, baseline_widths) =
            framed_table_widths(ui, &desired_widths, &minimum_widths, table_bound);
        let layout_key = table_layout_key(
            ui,
            &desired_widths,
            &minimum_widths,
            table_bound,
            line_h,
            html_table_digest(&table_rows),
            0,
            1.0,
        );
        let (initial_widths, height_layout_changed) = cached_height_aware_widths(
            ui,
            id,
            layout_key,
            &baseline_widths,
            &desired_widths,
            &minimum_widths,
            table_rows.len(),
            |column, width| {
                table_rows
                    .iter()
                    .map(|(_, row)| {
                        row.get(column).map_or(0.0, |cell| {
                            wrapped_text_height(ui, cell, width - 16.0, line_h) + 8.0
                        })
                    })
                    .collect()
            },
        );
        // Same reading-column escape as markdown tables (#64): carve out a
        // scope wider than the prose allocation, anchored at the cursor.
        let mut table_scope_rect = ui.cursor();
        table_scope_rect.max.x = table_scope_rect.min.x + table_bound;
        table_scope_rect.max.y = ui.max_rect().bottom();
        let _ = ui
            .scope_builder(egui::UiBuilder::new().max_rect(table_scope_rect), |ui| {
                let mut scroll_out = egui::ScrollArea::horizontal()
                    .id_salt(id.with("_scroll"))
                    .max_width(table_bound)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                ui.vertical(|ui| {
                    table_frame.show(ui, |ui| {
                        let reset_column_widths = table_layout_bound_changed(ui, id, table_bound)
                            || height_layout_changed;
                        let mut builder = egui_extras::TableBuilder::new(ui)
                            .id_salt(id.with("_wrapped"))
                            .striped(true)
                            .resizable(true)
                            .vscroll(false)
                            // Hug columns when narrower than the panel (#47); the
                            // outer ScrollArea still handles wide-table overflow.
                            .auto_shrink([true, true])
                            .min_scrolled_height(0.0)
                            .cell_layout(egui::Layout::left_to_right(egui::Align::Min));
                        for (column, width) in initial_widths.into_iter().enumerate() {
                            builder = builder.column(
                                egui_extras::Column::initial(width)
                                    .resizable(true)
                                    .clip(true)
                                    .at_least(minimum_widths[column]),
                            );
                        }

                        if reset_column_widths {
                            builder.reset();
                        }

                        let render_cell_strong = |ui: &mut Ui, cell: &str| {
                            egui::Frame::NONE
                                .inner_margin(egui::Margin::symmetric(8, 4))
                                .show(ui, |ui| {
                                    ui.strong(cell);
                                });
                        };

                        builder.body(|mut body| {
                            let widths = body.widths().to_vec();
                            let heights: Vec<f32> = {
                                let measure_ui = body.ui_mut();
                                table_rows
                                    .iter()
                                    .map(|(_, row)| {
                                        row.iter()
                                            .enumerate()
                                            .map(|(column, cell)| {
                                                wrapped_text_height(
                                                    measure_ui,
                                                    cell,
                                                    widths.get(column).copied().unwrap_or(40.0)
                                                        - 16.0,
                                                    line_h,
                                                ) + 8.0
                                            })
                                            .fold(cell_h, f32::max)
                                    })
                                    .collect()
                            };
                            body.heterogeneous_rows(heights.into_iter(), |mut row_ui| {
                                let (is_header, row) = table_rows[row_ui.index()];
                                for cell in row {
                                    row_ui.col(|ui| {
                                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                                        if is_header {
                                            render_cell_strong(ui, cell);
                                        } else {
                                            egui::Frame::NONE
                                                .inner_margin(egui::Margin::symmetric(8, 4))
                                                .show(ui, |ui| {
                                                    let rich_text = self
                                                        .text_style
                                                        .to_richtext_with_options(ui, cell, options);
                                                    ui.label(rich_text);
                                                });
                                        }
                                    });
                                }
                            });
                        });
                    });
                });
            });
                forward_shift_wheel_to_horizontal_scroll(ui, &mut scroll_out);
                scroll_out
            })
            .inner;
        self.line.try_insert_end(ui);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn frontmatter_splits_top_level_key_value_pairs() {
        let pairs = parse_frontmatter_pairs(
            "title: My Document\nauthor: Jane Doe\ndate: 2026-08-30\n",
        );
        assert_eq!(
            pairs,
            vec![
                ("title".to_owned(), "My Document".to_owned()),
                ("author".to_owned(), "Jane Doe".to_owned()),
                ("date".to_owned(), "2026-08-30".to_owned()),
            ]
        );
    }

    #[test]
    fn frontmatter_keeps_a_value_containing_a_colon_intact() {
        // Only the *first* colon separates; a URL must not be truncated.
        let pairs = parse_frontmatter_pairs("url: https://example.com/path?a=1\n");
        assert_eq!(
            pairs,
            vec![("url".to_owned(), "https://example.com/path?a=1".to_owned())]
        );
    }

    #[test]
    fn frontmatter_folds_nested_lines_into_the_preceding_value() {
        // Not a YAML parser by design: nested mappings and sequence items are
        // folded into the parent value rather than dropped or mis-split into
        // their own rows.
        let pairs = parse_frontmatter_pairs("nested:\n  key: value\nlist:\n  - first\n  - second\n");
        assert_eq!(
            pairs,
            vec![
                ("nested".to_owned(), "key: value".to_owned()),
                ("list".to_owned(), "- first - second".to_owned()),
            ]
        );
    }

    #[test]
    fn frontmatter_ignores_blank_lines_and_survives_leading_junk() {
        let pairs = parse_frontmatter_pairs("\n\nstray\n\ntitle: X\n");
        assert_eq!(
            pairs,
            vec![
                (String::new(), "stray".to_owned()),
                ("title".to_owned(), "X".to_owned()),
            ]
        );
    }

    #[test]
    fn frontmatter_of_only_blank_lines_yields_nothing_to_paint() {
        assert!(parse_frontmatter_pairs("\n   \n").is_empty());
    }

    use super::*;
    use pulldown_cmark::{Event, Options, Parser, Tag};

    // Snapshot scanner output so ranges and raw/rendered identities stay explicit.
    fn segment_snapshot(text: &str, start: usize) -> Vec<(String, Range<usize>, String, bool)> {
        let mut snapshots = Vec::new();
        visit_emoji_text_segments(text, &(start..start + text.len()), |segment| {
            snapshots.push((
                segment.rendered.to_owned(),
                segment.source_range,
                segment.raw.to_owned(),
                segment.replaced,
            ));
        });
        snapshots
    }

    // Collect highlight output only in tests; production emission stays borrowed.
    fn highlight_snapshot(
        text: &str,
        span: &Range<usize>,
        ranges: &[Range<usize>],
        active: Option<&Range<usize>>,
    ) -> Vec<(String, HighlightKind)> {
        let mut snapshots = Vec::new();
        visit_highlight_segments(text, span, ranges, active, |segment, kind| {
            snapshots.push((segment.to_owned(), kind));
        });
        snapshots
    }

    // Exercise expansion at pulldown event boundaries without invoking egui painting.
    fn expanded_visible_text(markdown: &str) -> String {
        let mut visible = String::new();
        let mut image_depth = 0usize;
        let mut code_block_depth = 0usize;

        for (event, span) in Parser::new_ext(markdown, Options::all()).into_offset_iter() {
            match event {
                Event::Start(Tag::Image { .. }) => image_depth += 1,
                Event::End(pulldown_cmark::TagEnd::Image) => image_depth -= 1,
                Event::Start(Tag::CodeBlock(_)) => code_block_depth += 1,
                Event::End(pulldown_cmark::TagEnd::CodeBlock) => code_block_depth -= 1,
                Event::Text(text)
                    if emoji_expansion_is_eligible(image_depth > 0, code_block_depth > 0) =>
                {
                    visit_emoji_text_segments(&text, &span, |segment| {
                        visible.push_str(segment.rendered);
                    });
                }
                Event::Text(text) | Event::Code(text) => visible.push_str(&text),
                _ => {}
            }
        }
        visible
    }

    #[test]
    fn table_cells_measure_inline_code_with_the_monospace_font() {
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        egui::CentralPanel::default().show(&ctx, |ui| {
            let mut style = ui.style().as_ref().clone();
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
            style
                .text_styles
                .insert(egui::TextStyle::Monospace, egui::FontId::monospace(14.0));
            ui.set_style(style);
            let cache = CommonMarkCache::default();
            let line_height = ui.text_style_height(&egui::TextStyle::Body);
            let code = "iiiiiiiiiiiiiiiiiiiiiiii";
            let body_font = egui::TextStyle::Body.resolve(ui.style());
            let mono_font = egui::FontId::new(body_font.size, egui::FontFamily::Monospace);
            let body_width = ui
                .painter()
                .layout_no_wrap(code.to_owned(), body_font, egui::Color32::WHITE)
                .size()
                .x;
            let mono_width = ui
                .painter()
                .layout_no_wrap(code.to_owned(), mono_font, egui::Color32::WHITE)
                .size()
                .x;
            assert!(
                mono_width > body_width,
                "expected monospace code to be wider: body={body_width} mono={mono_width}"
            );

            // `wrapped_text_height` subtracts 8 px before layout. Choose a
            // width where Body still fits but the rendered monospace code
            // must wrap, exposing the old one-line row-height estimate.
            let column_width = (body_width + mono_width) * 0.5 + 8.0;
            let cell = vec![(Event::Code(code.into()), 0..code.len())];
            let options = CommonMarkOptions::default();

            let height =
                table_cell_height(&cell, line_height, &cache, ui, column_width, &options);
            assert!(height >= line_height * 2.0);
            assert!(
                height < line_height * 3.0,
                "two visual lines should not reserve a third: {height}"
            );
        });
        let _ = ctx.end_pass();
    }

    #[test]
    fn table_body_line_height_respects_typography_configuration() {
        use egui_commonmark_backend_extended::typography::Measurement;

        egui::__run_test_ui(|ui| {
            let font = egui::TextStyle::Body.resolve(ui.style());
            let natural = ui
                .text_style_height(&egui::TextStyle::Body)
                .max(font.size);
            let mut options = CommonMarkOptions::default();

            assert!((body_line_height(ui, &options) - natural).abs() < 0.01);

            options.typography.line_height = Some(Measurement::Multiplier(1.5));
            assert!((body_line_height(ui, &options) - font.size * 1.5).abs() < 0.01);

            options.typography.line_height = Some(Measurement::Pixels(27.0));
            assert!((body_line_height(ui, &options) - 27.0).abs() < 0.01);

            options.typography.line_height = Some(Measurement::Pixels(1.0));
            let clamped = body_line_height(ui, &options);
            assert!(
                (clamped - natural).abs() < 0.01,
                "configured={clamped} natural={natural}"
            );
        });
    }

    #[test]
    fn table_cells_accumulate_rows_from_multiple_chunked_code_events() {
        egui::__run_test_ui(|ui| {
            let first = "a".repeat(60);
            let second = "b".repeat(60);
            let cell = vec![
                (Event::Code(first.into()), 0..60),
                (Event::Text(" between ".into()), 60..69),
                (Event::Code(second.into()), 69..129),
            ];

            assert_eq!(cell_visual_lines(&cell, ui, 2_000.0), 4);
        });
    }

    #[test]
    fn table_cells_measure_short_code_with_the_width_left_by_text() {
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        egui::CentralPanel::default().show(&ctx, |ui| {
            let mut style = ui.style().as_ref().clone();
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
            style
                .text_styles
                .insert(egui::TextStyle::Monospace, egui::FontId::monospace(14.0));
            ui.set_style(style);
            let plain = "iiiiiiiiiiii";
            let code = "iiiiiiiiiiii";
            let body_font = egui::TextStyle::Body.resolve(ui.style());
            let code_font = egui::FontId::new(body_font.size, egui::FontFamily::Monospace);
            let plain_width = ui
                .painter()
                .layout_no_wrap(plain.to_owned(), body_font.clone(), egui::Color32::WHITE)
                .size()
                .x;
            let code_as_body_width = ui
                .painter()
                .layout_no_wrap(code.to_owned(), body_font, egui::Color32::WHITE)
                .size()
                .x;
            let code_width = ui
                .painter()
                .layout_no_wrap(code.to_owned(), code_font, egui::Color32::WHITE)
                .size()
                .x;
            assert!(
                code_width > code_as_body_width,
                "expected code to be wider: body={code_as_body_width} code={code_width}"
            );

            // Body-only estimation fits, and either run fits alone, but the
            // real mixed-font line does not.
            let body_total = plain_width + code_as_body_width;
            let mixed_total = plain_width + code_width;
            let column_width = (body_total + mixed_total) * 0.5 + 8.0;
            let cell = vec![
                (Event::Text(plain.into()), 0..plain.len()),
                (Event::Code(code.into()), plain.len()..plain.len() + code.len()),
            ];

            assert_eq!(cell_visual_lines(&cell, ui, column_width), 2);
        });
        let _ = ctx.end_pass();
    }

    #[test]
    fn table_cells_reserve_extra_height_for_inline_math() {
        egui::__run_test_ui(|ui| {
            let cache = CommonMarkCache::default();
            let options = CommonMarkOptions::default();
            let line_height = ui.text_style_height(&egui::TextStyle::Body);
            let cell = vec![(Event::InlineMath(r"\frac{a}{b}".into()), 0..11)];

            assert!(
                table_cell_height(&cell, line_height, &cache, ui, 120.0, &options)
                    >= line_height * 2.0
            );
        });
    }

    #[test]
    fn table_cell_reserves_height_for_an_image_of_known_size() {
        // The size an image was last painted at is what its row must reserve.
        egui::__run_test_ui(|ui| {
            let mut cache = CommonMarkCache::default();
            let options = CommonMarkOptions::default();
            let line_height = ui.text_style_height(&egui::TextStyle::Body);

            let cell = vec![(
                Event::Start(Tag::Image {
                    link_type: pulldown_cmark::LinkType::Inline,
                    dest_url: "chart.png".into(),
                    title: "".into(),
                    id: "".into(),
                }),
                0..10,
            )];

            let without = table_cell_height(&cell, line_height, &cache, ui, 120.0, &options);

            let uri = crate::Image::new("chart.png", &options).uri;
            cache.observe_image_size_for_test(&uri, egui::vec2(120.0, 400.0));
            let with = table_cell_height(&cell, line_height, &cache, ui, 120.0, &options);

            assert!(
                with >= 400.0,
                "row must reserve the painted image height, got {with}"
            );
            assert!(
                with > without,
                "a known image size must not shrink the reservation ({with} vs {without})"
            );
        });
    }

    #[test]
    fn a_cached_image_size_does_not_leak_into_text_only_cells() {
        // Guards the blast radius: seeding a known image size must change the
        // height of cells that contain that image and nothing else.
        egui::__run_test_ui(|ui| {
            let mut cache = CommonMarkCache::default();
            let options = CommonMarkOptions::default();
            let line_height = ui.text_style_height(&egui::TextStyle::Body);
            let cell = vec![(Event::Text("plain".into()), 0..5)];

            let before = table_cell_height(&cell, line_height, &cache, ui, 120.0, &options);

            let uri = crate::Image::new("chart.png", &options).uri;
            cache.observe_image_size_for_test(&uri, egui::vec2(120.0, 400.0));
            let after = table_cell_height(&cell, line_height, &cache, ui, 120.0, &options);

            assert_eq!(
                before, after,
                "a text-only cell must not change when some image's size becomes known"
            );
        });
    }

    #[test]
    fn fitted_columns_leave_room_for_the_visible_table_frame() {
        egui::__run_test_ui(|ui| {
            let table_bound = 360.0;
            let desired = [400.0, 500.0, 600.0];
            let (frame, widths) =
                framed_table_widths(ui, &desired, &[40.0; 3], table_bound);
            let column_space =
                ui.spacing().item_spacing.x * desired.len().saturating_sub(1) as f32;
            let visible_width =
                widths.iter().sum::<f32>() + column_space + frame.total_margin().sum().x;

            assert!((visible_width - table_bound).abs() < 0.01);
        });
    }

    fn render_resizable_table_widths(
        ctx: &egui::Context,
        table_id: Id,
        table_bound: f32,
        initial_widths: &[f32],
    ) -> Vec<f32> {
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 240.0),
            )),
            ..Default::default()
        });
        let mut rendered_widths = Vec::new();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.set_width(table_bound);
            ui.set_max_width(table_bound);
            let reset_column_widths = table_layout_bound_changed(ui, table_id, table_bound);
            let mut builder = egui_extras::TableBuilder::new(ui)
                .id_salt(table_id.with("_wrapped"))
                .resizable(true);
            for width in initial_widths {
                builder = builder.column(
                    egui_extras::Column::initial(*width)
                        .resizable(true)
                        .at_least(40.0),
                );
            }
            if reset_column_widths {
                builder.reset();
            }
            builder.body(|body| rendered_widths = body.widths().to_vec());
        });
        let _ = ctx.end_pass();
        rendered_widths
    }

    #[test]
    fn resizable_table_state_reflows_when_its_bound_changes() {
        let ctx = egui::Context::default();
        let table_id = Id::new("responsive-table");
        let initial = render_resizable_table_widths(&ctx, table_id, 180.0, &[60.0, 80.0]);
        let same_bound = render_resizable_table_widths(&ctx, table_id, 180.0, &[90.0, 110.0]);
        assert_eq!(same_bound, initial, "stable bounds must retain cached widths");

        let wider = render_resizable_table_widths(&ctx, table_id, 360.0, &[120.0, 160.0]);
        assert!(wider[0] > initial[0] && wider[1] > initial[1]);

        let narrower = render_resizable_table_widths(&ctx, table_id, 140.0, &[40.0, 60.0]);
        assert!(narrower[0] < wider[0] && narrower[1] < wider[1]);
    }

    #[test]
    fn fitted_table_columns_balance_fairness_and_content_demand() {
        let widths = fit_column_widths(&[60.0, 500.0, 120.0], 360.0, &[40.0; 3]);

        assert!((widths.iter().sum::<f32>() - 360.0).abs() < 0.1);
        assert!(widths[1] > widths[2] && widths[2] > widths[0]);
        assert!(widths.iter().all(|width| *width >= 40.0));
    }

    #[test]
    fn fitted_table_columns_keep_minimum_for_horizontal_overflow() {
        assert_eq!(
            fit_column_widths(&[100.0, 200.0, 300.0], 100.0, &[40.0; 3]),
            vec![40.0; 3]
        );
    }

    #[test]
    fn fitted_table_columns_respect_individual_header_floors() {
        let minimums = [40.0, 72.0, 88.0];
        let widths = fit_column_widths(&[100.0, 300.0, 240.0], 300.0, &minimums);

        assert!((widths.iter().sum::<f32>() - 300.0).abs() < 0.1);
        assert!(
            widths
                .iter()
                .zip(minimums)
                .all(|(width, minimum)| *width >= minimum)
        );
    }

    #[test]
    fn header_floors_overflow_instead_of_splitting_words() {
        let minimums = [40.0, 92.0, 96.0];

        assert_eq!(
            fit_column_widths(&[100.0, 300.0, 240.0], 180.0, &minimums),
            minimums
        );
    }

    #[test]
    fn header_floor_uses_the_widest_unicode_word() {
        egui::__run_test_ui(|ui| {
            let required = unbreakable_text_width(ui, "Required");
            let allowed_types = unbreakable_text_width(ui, "Allowed Types");

            assert_eq!(required, natural_text_width(ui, "Required").max(40.0));
            assert_eq!(
                allowed_types,
                natural_text_width(ui, "Allowed")
                    .max(natural_text_width(ui, "Types"))
                    .max(40.0)
            );
        });
    }

    #[test]
    fn fitted_table_columns_leave_compact_tables_at_natural_width() {
        assert_eq!(
            fit_column_widths(&[60.0, 80.0, 120.0], 360.0, &[40.0; 3]),
            vec![60.0, 80.0, 120.0]
        );
    }

    #[test]
    fn fitted_table_columns_normalize_demands_below_the_minimum() {
        let widths = fit_column_widths(&[10.0, 100.0], 120.0, &[40.0; 2]);

        assert!((widths.iter().sum::<f32>() - 120.0).abs() < 0.1);
        assert!(widths[0] >= 40.0 && widths[1] >= 40.0);
        assert!(widths[1] > widths[0]);
    }

    #[test]
    fn fitted_table_columns_weight_space_by_unmet_width() {
        let widths = fit_column_widths(&[200.0, 400.0, 300.0], 600.0, &[40.0; 3]);

        assert!((widths.iter().sum::<f32>() - 600.0).abs() < 0.1);
        assert!(widths[1] > widths[2] && widths[2] > widths[0]);
    }

    #[test]
    fn fitted_table_columns_are_continuous_across_equal_share() {
        let below = fit_column_widths(&[200.0, 400.0, 300.0], 599.9, &[40.0; 3]);
        let above = fit_column_widths(&[200.0, 400.0, 300.0], 600.1, &[40.0; 3]);

        assert!(
            below
                .iter()
                .zip(above)
                .all(|(left, right)| (left - right).abs() < 0.2),
            "column widths jumped across a 0.2 px resize: {below:?}"
        );
    }

    #[test]
    fn fitted_table_columns_keep_outlier_neighbors_ordered() {
        let desired = [60.0, 70.0, 10_000.0];
        let widths = fit_column_widths(&desired, 200.0, &[40.0; 3]);

        assert!((widths.iter().sum::<f32>() - 200.0).abs() < 0.1);
        assert!(widths[2] > widths[1] && widths[1] > widths[0]);
        assert!(
            widths
                .iter()
                .zip(desired)
                .all(|(width, wanted)| *width >= 40.0 && *width <= wanted)
        );
    }

    #[test]
    fn fitted_table_columns_keep_total_with_extreme_outlier() {
        let desired = [60.0, 70.0, 1.0e10];
        let widths = fit_column_widths(&desired, 200.0, &[40.0; 3]);

        assert!((widths.iter().sum::<f32>() - 200.0).abs() < 0.1);
        assert!(widths[2] > widths[1] && widths[1] > widths[0]);
    }

    #[test]
    fn fitted_table_columns_handle_a_single_column() {
        assert_eq!(
            fit_column_widths(&[500.0], 200.0, &[40.0]),
            vec![200.0]
        );
    }

    #[test]
    fn fitted_table_columns_keep_equal_demands_equal() {
        let widths = fit_column_widths(&[300.0, 300.0, 300.0], 600.0, &[40.0; 3]);

        assert!((widths[0] - widths[1]).abs() < 0.01);
        assert!((widths[1] - widths[2]).abs() < 0.01);
    }

    #[test]
    fn height_aware_columns_move_space_to_reduce_wrapping() {
        let baseline = [150.0, 150.0];
        let desired = [200.0, 500.0];
        let widths = optimize_fitted_widths(
            &baseline,
            &desired,
            &[40.0; 2],
            2,
            |column, width| {
                let logical_lengths = if column == 0 {
                    [40.0, 60.0]
                } else {
                    [500.0, 700.0]
                };
                logical_lengths
                    .map(|length| (length / width).ceil() * 20.0)
                    .to_vec()
            },
        );

        assert!(widths[0] < baseline[0]);
        assert!(widths[1] > baseline[1]);
        assert!(
            (widths.iter().sum::<f32>() - baseline.iter().sum::<f32>()).abs() < 0.01
        );
        assert!(widths[0] >= 40.0 && widths[1] <= desired[1]);
    }

    #[test]
    fn height_aware_columns_check_each_eight_pixel_step() {
        let baseline = [100.0, 100.0];
        let widths = optimize_fitted_widths(
            &baseline,
            &[100.0, 200.0],
            &[40.0, 40.0],
            1,
            |column, width| {
                let height = if column == 0 {
                    if width < 76.0 { 100.0 } else { 20.0 }
                } else if width < 124.0 {
                    100.0
                } else {
                    20.0
                };
                vec![height]
            },
        );

        assert_eq!(widths, [76.0, 124.0]);
    }

    #[test]
    fn height_aware_columns_bound_expensive_cell_measurements() {
        let measured_cells = std::cell::Cell::new(0usize);
        let row_count = 1_000;
        let baseline = [100.0, 100.0];
        let widths = optimize_fitted_widths(
            &baseline,
            &[200.0, 300.0],
            &[40.0, 40.0],
            row_count,
            |_, _| {
                measured_cells.set(measured_cells.get() + row_count);
                vec![20.0; row_count]
            },
        );

        assert_eq!(widths, baseline);
        assert!(measured_cells.get() <= 4_096);
    }

    #[test]
    fn height_aware_columns_are_deterministic_when_scores_tie() {
        let baseline = [100.0, 100.0, 100.0];
        let desired = [300.0, 300.0, 300.0];
        let measure = |_: usize, _: f32| vec![20.0, 20.0];

        let first = optimize_fitted_widths(&baseline, &desired, &[40.0; 3], 2, measure);
        let second = optimize_fitted_widths(&baseline, &desired, &[40.0; 3], 2, measure);

        assert_eq!(first, baseline);
        assert_eq!(second, baseline);
    }

    #[test]
    fn height_aware_columns_skip_measurement_when_no_transfer_is_possible() {
        let compact = optimize_fitted_widths(
            &[80.0, 120.0],
            &[80.0, 120.0],
            &[40.0, 40.0],
            2,
            |_, _| panic!("compact natural widths do not need height optimization"),
        );
        let all_at_minimum = optimize_fitted_widths(
            &[60.0, 80.0],
            &[200.0, 300.0],
            &[60.0, 80.0],
            2,
            |_, _| panic!("columns at their floors cannot donate width"),
        );

        assert_eq!(compact, [80.0, 120.0]);
        assert_eq!(all_at_minimum, [60.0, 80.0]);
    }

    #[test]
    fn height_aware_columns_keep_baseline_without_a_row_height_reduction() {
        let baseline = [100.0, 100.0];
        let desired = [200.0, 200.0];
        let minimums = [40.0, 40.0];
        let widths = optimize_fitted_widths(
            &baseline,
            &desired,
            &minimums,
            1,
            |column, width| {
                if column == 0 {
                    vec![if width >= 108.0 { 20.0 } else { 40.0 }]
                } else {
                    vec![100.0]
                }
            },
        );

        assert_eq!(widths, baseline);
    }

    #[test]
    fn height_aware_columns_cross_one_flat_step_to_reduce_tied_row_maxima() {
        let baseline = [100.0, 100.0, 100.0];
        let desired = [200.0, 200.0, 100.0];
        let minimums = [40.0, 40.0, 40.0];
        let widths = optimize_fitted_widths(
            &baseline,
            &desired,
            &minimums,
            1,
            |column, width| {
                let height = match column {
                    0 | 1 if width >= 108.0 => 20.0,
                    0 | 1 => 100.0,
                    _ => 20.0,
                };
                vec![height]
            },
        );

        assert!(widths[0] >= 108.0);
        assert!(widths[1] >= 108.0);
        assert!(widths[2] <= 84.0);
        assert!((widths.iter().sum::<f32>() - baseline.iter().sum::<f32>()).abs() < 0.01);
    }

    #[test]
    fn height_aware_cache_invalidates_only_when_its_layout_key_changes() {
        egui::__run_test_ui(|ui| {
            let id = egui::Id::new("height-aware-cache");
            let baseline = [100.0, 100.0];
            let desired = [200.0, 300.0];
            let minimums = [40.0, 40.0];
            let measurements = std::cell::Cell::new(0);
            let measure = |_: usize, _: f32| {
                measurements.set(measurements.get() + 1);
                vec![20.0]
            };

            let (_, first_changed) = cached_height_aware_widths(
                ui, id, 1, &baseline, &desired, &minimums, 1, measure,
            );
            let after_first = measurements.get();
            let (_, stable_changed) = cached_height_aware_widths(
                ui, id, 1, &baseline, &desired, &minimums, 1, measure,
            );
            let after_stable = measurements.get();
            let (_, new_key_changed) = cached_height_aware_widths(
                ui, id, 2, &baseline, &desired, &minimums, 1, measure,
            );

            assert!(!first_changed);
            assert!(!stable_changed);
            assert!(new_key_changed);
            assert!(after_first > 0);
            assert_eq!(after_stable, after_first);
            assert!(measurements.get() > after_stable);
        });
    }

    #[test]
    fn table_layout_key_tracks_measurement_inputs() {
        egui::__run_test_ui(|ui| {
            let desired = [100.0, 200.0];
            let minimums = [40.0, 60.0];
            let key = table_layout_key(ui, &desired, &minimums, 300.0, 20.0, 7, 0, 1.0);

            assert_ne!(
                key,
                table_layout_key(ui, &desired, &[40.0, 61.0], 300.0, 20.0, 7, 0, 1.0)
            );
            assert_ne!(
                key,
                table_layout_key(ui, &desired, &minimums, 300.0, 20.0, 7, 1, 1.0)
            );
            assert_ne!(
                key,
                table_layout_key(ui, &desired, &minimums, 300.0, 20.0, 7, 0, 1.25)
            );
        });
    }

    #[test]
    fn height_aware_columns_reduce_production_wrapped_row_height() {
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        egui::CentralPanel::default().show(&ctx, |ui| {
            let mut style = ui.style().as_ref().clone();
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
            style
                .text_styles
                .insert(egui::TextStyle::Monospace, egui::FontId::monospace(16.0));
            ui.set_style(style);
            ui.set_width(360.0);
            let cache = CommonMarkCache::default();
            let options = CommonMarkOptions::default();
            let line_height = body_line_height(ui, &options);
            let rows = [
                [
                    vec![(Event::Text("field".into()), 0..5)],
                    vec![(Event::Text(
                        "A long requirement whose prose should receive enough width to avoid excessive wrapped lines."
                            .into(),
                    ), 0..91)],
                ],
                [
                    vec![(Event::Code("limitations".into()), 0..11)],
                    vec![(Event::Text(
                        "Record missing data, sampling bias, execution failures, and other interpretation limits."
                            .into(),
                    ), 0..88)],
                ],
            ];
            // Natural-width collection is covered separately. Use explicit
            // demands here so this test remains about the production height
            // measurement path even under egui's minimal test font setup.
            let desired = vec![120.0, 700.0];
            let minimums = [40.0; 2];
            let baseline = fit_column_widths(&desired, 340.0, &minimums);
            let measure = |column: usize, width: f32| {
                rows.iter()
                    .map(|row| {
                        table_cell_height(
                            &row[column],
                            line_height,
                            &cache,
                            ui,
                            width,
                            &options,
                        )
                    })
                    .collect::<Vec<_>>()
            };
            let optimized = optimize_fitted_widths(
                &baseline,
                &desired,
                &minimums,
                rows.len(),
                measure,
            );

            let mut before_cache = HashMap::new();
            let before = table_height_score(
                &baseline,
                rows.len(),
                &mut before_cache,
                usize::MAX,
                &mut |column, width| {
                    rows.iter()
                        .map(|row| {
                            table_cell_height(
                                &row[column],
                                line_height,
                                &cache,
                                ui,
                                width,
                                &options,
                            )
                        })
                        .collect()
                },
            )
            .unwrap();
            let mut after_cache = HashMap::new();
            let after = table_height_score(
                &optimized,
                rows.len(),
                &mut after_cache,
                usize::MAX,
                &mut |column, width| {
                    rows.iter()
                        .map(|row| {
                            table_cell_height(
                                &row[column],
                                line_height,
                                &cache,
                                ui,
                                width,
                                &options,
                            )
                        })
                        .collect()
                },
            )
            .unwrap();

            assert!(
                after.row_max_total < before.row_max_total,
                "desired={desired:?} baseline={baseline:?} optimized={optimized:?} before={before:?} after={after:?}"
            );
            assert!((optimized.iter().sum::<f32>() - baseline.iter().sum::<f32>()).abs() < 0.01);
        });
        let _ = ctx.end_pass();
    }

    #[test]
    fn scrollable_renderer_resumes_only_after_complete_blocks() {
        egui::__run_test_ui(|ui| {
            ui.set_width(600.0);
            ui.set_height(300.0);
            let markdown = concat!(
                "# Heading\n\n",
                "Paragraph before.\n\n",
                "- outer\n  - nested\n  - nested two\n- second\n\n",
                "> quoted\n> continuation\n\n",
                "| a | b |\n|---|---|\n| one | two |\n\n",
                "Final paragraph.\n",
            );
            let source_id = egui::Id::new("virtualization-regression");
            let mut cache = CommonMarkCache::default();
            let options = CommonMarkOptions::default();

            CommonMarkViewerInternal::new().show_scrollable(
                source_id,
                ui,
                &mut cache,
                &options,
                markdown,
                Some(1),
                None,
                false,
                None,
            );
            let sc = scroll_cache(&mut cache, &source_id);
            assert!(sc.page_size.is_some_and(|size| size.y > 0.0));
            assert!(!sc.split_points.is_empty());
            for (index, _, _) in &sc.split_points {
                if let Some((event, _)) = sc.events.get(*index) {
                    assert!(
                        !matches!(event, Event::End(_)),
                        "split resumed at an unmatched End event: {event:?}"
                    );
                }
            }
            let table_end_index = sc
                .events
                .iter()
                .position(|(event, _)| matches!(event, Event::End(pulldown_cmark::TagEnd::Table)))
                .expect("fixture contains a table end");
            assert!(
                sc.split_points
                    .iter()
                    .any(|(index, _, _)| *index == table_end_index + 1),
                "atomic table must leave a safe resume point after its consumed End event"
            );

            CommonMarkViewerInternal::new().show_scrollable(
                source_id,
                ui,
                &mut cache,
                &options,
                markdown,
                Some(1),
                None,
                false,
                None,
            );
        });
    }

    #[test]
    fn registered_markdown_paths_are_detected_as_auto_links() {
        let hooks = std::collections::HashMap::from([
            ("docs/guide.md".to_string(), false),
            ("#section".to_string(), false),
        ]);
        assert_eq!(
            registered_auto_link_ranges("See docs/guide.md, then continue.", &hooks),
            vec![(4..17, "docs/guide.md".to_string())]
        );
    }

    #[test]
    fn auto_link_detection_respects_filename_boundaries() {
        let hooks = std::collections::HashMap::from([("guide.md".to_string(), false)]);
        assert!(registered_auto_link_ranges("not-guide.md.backup", &hooks).is_empty());
        assert_eq!(
            registered_auto_link_ranges("(guide.md)", &hooks),
            vec![(1..9, "guide.md".to_string())]
        );
    }

    #[test]
    fn inline_code_can_match_an_exact_registered_markdown_path() {
        let hooks = std::collections::HashMap::from([
            ("docs/guide.md".to_string(), false),
            ("#section".to_string(), false),
        ]);
        assert_eq!(
            registered_exact_auto_link("docs/guide.md", &hooks),
            Some("docs/guide.md".to_string())
        );
        assert_eq!(registered_exact_auto_link("#section", &hooks), None);
        assert_eq!(registered_exact_auto_link("guide.md", &hooks), None);
    }

    #[test]
    fn no_colon_fast_path_borrows_original_text() {
        let text = "plain text only";
        let mut seen = 0;

        visit_emoji_text_segments(text, &(7..7 + text.len()), |segment| {
            seen += 1;
            assert_eq!(segment.rendered.as_ptr(), text.as_ptr());
            assert_eq!(segment.raw.as_ptr(), text.as_ptr());
            assert_eq!(segment.source_range, 7..7 + text.len());
            assert!(!segment.replaced);
        });

        assert_eq!(seen, 1);
    }

    #[test]
    fn unknown_only_fast_path_borrows_original_text() {
        let text = ":unknown: and :still_unknown:";
        let mut seen = 0;

        visit_emoji_text_segments(text, &(3..3 + text.len()), |segment| {
            seen += 1;
            assert_eq!(segment.rendered.as_ptr(), text.as_ptr());
            assert_eq!(segment.raw.as_ptr(), text.as_ptr());
            assert_eq!(segment.source_range, 3..3 + text.len());
            assert!(!segment.replaced);
        });

        assert_eq!(seen, 1);
    }

    #[test]
    fn plain_query_boundary_does_not_highlight_following_emoji() {
        let text = "hello world :rocket:";
        let span = 30..30 + text.len();
        let world = 36..41;
        let mut snapshots = Vec::new();

        visit_emoji_text_segments(text, &span, |segment| {
            if segment.replaced {
                snapshots.push((
                    segment.rendered.to_owned(),
                    highlight_for_source_span(
                        &segment.source_range,
                        std::slice::from_ref(&world),
                        None,
                    ),
                ));
            } else {
                snapshots.extend(highlight_snapshot(
                    segment.rendered,
                    &segment.source_range,
                    std::slice::from_ref(&world),
                    None,
                ));
            }
        });

        assert_eq!(
            snapshots,
            vec![
                ("hello ".into(), HighlightKind::None),
                ("world".into(), HighlightKind::Match),
                (" ".into(), HighlightKind::None),
                ("🚀".into(), HighlightKind::None),
            ]
        );
    }

    #[test]
    fn emoji_segments_keep_absolute_raw_source_ranges() {
        assert_eq!(
            segment_snapshot("A :pushpin: B", 20),
            vec![
                ("A ".into(), 20..22, "A ".into(), false),
                ("📌".into(), 22..31, ":pushpin:".into(), true),
                (" B".into(), 31..33, " B".into(), false),
            ]
        );
    }

    #[test]
    fn emoji_segments_support_multiple_and_adjacent_shortcodes() {
        assert_eq!(
            segment_snapshot(":rocket::pushpin:", 0),
            vec![
                ("🚀".into(), 0..8, ":rocket:".into(), true),
                ("📌".into(), 8..17, ":pushpin:".into(), true),
            ]
        );
    }

    #[test]
    fn emoji_segments_preserve_unknown_empty_and_unterminated_candidates() {
        for text in [
            ":not_a_gemoji:",
            "::",
            "prefix :pushpin",
            "12:30",
            "https://x:y",
        ] {
            assert_eq!(
                segment_snapshot(text, 7),
                vec![(text.into(), 7..7 + text.len(), text.into(), false)]
            );
        }
    }

    #[test]
    fn emoji_segments_continue_after_unknown_candidates() {
        assert_eq!(
            segment_snapshot(":unknown: then :rocket:", 4),
            vec![
                (
                    ":unknown: then ".into(),
                    4..19,
                    ":unknown: then ".into(),
                    false
                ),
                ("🚀".into(), 19..27, ":rocket:".into(), true),
            ]
        );
    }

    #[test]
    fn emoji_segments_are_utf8_safe_before_and_after_shortcode() {
        assert_eq!(
            segment_snapshot("é:pushpin:界", 10),
            vec![
                ("é".into(), 10..12, "é".into(), false),
                ("📌".into(), 12..21, ":pushpin:".into(), true),
                ("界".into(), 21..24, "界".into(), false),
            ]
        );
    }

    #[test]
    fn emoji_segments_refuse_non_identity_source_mapping() {
        let mut snapshots = Vec::new();
        visit_emoji_text_segments("📌", &(10..19), |segment| {
            snapshots.push((
                segment.rendered.to_owned(),
                segment.source_range,
                segment.raw.to_owned(),
                segment.replaced,
            ));
        });
        assert_eq!(snapshots, vec![("📌".into(), 10..19, "📌".into(), false)]);
    }

    #[test]
    fn replacement_highlight_is_indivisible_for_any_source_overlap() {
        let source = 22..31;
        assert_eq!(
            highlight_for_source_span(&source, &[25..26], None),
            HighlightKind::Match
        );
        assert_eq!(
            highlight_for_source_span(&source, &[0..100], None),
            HighlightKind::Match
        );
    }

    #[test]
    fn active_overlap_wins_over_regular_match() {
        assert_eq!(
            highlight_for_source_span(&(22..31), &[22..31], Some(&(24..25))),
            HighlightKind::Active
        );
    }

    #[test]
    fn disjoint_source_ranges_do_not_highlight_replacement() {
        assert_eq!(
            highlight_for_source_span(&(22..31), &[0..22, 31..40], None),
            HighlightKind::None
        );
    }

    #[test]
    fn emoji_expansion_eligibility_excludes_images_and_code_blocks() {
        assert!(emoji_expansion_is_eligible(false, false));
        assert!(!emoji_expansion_is_eligible(true, false));
        assert!(!emoji_expansion_is_eligible(false, true));
        assert!(!emoji_expansion_is_eligible(true, true));
    }

    #[test]
    fn eligible_markdown_text_and_link_labels_expand() {
        let markdown = "Paragraph :pushpin: *:rocket:* [:pushpin:](https://e/:rocket:)";
        assert_eq!(expanded_visible_text(markdown), "Paragraph 📌 🚀 📌");
    }

    #[test]
    fn production_inline_code_stays_literal_and_keeps_source_highlighting() {
        // Drive the production Event::Code branch and inspect its heading accumulator output.
        egui::__run_test_ui(|ui| {
            let markdown = "`:pushpin:`";
            let mut renderer = CommonMarkViewerInternal::new();
            let mut cache = CommonMarkCache::default();
            cache.set_search_ranges(std::iter::once(1..10).collect());
            cache.set_active_search_range(Some(1..10));
            renderer.text_style.heading = Some(1);

            renderer.event(
                ui,
                Event::Code(":pushpin:".into()),
                0..markdown.len(),
                &mut cache,
                &CommonMarkOptions::default(),
                540.0,
            );

            assert_eq!(renderer.current_heading_rich_texts.len(), 1);
            assert_eq!(renderer.current_heading_rich_texts[0].text(), ":pushpin:");
            assert_eq!(renderer.current_heading_text, ":pushpin:");
            assert!(
                cache.active_search_y().is_some(),
                "inline-code active search range was not recorded"
            );
        });
    }

    #[test]
    fn production_heading_accumulates_emoji_display_and_raw_shortcode_identity() {
        // Drive production Event::Text while heading mode is active.
        egui::__run_test_ui(|ui| {
            let mut renderer = CommonMarkViewerInternal::new();
            let mut cache = CommonMarkCache::default();
            renderer.text_style.heading = Some(1);

            renderer.event(
                ui,
                Event::Text("Pin :pushpin:".into()),
                3..16,
                &mut cache,
                &CommonMarkOptions::default(),
                540.0,
            );

            let display: String = renderer
                .current_heading_rich_texts
                .iter()
                .map(|text| text.text())
                .collect();
            assert_eq!(display, "Pin 📌");
            assert_eq!(renderer.current_heading_text, "Pin :pushpin:");
        });
    }

    #[test]
    fn production_duplicate_shortcode_headings_use_source_keys() {
        // Run complete heading start/text/end production events twice against one cache.
        egui::__run_test_ui(|ui| {
            let mut renderer = CommonMarkViewerInternal::new();
            let mut cache = CommonMarkCache::default();
            let options = CommonMarkOptions::default();

            for source_start in [0, 20] {
                renderer.start_tag(
                    ui,
                    Tag::Heading {
                        level: HeadingLevel::H2,
                        id: None,
                        classes: Vec::new(),
                        attrs: Vec::new(),
                    },
                    source_start,
                    &options,
                );
                renderer.event(
                    ui,
                    Event::Text("Pin :pushpin:".into()),
                    3..16,
                    &mut cache,
                    &options,
                    540.0,
                );
                renderer.end_tag(
                    ui,
                    pulldown_cmark::TagEnd::Heading(HeadingLevel::H2),
                    &mut cache,
                    &options,
                    540.0,
                );
            }

            assert!(cache.get_header_position("heading-source:0").is_some());
            assert!(cache.get_header_position("heading-source:20").is_some());
        });
    }

    #[test]
    fn sliced_heading_position_includes_the_slice_origin() {
        assert_eq!(content_relative_y(50.0, -229.0, 0.0), 279.0);
        assert_eq!(content_relative_y(120.0, 80.0, 1_976.0), 2_016.0);
    }

    #[test]
    fn sliced_search_position_includes_origin_and_excludes_chrome() {
        let mut cache = CommonMarkCache::default();

        record_active_search_content_y(&mut cache, 120.0, 80.0, 1_976.0);

        assert_eq!(cache.active_search_y(), Some(2_016.0));
    }

    #[test]
    fn nested_navigation_positions_keep_the_root_render_origin() {
        let markdown = concat!(
            "Paragraph one.\n\n",
            "Paragraph two.\n\n",
            "Paragraph three.\n\n",
            "Paragraph four.\n\n",
            "> ## Nested heading\n",
            "> quoted text\n\n",
            "| Key | Value |\n",
            "|---|---|\n",
            "| row | ACTIVE_TABLE_MATCH |\n",
        );
        let active_start = markdown.find("ACTIVE_TABLE_MATCH").unwrap();
        let active_range = active_start..active_start + "ACTIVE_TABLE_MATCH".len();
        let heading_source_start = crate::parsers::latex_delimiters::parse_events(markdown, false, false)
            .into_iter()
            .find_map(|(event, range)| {
                matches!(event, Event::Start(Tag::Heading { .. })).then_some(range.start)
            })
            .expect("fixture contains a nested heading");
        let heading_key = crate::header_position_key(heading_source_start);
        let mut cache = CommonMarkCache::default();
        cache.set_search_ranges(vec![active_range.clone()]);
        cache.set_active_search_range(Some(active_range));
        let ctx = egui::Context::default();
        let mut minimum_nested_y = 0.0;

        ctx.begin_pass(Default::default());
        egui::CentralPanel::default().show(&ctx, |ui| {
            ui.set_width(400.0);
            let line_height = ui.text_style_height(&egui::TextStyle::Body);
            minimum_nested_y = line_height * 4.0;
            CommonMarkViewerInternal::new().show(
                ui,
                &mut cache,
                &CommonMarkOptions::default(),
                markdown,
                None,
            );
        });
        let _ = ctx.end_pass();

        assert!(
            cache
                .get_header_position(&heading_key)
                .is_some_and(|y| y > minimum_nested_y),
            "nested heading lost the preceding document height"
        );
        assert!(
            cache
                .active_search_y()
                .is_some_and(|y| y > minimum_nested_y),
            "table-cell search match lost the preceding document height"
        );
    }

    #[test]
    fn markdown_table_identity_uses_document_and_source_position() {
        let document = Id::new("document-a");

        assert_eq!(
            markdown_table_id(document, 120),
            markdown_table_id(document, 120)
        );
        assert_ne!(
            markdown_table_id(document, 120),
            markdown_table_id(document, 240)
        );
        assert_ne!(
            markdown_table_id(document, 120),
            markdown_table_id(Id::new("document-b"), 120)
        );
    }

    #[test]
    fn inline_fenced_and_indented_code_remain_literal() {
        let markdown = "`:pushpin:`\n\n```text\n:rocket:\n```\n\n    :pushpin:\n";
        assert_eq!(
            expanded_visible_text(markdown),
            ":pushpin::rocket:\n:pushpin:\n"
        );
    }

    #[test]
    fn image_alt_text_and_url_remain_literal() {
        let markdown = "![:pushpin:](https://e/:rocket:.png)";
        assert_eq!(expanded_visible_text(markdown), ":pushpin:");
    }

    #[test]
    fn ending_linked_image_keeps_wrapped_row_cursor_after_image() {
        egui::__run_test_ui(|ui| {
            let layout = egui::Layout::left_to_right(egui::Align::BOTTOM).with_main_wrap(true);
            ui.allocate_ui_with_layout(egui::vec2(400.0, 0.0), layout, |ui| {
                let mut renderer = CommonMarkViewerInternal::new();
                let mut cache = CommonMarkCache::default();
                let options = CommonMarkOptions::default();

                renderer.link = Some(crate::Link {
                    destination: "https://example.com".to_string(),
                    text: Vec::new(),
                });
                renderer.image = Some(crate::Image::new(
                    "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg'/>",
                    &options,
                ));

                renderer.end_tag(
                    ui,
                    pulldown_cmark::TagEnd::Image,
                    &mut cache,
                    &options,
                    400.0,
                );
                let cursor_after_image = ui.next_widget_position();

                renderer.end_tag(
                    ui,
                    pulldown_cmark::TagEnd::Link,
                    &mut cache,
                    &options,
                    400.0,
                );

                assert_eq!(ui.next_widget_position(), cursor_after_image);
            });
        });
    }
}
