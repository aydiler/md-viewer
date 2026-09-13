//! LaTeX-style math delimiter support (#60): `\(...\)` renders as inline
//! math and `\[...\]` as display math, matching LaTeX/Pandoc conventions.
//!
//! pulldown-cmark applies CommonMark backslash escapes while parsing, so the
//! backslash of `\(` never survives into a [`Event::Text`] and such
//! delimiters cannot be recognized after parsing. The conversion therefore
//! rewrites the raw source before it reaches the parser.
//!
//! A blind rewrite would shift byte offsets and silently break search
//! highlighting, which is defined against the original text. Instead this
//! module
//!
//! 1. parses the original text once without math to find contexts that must
//!    never be converted (inline code, fenced/indented code blocks, raw
//!    HTML) and the blocks within which delimiters may pair (paragraphs,
//!    headings, table cells),
//! 2. scans only prose bytes for paired, unescaped delimiters,
//! 3. splices `$` / `$$` replacements into a copy of the source while
//!    recording every length change, and
//! 4. remaps every event range parsed from the rewritten copy back to
//!    original coordinates before caching.

use egui_commonmark_backend_extended::pulldown::parser_options;
use pulldown_cmark::{Event, Options, Parser, Tag};
use std::ops::Range;

/// One delimiter replacement: the two bytes starting at `at` in the original
/// source are replaced by `replacement` (`"$"` or `"$$"`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rewrite {
    at: usize,
    original_len: usize,
    replacement: &'static str,
}

/// Maps normalized-text coordinates back to original-source coordinates so
/// parsed event ranges stay valid against the original text.
#[derive(Debug, Default)]
struct OffsetMap {
    /// Sorted by `norm_pos`, recorded after every replacement. For a
    /// normalized position `p`, the original position is `p + delta` using
    /// the last entry with `norm_pos <= p` (`delta = 0` before the first).
    shifts: Vec<(usize, i64)>,
}

impl OffsetMap {
    fn map_pos(&self, pos: usize) -> usize {
        match self.shifts.partition_point(|&(n, _)| n <= pos) {
            0 => pos,
            i => {
                let &(n, delta) = &self.shifts[i - 1];
                debug_assert!(n <= pos);
                (pos as i64 + delta) as usize
            }
        }
    }

    fn map_range(&self, range: &mut Range<usize>) {
        range.start = self.map_pos(range.start);
        range.end = self.map_pos(range.end);
    }
}

/// Parse markdown with optional math support, converting LaTeX-style
/// `\(...\)` / `\[...\]` delimiters to `$...$` / `$$...$$` on an in-memory
/// copy when paired occurrences exist in prose. Returned event ranges always
/// refer to `text`, the original source.
pub fn parse_events(
    text: &str,
    math_enabled: bool,
    frontmatter_enabled: bool,
) -> Vec<(Event<'static>, Range<usize>)> {
    if !math_enabled {
        return parse_owned(text, false, frontmatter_enabled);
    }
    let rewrites = find_rewrites(text);
    if rewrites.is_empty() {
        return parse_owned(text, true, frontmatter_enabled);
    }
    let (normalized, map) = build_normalized(text, &rewrites);
    let mut events = parse_owned(&normalized, true, frontmatter_enabled);
    for (_, range) in events.iter_mut() {
        map.map_range(range);
    }
    events
}

fn parse_owned(
    text: &str,
    math_enabled: bool,
    frontmatter_enabled: bool,
) -> Vec<(Event<'static>, Range<usize>)> {
    let mut options = parser_options();
    if math_enabled {
        options |= Options::ENABLE_MATH;
    }
    if frontmatter_enabled {
        options |= Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    }
    Parser::new_ext(text, options)
        .into_offset_iter()
        .map(|(event, range)| (event.into_static(), range))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DelimKind {
    Inline,
    Display,
}

impl DelimKind {
    fn replacement(self) -> &'static str {
        match self {
            DelimKind::Inline => "$",
            DelimKind::Display => "$$",
        }
    }
}

/// Contexts extracted from a plain parse of the original text.
struct Contexts {
    /// Ranges that must never be converted or scanned through: inline code,
    /// fenced/indented code blocks, raw HTML (block and inline).
    protected_ranges: Vec<Range<usize>>,
    /// Container ranges within which delimiters may pair (paragraphs,
    /// headings, table cells). Restricting pairing to one block keeps a
    /// stray opener in one paragraph from swallowing prose up to a closer
    /// paragraphs later.
    block_ranges: Vec<Range<usize>>,
}

