//! Styled layouter for live-preview editing: renders a markdown *block*
//! inside a stock [`egui::TextEdit`] so it looks like its rendered form.
//!
//! Core contract (mirrors egui's password masking): the produced job text has
//! exactly the same CHAR count as the input buffer — markup characters are
//! replaced with spaces, never dropped — so every `CCursor` index keeps
//! meaning the same character. Byte-level differences inside the job are fine;
//! sections are rebuilt over the transformed text.
//!
//! Styling rules:
//! - structural prefixes (`# `, `- `, `> `, `1. `) become blank glyphs
//! - `**strong**`, `*em*`, `` `code` `` runs are styled; their delimiters are
//!   blanked
//! - `[label](url)` keeps the label styled as a link, blanks brackets/url
//! - headings scale/weight their text
//! - when `reveal_line` points at the caret's line, that line paints fully
//!   raw (no blanks, base formatting) — the Obsidian Live Preview signature

use egui::{text::LayoutJob, Color32, FontFamily, FontId, TextFormat};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;

/// What kind of top-level block the edited buffer represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditBlockKind {
    Heading(u8),
    Paragraph,
    Quote,
    ListItem,
    CodeBlock,
}

/// Visual parameters for the styled editing surface. Build once per frame
/// from the active `Ui` via [`MarkdownEditStyle::from_ui`].
#[derive(Clone, Debug)]
pub struct MarkdownEditStyle {
    pub body_size: f32,
    /// Line height applied to text rows (renderer default 1.5×).
    pub line_height: Option<f32>,
    /// Space inserted AFTER a paragraph block.
    pub paragraph_gap: f32,
    /// Space ABOVE / BELOW a heading of level l (1-based): scale lookup.
    pub heading_above_scale: [f32; 6],
    pub heading_below_scale: [f32; 6],
    /// Multiplier applied to `body_size` per heading level (H1..H6).
    pub heading_scale: [f32; 6],
    /// Family used for strong/heading text (registered bold face).
    pub strong_family: FontFamily,
    pub body_family: FontFamily,
    pub mono_family: FontFamily,
    pub text_color: Color32,
    pub marker_color: Color32,
    pub code_bg: Color32,
    pub link_color: Color32,
}

impl MarkdownEditStyle {
    pub fn from_ui(ui: &egui::Ui) -> Self {
        let st = ui.style();
        let body = st
            .text_styles
            .get(&egui::TextStyle::Body)
            .cloned()
            .unwrap_or_else(|| egui::FontId::new(14.0, FontFamily::Proportional));
        let mono = st
            .text_styles
            .get(&egui::TextStyle::Monospace)
            .cloned()
            .unwrap_or_else(|| egui::FontId::new(13.0, FontFamily::Monospace));
        let v = ui.visuals();
        // Bold face when the app registered one; else fall back to body.
        let strong = FontFamily::Name(crate::misc::STRONG_FONT_FAMILY.into());
        let strong_registered =
            ui.fonts(|f| f.definitions().families.contains_key(&strong));
        Self {
            body_size: body.size,
            heading_scale: [2.0, 1.6, 1.25, 1.125, 1.0, 0.875],
            line_height: Some(body.size * 1.5),
            paragraph_gap: body.size * 0.75,
            heading_above_scale: [1.125, 1.0, 0.875, 0.75, 0.5, 0.5],
            heading_below_scale: [0.25, 0.25, 0.125, 0.125, 0.0, 0.0],
            strong_family: if strong_registered { strong } else { body.family.clone() },
            body_family: body.family.clone(),
            mono_family: mono.family,
            text_color: v.widgets.inactive.fg_stroke.color,
            marker_color: v.weak_text_color(),
            code_bg: v.extreme_bg_color,
            link_color: v.hyperlink_color,
        }
    }
}

// Char-level decoration tags. Order matters only for grouping.
const T_BASE: u8 = 0;
const T_MARKER: u8 = 1; // blanked glyph
const T_STRONG: u8 = 2;
const T_EM: u8 = 3;
const T_CODE: u8 = 4;
const T_LINK: u8 = 5;

struct LineInfo {
    /// Byte offset where each line starts (first entry always 0).
    starts: Vec<usize>,
    /// 0-based line index for each char position.
    line_of_char: Vec<usize>,
}