fn context_ranges(text: &str) -> Contexts {
    let mut protected: Vec<Range<usize>> = Vec::new();
    let mut blocks: Vec<Range<usize>> = Vec::new();
    for (event, range) in Parser::new_ext(text, parser_options()).into_offset_iter() {
        match event {
            Event::Code(_) | Event::Html(_) | Event::InlineHtml(_) => protected.push(range),
            Event::Start(Tag::CodeBlock(_)) => protected.push(range),
            Event::Start(Tag::Paragraph | Tag::Heading { .. } | Tag::Table(_)) => {
                blocks.push(range)
            }
            _ => {}
        }
    }
    protected.sort_by_key(|r| r.start);
    // Merge overlapping ranges (e.g. an inline code span inside a paragraph
    // is fine, but nested containers can overlap) for cheap skip-ahead.
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(protected.len());
    for range in protected {
        match merged.last_mut() {
            Some(last) if last.end >= range.start => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    Contexts {
        protected_ranges: merged,
        block_ranges: blocks,
    }
}

/// Find paired, unescaped `\(...\)` / `\[...\]` delimiters in prose.
///
/// Returns rewrites ordered by position. An opener supersedes a pending one
/// (mirroring how `$...$` scanning behaves); a closer only completes a
/// pending opener of the same kind, so mismatched leftovers stay literal.
fn find_rewrites(text: &str) -> Vec<Rewrite> {
    let contexts = context_ranges(text);
    if contexts.block_ranges.is_empty() {
        return Vec::new();
    }
    let mut rewrites = Vec::new();
    for block in &contexts.block_ranges {
        for run in unprotected_runs(block.clone(), &contexts.protected_ranges) {
            scan_run(text, run, &mut rewrites);
        }
    }
    rewrites
}

/// Split `range` into the sub-ranges not covered by the sorted, merged
/// `protected` ranges.
fn unprotected_runs(range: Range<usize>, protected: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut runs = Vec::new();
    let mut cursor = range.start;
    for p in protected {
        if p.end <= cursor || p.start >= range.end {
            continue;
        }
        if p.start > cursor {
            runs.push(cursor..p.start);
        }
        cursor = cursor.max(p.end);
    }
    if cursor < range.end {
        runs.push(cursor..range.end);
    }
    runs
}

fn scan_run(text: &str, run: Range<usize>, out: &mut Vec<Rewrite>) {
    #[derive(Clone, Copy)]
    enum Pending {
        Inline(usize),
        Display(usize),
    }

    let bytes = text.as_bytes();
    let mut pending: Option<Pending> = None;
    let mut i = run.start;
    while i + 2 <= run.end {
        if bytes[i] != b'\\' {
            i += 1;
            continue;
        }
        let kind = match bytes[i + 1] {
            b'(' | b')' => DelimKind::Inline,
            b'[' | b']' => DelimKind::Display,
            _ => {
                i += 2;
                continue;
            }
        };
        // An active delimiter needs its backslash free, i.e. an even number
        // of backslashes before it. `\\(` is an escaped backslash followed by
        // an escaped parenthesis and must stay literal.
        let mut slashes = 0usize;
        while slashes < i && bytes[i - 1 - slashes] == b'\\' {
            slashes += 1;
        }
        if slashes % 2 == 1 {
            i += 2;
            continue;
        }
        let opening = matches!(bytes[i + 1], b'(' | b'[');
        if opening {
            pending = Some(match kind {
                DelimKind::Inline => Pending::Inline(i),
                DelimKind::Display => Pending::Display(i),
            });
        } else if let Some(open_at) = pending.and_then(|p| match (p, kind) {
            (Pending::Inline(at), DelimKind::Inline) => Some(at),
            (Pending::Display(at), DelimKind::Display) => Some(at),
            _ => None,
        }) {
            out.push(Rewrite {
                at: open_at,
                original_len: 2,
                replacement: kind.replacement(),
            });
            // CommonMark recognizes table separators before it recognizes math.
            // Hide bare absolute-value bars inside a completed LaTeX-style pair
            // from the table parser; the math backend decodes the entity again.
            for bar in open_at + 2..i {
                let preceding_slashes = bytes[..bar]
                    .iter()
                    .rev()
                    .take_while(|&&byte| byte == b'\\')
                    .count();
                if bytes[bar] == b'|' && preceding_slashes % 2 == 0 {
                    out.push(Rewrite {
                        at: bar,
                        original_len: 1,
                        replacement: "&#124;",
                    });
                }
            }
            out.push(Rewrite {
                at: i,
                original_len: 2,
                replacement: kind.replacement(),
            });
            pending = None;
        }
        i += 2;
    }
}

/// Splice `rewrites` (sorted by position) into a copy of `text` and build the
/// offset map from rewritten coordinates back to original ones.
fn build_normalized<'a>(text: &'a str, rewrites: &[Rewrite]) -> (String, OffsetMap) {
    let mut out = String::with_capacity(text.len());
    let mut shifts: Vec<(usize, i64)> = Vec::with_capacity(rewrites.len());
    let mut delta: i64 = 0;
    let mut prev = 0usize;
    for rw in rewrites {
        out.push_str(&text[prev..rw.at]);
        out.push_str(rw.replacement);
        delta += rw.original_len as i64 - rw.replacement.len() as i64;
        shifts.push((out.len(), delta));
        prev = rw.at + rw.original_len;
    }
    out.push_str(&text[prev..]);
    (out, OffsetMap { shifts })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulldown_cmark::TagEnd;

    /// Collect (event-debug, original-slice) pairs.
    fn outline(text: &str) -> Vec<(String, String)> {
        parse_events(text, true, false)
            .into_iter()
            .map(|(event, range)| (format!("{event:?}"), text[range].to_string()))
            .collect()
    }

    fn find_math(text: &str, needle: &str) -> Option<(String, String)> {
        outline(text).into_iter().find(|(event, slice)| {
            (event.contains("InlineMath") || event.contains("DisplayMath"))
                && slice.contains(needle)
        })
    }

    fn plain_texts(text: &str) -> Vec<String> {
        parse_events(text, true, false)
            .into_iter()
            .filter(|(event, _)| matches!(event, Event::Text(_)))
            .map(|(event, _)| match event {
                Event::Text(t) => t.to_string(),
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn inline_pair_converts_with_original_range() {
        let text = "before \\(P_B,P_A\\) after";
        let (event, slice) = find_math(text, "P_B,P_A").expect("inline math expected");
        assert!(event.contains("InlineMath"));
        assert_eq!(slice, "\\(P_B,P_A\\)");
        // Prose around the formula still lands in Text events unchanged.
        assert!(plain_texts(text).iter().any(|t| t.contains("after")));
    }

    #[test]
    fn table_formula_keeps_command_before_closing_delimiter() {
        let text = concat!(
            "| formula |\n|---|\n",
            r"| \(D_t+\epsilon\) |",
            "\n",
            r"| \(a_t/(D_t^{opp}+\epsilon)\) |",
            "\n",
        );
        let formulas: Vec<_> = parse_events(text, true, false)
            .into_iter()
            .filter_map(|(event, range)| match event {
                Event::InlineMath(tex) => Some((tex.into_string(), text[range].to_string())),
                _ => None,
            })
            .collect();

        assert_eq!(
            formulas,
            [
                (r"D_t+\epsilon".into(), r"\(D_t+\epsilon\)".into()),
                (
                    r"a_t/(D_t^{opp}+\epsilon)".into(),
                    r"\(a_t/(D_t^{opp}+\epsilon)\)".into(),
                ),
            ]
        );
    }

    #[test]
    fn display_pair_across_lines_converts() {
        let text = "\\[\nr_a=1\n\\]";
        let (event, slice) =
            find_math(text, "r_a=1").expect("display math expected");
        assert!(event.contains("DisplayMath"));
        assert_eq!(slice, "\\[\nr_a=1\n\\]");
    }

    #[test]
    fn mixed_inline_and_display_and_plain_dollar_still_work() {
        let text = "$a$ then \\(b\\) and\n\n\\[\nc\n\\]";
        let events = outline(text);
        let inline: Vec<&(String, String)> = events
            .iter()
            .filter(|(e, _)| e.contains("InlineMath"))
            .collect();
        assert_eq!(inline.len(), 2, "both $a$ and \\(b\\) become inline math");
        let display: Vec<&(String, String)> = events
            .iter()
            .filter(|(e, _)| e.contains("DisplayMath"))
            .collect();
        assert_eq!(display.len(), 1);
        assert!(inline.iter().any(|(_, s)| s == "$a$"));
        assert!(inline.iter().any(|(_, s)| s == "\\(b\\)"));
    }

    #[test]
    fn absolute_value_bars_do_not_split_markdown_table_cells() {
        let text = concat!(
            "| name | formula | kind |\n",
            "|---|---|---|\n",
            r"| first | \(\operatorname{EW}[|\Delta OI|]\) | native |",
            "\n",
        );
        let events = parse_events(text, true, false);

        assert_eq!(
            events
                .iter()
                .filter(|(event, _)| matches!(event, Event::Start(Tag::TableCell)))
                .count(),
            6
        );
        let formula = events.iter().find_map(|(event, range)| match event {
            Event::InlineMath(tex) => Some((tex.as_ref(), range.clone())),
            _ => None,
        });
        let (formula, range) = formula.expect("inline formula");
        assert_eq!(formula, r"\operatorname{EW}[&#124;\Delta OI&#124;]");
        assert_eq!(&text[range], r"\(\operatorname{EW}[|\Delta OI|]\)");
    }

    #[test]
    fn dollar_math_unchanged_without_latex_pairs() {
        let (event, _) = find_math("$a^2$", "a^2").expect("dollar math expected");
        assert!(event.contains("InlineMath"));
    }

    #[test]
    fn escaped_delimiters_stay_literal() {
        let text = "\\\\(x\\\\) stays text";
        assert!(find_math(text, "x").is_none());
    }

    #[test]
    fn unmatched_open_stays_literal() {
        let text = "an \\( open pair";
        assert!(find_math(text, "open pair").is_none());
        // Unmatched delimiters keep their CommonMark meaning: literal paren.
        assert!(plain_texts(text).iter().any(|t| t.contains('(')));
    }

    #[test]
    fn unmatched_close_stays_literal() {
        let text = "a lone \\) close";
        assert!(find_math(text, "lone").is_none());
        assert!(plain_texts(text).iter().any(|t| t.contains(')')));
    }

    #[test]
    fn mismatched_kinds_do_not_pair() {
        let text = "\\(a \\] b\\)";
        // Mismatched closers are ignored like stray brackets; the inline
        // pair still completes around them.
        let (event, _) = find_math(text, "b").expect("inline pair completes");
        assert!(event.contains("InlineMath"));
    }

    #[test]
    fn inline_code_is_protected() {
        let text = "`\\(x\\)` stays code";
        assert!(find_math(text, "x").is_none());
    }

    #[test]
    fn fenced_code_block_is_protected() {
        let text = "```\n\\(x\\)\n```";
        assert!(find_math(text, "x").is_none());
    }

    #[test]
    fn indented_code_block_is_protected() {
        let text = "text\n\n    \\(x\\)\n";
        assert!(find_math(text, "x").is_none());
    }

    #[test]
    fn html_block_is_protected() {
        let text = "<div>\n\\(x\\)\n</div>\n";
        assert!(find_math(text, "x").is_none());
    }

    #[test]
    fn no_pairing_across_paragraphs() {
        let text = "open \\( here\n\nand \\) there";
        assert!(find_math(text, "").is_none());
    }

    #[test]
    fn crlf_line_endings_survive() {
        let text = "\\[\r\nr_a=1\r\n\\]\r\n";
        let (event, slice) = find_math(text, "r_a=1").expect("display math expected");
        assert!(event.contains("DisplayMath"));
        assert_eq!(slice, "\\[\r\nr_a=1\r\n\\]");
    }

    #[test]
    fn ranges_after_conversion_point_into_the_original() {
        let text = "a \\(x\\) b";
        let events = parse_events(text, true, false);
        let after = events
            .iter()
            .find(|(e, _)| matches!(e, Event::Text(t) if t.contains('b')))
            .expect("trailing text");
        assert_eq!(&text[after.1.clone()], " b");
        let end_tag = events
            .iter()
            .find(|(e, _)| matches!(e, Event::End(TagEnd::Paragraph)))
            .unwrap();
        assert_eq!(end_tag.1, 0..text.len());
    }

    #[test]
    fn fast_path_when_nothing_converts() {
        // No LaTeX pairs: identical output to a direct parse.
        assert_eq!(
            parse_events("plain $x$ text", true, false),
            parse_owned("plain $x$ text", true, false)
        );
    }

    #[test]
    fn math_disabled_leaves_everything_alone() {
        let events = parse_events("\\(x\\)", false, false);
        assert!(events.iter().all(|(e, _)| !matches!(
            e,
            Event::InlineMath(_) | Event::DisplayMath(_)
        )));
    }
}