fn line_info(chars: &[char]) -> LineInfo {
    let mut starts = vec![0usize];
    let mut line_of_char = Vec::with_capacity(chars.len());
    let mut line = 0usize;
    for &c in chars {
        line_of_char.push(line);
        if c == '\n' {
            line += 1;
            starts.push(0); // placeholder, fixed below
        }
    }
    // Recompute byte starts precisely.
    starts.clear();
    starts.push(0);
    let mut byte = 0usize;
    for &c in chars {
        if c == '\n' {
            starts.push(byte + 1);
        }
        byte += c.len_utf8();
    }
    LineInfo {
        starts,
        line_of_char,
    }
}

/// Leading structural marker byte-range for one line, given block kind.
/// Returns None when the line has no structural prefix.
fn structural_prefix(text: &str, line_byte_start: usize, kind: EditBlockKind) -> Option<Range<usize>> {
    let rest = text.get(line_byte_start..)?;
    let mut cols = rest.char_indices();
    // Skip leading spaces (list indentation).
    let mut saw_space = false;
    let mut pos = 0usize;
    loop {
        match cols.next() {
            Some((i, ' ')) | Some((i, '\t')) => {
                saw_space = true;
                pos = i + 1;
            }
            _ => break,
        }
    }
    let _ = saw_space;
    let after_ws = pos;
    let tail = &rest[after_ws..];
    let prefix_len = match kind {
        EditBlockKind::Heading(level) => {
            let hashes = tail.chars().take_while(|&c| c == '#').count();
            if hashes == 0 || hashes as u8 != level {
                return None;
            }
            let after_hashes = tail[hashes..]
                .chars()
                .take_while(|&c| c == ' ')
                .count();
            if after_hashes == 0 {
                return None;
            }
            hashes + after_hashes
        }
        EditBlockKind::Quote => {
            if tail.starts_with('>') {
                let after_gt = tail[1..].chars().take_while(|&c| c == ' ').count() + 1;
                after_gt
            } else {
                return None;
            }
        }
        EditBlockKind::ListItem => {
            let bytes = tail.as_bytes();
            if bytes.is_empty() {
                return None;
            }
            let (marker_len, after) = match bytes[0] {
                b'-' | b'*' | b'+' => (1usize, &tail[1..]),
                _ => {
                    let digits = tail.chars().take_while(|c| c.is_ascii_digit()).count();
                    if digits == 0 || !tail[digits..].starts_with('.') {
                        return None;
                    }
                    (digits + 1, &tail[digits + 1..])
                }
            };
            let spaces = after.chars().take_while(|&c| c == ' ').count();
            if spaces == 0 {
                return None;
            }
            marker_len + spaces
        }
        _ => return None,
    };
    Some(line_byte_start + after_ws..line_byte_start + after_ws + prefix_len)
}

/// Build the styled layout job for one markdown block.
///
/// Guarantees: `job.text.chars().count() == text.chars().count()`.
pub fn markdown_block_job(
    text: &str,
    kind: EditBlockKind,
    style: &MarkdownEditStyle,
    reveal_line: Option<usize>,
    wrap_width: f32,
) -> LayoutJob {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        let mut job = LayoutJob::default();
        job.wrap.max_width = wrap_width;
        return job;
    }
    if matches!(kind, EditBlockKind::CodeBlock) {
        // Code stays raw monospace — that IS its rendered form.
        let font = FontId::new(style.body_size, style.mono_family.clone());
        let fmt = TextFormat {
            font_id: font,
            color: style.text_color,
            ..Default::default()
        };
        let mut job = LayoutJob::default();
        job.append(text, 0.0, fmt);
        job.wrap.max_width = wrap_width;
        return job;
    }

    let li = line_info(&chars);
    let mut tags = vec![T_BASE; chars.len()];
    let mut blank = vec![false; chars.len()];

    // ---- structural markers ----
    let mut line_byte = 0usize;
    let mut line_idx = 0usize;
    for line in text.split_inclusive('\n') {
        if reveal_line != Some(line_idx) {
            if let Some(r) = structural_prefix(text, line_byte, kind) {
                for bi in 0..r.len() {
                    if let Some(ci) = char_index_of(text, r.start + bi) {
                        blank[ci] = true;
                        tags[ci] = T_MARKER;
                    }
                }
            }
        }
        line_byte += line.len();
        line_idx += 1;
    }

    // ---- inline formatting ----
    let mut containers: Vec<(u8, Range<usize>)> = Vec::new();

    for (event, span) in Parser::new_ext(text, Options::all()).into_offset_iter() {
        match event {
            Event::Start(Tag::Strong) => containers.push((T_STRONG, span)),
            Event::Start(Tag::Emphasis) => containers.push((T_EM, span)),
            Event::Start(Tag::Link { .. }) => containers.push((T_LINK, span)),
            Event::Start(_) => {}
            Event::End(end) => {
                let popped = match end {
                    TagEnd::Strong => containers.pop().filter(|(t, _)| *t == T_STRONG),
                    TagEnd::Emphasis => containers.pop().filter(|(t, _)| *t == T_EM),
                    TagEnd::Link { .. } => containers.pop().filter(|(t, _)| *t == T_LINK),
                    _ => None,
                };
                if let Some((_tag, container_span)) = popped {
                    // Children styled their chars already; blank the
                    // leftovers (delimiters, link urls) unless on reveal line.
                    for ci in byte_range_to_char_indices(text, container_span) {
                        let on_reveal = reveal_line == Some(li.line_of_char[ci]);
                        if !on_reveal && tags[ci] == T_BASE {
                            blank[ci] = true;
                            tags[ci] = T_MARKER;
                        } else if on_reveal {
                            tags[ci] = T_BASE;
                        }
                    }
                }
            }
            Event::Text(t) => {
                let cur_tag = containers.last().map(|&(tag, _)| tag).unwrap_or(T_BASE);
                for ci in byte_range_to_char_indices(text, span) {
                    if reveal_line == Some(li.line_of_char[ci]) {
                        tags[ci] = T_BASE;
                    } else if cur_tag != T_BASE {
                        tags[ci] = cur_tag;
                    }
                }
            }
            Event::Code(_c) => {
                // The span INCLUDES the surrounding backticks.
                let cis = byte_range_to_char_indices(text, span.clone());
                for (n, &ci) in cis.iter().enumerate() {
                    let is_tick = n == 0 || n + 1 == cis.len();
                    if reveal_line == Some(li.line_of_char[ci]) {
                        tags[ci] = T_BASE;
                    } else if is_tick {
                        blank[ci] = true;
                        tags[ci] = T_MARKER;
                    } else {
                        tags[ci] = T_CODE;
                    }
                }
            }
            _ => {}
        }
    }

    // ---- assemble ----
    // Effective format key per char: 0=base, 2..6 styled, 6=marker,
    // 8..13 = heading level. Revealed chars always key 0.
    let fmt_key_of = |i: usize| -> u8 {
        if reveal_line == Some(li.line_of_char[i]) {
            return 0;
        }
        match tags[i] {
            T_STRONG => 2,
            T_EM => 3,
            T_CODE => 4,
            T_LINK => 5,
            T_MARKER => 6,
            _ => match kind {
                EditBlockKind::Heading(l) => 7 + l.clamp(1, 6),
                _ => 0,
            },
        }
    };

    let heading_font_for = |l: u8, s: &MarkdownEditStyle| {
        let size = s.body_size * s.heading_scale[(l as usize - 1).min(5)];
        FontId::new(size, s.strong_family.clone())
    };
    let format_for_key =
        |key: u8, style: &MarkdownEditStyle| -> TextFormat {
            match key {
                2 => TextFormat {
                    font_id: FontId::new(
                        style.body_size,
                        style.strong_family.clone(),
                    ),
                    color: style.text_color,
                    ..Default::default()
                },
                3 => TextFormat {
                    font_id: base_body_font(kind, style),
                    color: style.text_color,
                    italics: true,
                    ..Default::default()
                },
                4 => TextFormat {
                    font_id: FontId::new(
                        style.body_size,
                        style.mono_family.clone(),
                    ),
                    color: style.text_color,
                    background: style.code_bg,
                    ..Default::default()
                },
                5 => TextFormat {
                    font_id: base_body_font(kind, style),
                    color: style.link_color,
                    underline: egui::Stroke::new(1.0, style.link_color),
                    ..Default::default()
                },
                6 => TextFormat {
                    font_id: base_body_font(kind, style),
                    color: style.marker_color,
                    ..Default::default()
                },
                k if (8..=13).contains(&k) => {
                    let l = k - 7;
                    TextFormat {
                        font_id: heading_font_for(l, style),
                        color: style.text_color,
                        ..Default::default()
                    }
                }
                _ => TextFormat {
                    font_id: base_body_font(kind, style),
                    color: style.text_color,
                    ..Default::default()
                },
            }
        };

    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;

    // Single pass: group consecutive chars sharing a format key.
    let mut out = String::with_capacity(text.len());
    let mut run_key: Option<u8> = None;
    let mut run_start_byte = 0usize;

    for (i, &c) in chars.iter().enumerate() {
        let key = fmt_key_of(i);
        let out_char = if blank[i] && key == 6 { ' ' } else { c };

        if run_key != Some(key) {
            if let Some(k) = run_key.take() {
                let slice = out[run_start_byte..].to_string();
                let fmt = format_for_key(k, style);
                job.append(&slice, 0.0, fmt);
            }
            run_start_byte = out.len();
            run_key = Some(key);
        }
        out.push(out_char);
    }
    if !run_key.is_none() {
        let k = run_key.unwrap_or(0);
        let slice = out[run_start_byte.min(out.len())..].to_string();
        let fmt = format_for_key(k, style);
        job.append(&slice, 0.0, fmt);
    }

    debug_assert_eq!(out.chars().count(), chars.len(), "char count preserved");
    job
}

fn base_body_font(kind: EditBlockKind, style: &MarkdownEditStyle) -> FontId {
    match kind {
        EditBlockKind::CodeBlock => {
            FontId::new(style.body_size, style.mono_family.clone())
        }
        _ => FontId::new(style.body_size, style.body_family.clone()),
    }
}

/// Map a byte offset in `text` to its char index (binary search free version).
fn char_index_of(text: &str, byte: usize) -> Option<usize> {
    if byte > text.len() {
        return None;
    }
    let mut ci = 0usize;
    for (bi, _) in text.char_indices() {
        if bi == byte {
            return Some(ci);
        }
        ci += 1;
    }
    if byte == text.len() {
        Some(ci)
    } else {
        None
    }
}

fn byte_range_to_char_indices(text: &str, range: Range<usize>) -> Vec<usize> {
    let start = char_index_of(text, range.start).unwrap_or(0);
    let end = char_index_of(text, range.end).unwrap_or_else(|| text.chars().count());
    (start..end).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> MarkdownEditStyle {
        MarkdownEditStyle {
            body_size: 14.0,
            heading_scale: [2.0, 1.6, 1.25, 1.125, 1.0, 0.875],
            line_height: Some(21.0),
            paragraph_gap: 10.5,
            heading_above_scale: [1.125, 1.0, 0.875, 0.75, 0.5, 0.5],
            heading_below_scale: [0.25, 0.25, 0.125, 0.125, 0.0, 0.0],
            strong_family: FontFamily::Proportional,
            body_family: FontFamily::Proportional,
            mono_family: FontFamily::Monospace,
            text_color: Color32::WHITE,
            marker_color: Color32::GRAY,
            code_bg: Color32::BLACK,
            link_color: Color32::BLUE,
        }
    }

    #[test]
    fn preserves_char_count_for_all_kinds_and_unicode() {
        let style = style();
        let cases = [
            ("**bold** tail", EditBlockKind::Paragraph),
            ("# Title\n", EditBlockKind::Heading(1)),
            ("- item **b** 中🎉\n- second", EditBlockKind::ListItem),
            ("> quoted *em* 文字", EditBlockKind::Quote),
            ("a `code` span 中文", EditBlockKind::Paragraph),
        ];
        for (src, kind) in cases {
            let job = markdown_block_job(src, kind, &style, None, 600.0);
            assert_eq!(
                job.text.chars().count(),
                src.chars().count(),
                "{src:?} -> {:?}",
                job.text
            );
        }
    }

    #[test]
    fn hides_heading_and_list_markers_but_not_reveal_line() {
        let style = style();
        let heading =
            markdown_block_job("# Title", EditBlockKind::Heading(1), &style, None, 600.0);
        assert_eq!(heading.text, "  Title");

        let revealed = markdown_block_job(
            "# Title",
            EditBlockKind::Heading(1),
            &style,
            Some(0),
            600.0,
        );
        assert_eq!(revealed.text, "# Title");

        let list = markdown_block_job("- item", EditBlockKind::ListItem, &style, None, 600.0);
        assert_eq!(list.text, "  item"); // "-" + space blanked
    }

    #[test]
    fn ordered_list_marker_blank_including_number() {
        let style = style();
        let job = markdown_block_job("12. ok", EditBlockKind::ListItem, &style, None, 600.0);
        assert_eq!(job.text, "    ok"); // "12." + space blanked
    }

    #[test]
    fn inline_delimiters_blank_runs_styled() {
        let style = style();
        let job = markdown_block_job(
            "**b** t `c`",
            EditBlockKind::Paragraph,
            &style,
            None,
            600.0,
        );
        // "**"->2sp, b, "**"->2sp, " t ", "`"->sp, c, "`"->sp
        assert_eq!(job.text, "  b   t  c ");
    }

    #[test]
    fn link_label_kept_url_hidden() {
        let style = style();
        let job = markdown_block_job(
            "[site](https://x.io)",
            EditBlockKind::Paragraph,
            &style,
            None,
            600.0,
        );
        assert_eq!(job.text.chars().count(), 20);
        assert!(job.text.contains("site"));
        assert!(!job.text.contains("https"), "{}", job.text);
    }

    #[test]
    fn codeblock_kind_stays_raw() {
        let style = style();
        let src = "let x = 1;\n";
        let job = markdown_block_job(src, EditBlockKind::CodeBlock, &style, None, 600.0);
        assert_eq!(job.text, src);
    }

    #[test]
    fn sections_cover_entire_job_text_in_order() {
        let style = style();
        let job = markdown_block_job(
            "# H\npara **x** more",
            EditBlockKind::Paragraph,
            &style,
            None,
            600.0,
        );
        let mut covered = 0usize;
        for s in &job.sections {
            assert_eq!(s.byte_range.start, covered);
            covered = s.byte_range.end;
        }
        assert_eq!(covered, job.text.len());
    }
}

pub(crate) fn style() -> MarkdownEditStyle {
        MarkdownEditStyle {
            body_size: 14.0,
            heading_scale: [2.0, 1.6, 1.25, 1.125, 1.0, 0.875],
            line_height: Some(21.0),
            paragraph_gap: 10.5,
            heading_above_scale: [1.125, 1.0, 0.875, 0.75, 0.5, 0.5],
            heading_below_scale: [0.25, 0.25, 0.125, 0.125, 0.0, 0.0],
            strong_family: FontFamily::Proportional,
            body_family: FontFamily::Proportional,
            mono_family: FontFamily::Monospace,
            text_color: Color32::WHITE,
            marker_color: Color32::GRAY,
            code_bg: Color32::BLACK,
            link_color: Color32::BLUE,
        }
    }

#[cfg(test)]
mod galley_tests {
    use super::*;

    /// Lays out the styled job inside a REAL egui pass so fonts are
    /// initialized exactly like production.
    fn galley_shape(text: &str, kind: EditBlockKind, reveal: Option<usize>, wrap: f32)
        -> (f32, usize)
    {
        let style = style();
        let mut job = markdown_block_job(text, kind, &style, reveal, wrap);
        job.wrap.max_width = wrap;
        // BISECTION: also lay out a trivial simple() job as control.
        let mut ctrl = egui::text::LayoutJob::simple(
            "Hello control width".into(),
            egui::FontId::new(14.0, FontFamily::Proportional),
            style.text_color,
            wrap,
        );
        let _ = &mut ctrl;

        let ctx = egui::Context::default();
        let result = std::sync::Mutex::new((0.0f32, 0usize));
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Default::default(),
                    egui::Vec2::new(wrap + 100.0, 800.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    {
                        let cg = ui.fonts_mut(|f| f.layout_job(ctrl.clone()));
                        eprintln!("CTRL galley w={:.1} rows={}", cg.size().x, cg.rows.len());
                    }
                    let galley = ui.fonts_mut(|f| f.layout_job(job.clone()));
                    let mut res = result.lock().unwrap();
                    res.0 = galley.size().x;
                    res.1 = galley.rows.len();
                });
            },
        );
        result.into_inner().unwrap()
    }

    #[test]
    fn paragraph_rows_match_lines() {
        let text = "This paragraph supports *italics*, **bold**, `inline code`, and links.\n";
        let (_w, rows) = galley_shape(text, EditBlockKind::Paragraph, None, 600.0);
        // Line + trailing-newline row. (Absolute width can't be asserted in
        // this environment: headless fonts report zero advances.)
        assert_eq!(rows, 2, "paragraph rows");
    }

    #[test]
    fn heading_is_single_row() {
        let text = "# Live Preview Test Doc";
        let (_w, rows) = galley_shape(text, EditBlockKind::Heading(1), None, 600.0);
        assert_eq!(rows, 1, "heading has no trailing newline in test text");
    }

    #[test]
    fn multiline_block_has_row_per_line_plus_trailing() {
        let text = "- First\n- Second\n- Third\n";
        let (_w, rows) = galley_shape(text, EditBlockKind::ListItem, None, 600.0);
        assert_eq!(rows, 4, "3 items + trailing newline row, got {rows}");
    }
}
