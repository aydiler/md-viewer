use crate::alerts::*;
use egui::{Pos2, Vec2};
use pulldown_cmark::Options;
use std::ops::Range;

#[derive(Default, Debug)]
pub struct ScrollableCache {
    pub available_size: Vec2,
    pub page_size: Option<Vec2>,
    pub split_points: Vec<(usize, Pos2, Pos2)>,
    /// Parsed pulldown events, owned (Event<'static>) so they outlive the
    /// borrow of the source text. Repopulated only when `content_version`
    /// changes, replacing the per-frame `Parser::new_ext(text).collect()`
    /// that previously ran at every paint.
    pub events: Vec<(pulldown_cmark::Event<'static>, Range<usize>)>,
    /// Last content version this cache was populated for. The caller
    /// (typically a `Tab`) bumps a u64 on every load/reload; when the
    /// renderer sees a mismatch it re-parses and clears split_points.
    pub content_version: u64,
    /// Hash of the layout-affecting context (width, font size, line height,
    /// theme is_dark). When this changes, split_points must be cleared —
    /// their y-positions are no longer valid for the new layout.
    pub layout_signature: u64,
    /// Content height as reported by the previous frame's ScrollAreaOutput.
    pub last_content_h: f32,
    /// Content height captured at the most recent bootstrap. Subsequent
    /// paints compare `last_content_h` against this value with a hysteresis
    /// threshold: only when |last - bootstrap| exceeds the threshold do we
    /// invalidate page_size and trigger a re-bootstrap. This avoids the
    /// known egui artifact where `ScrollArea::show` (bootstrap path) and
    /// `ScrollArea::show_viewport` (skip-paint path) report content_size.y
    /// differing by ~44 px (the panel chrome offset) for the same content —
    /// without hysteresis that oscillation crosses any bucket boundary and
    /// keeps the renderer in a perpetual bootstrap loop.
    pub bootstrap_content_h: f32,
}

pub type EventIteratorItem<'e> = (usize, (pulldown_cmark::Event<'e>, Range<usize>));

/// Parse events until a desired end tag is reached or no more events are found.
/// This is needed for multiple events that must be rendered inside a single widget
pub fn delayed_events<'e>(
    events: &mut impl Iterator<Item = EventIteratorItem<'e>>,
    end_at: impl Fn(pulldown_cmark::TagEnd) -> bool,
) -> Vec<(pulldown_cmark::Event<'e>, Range<usize>)> {
    let mut curr_event = events.next();
    let mut total_events = Vec::new();
    loop {
        if let Some(event) = curr_event.take() {
            total_events.push(event.1.clone());
            if let (_, (pulldown_cmark::Event::End(tag), _range)) = event {
                if end_at(tag) {
                    return total_events;
                }
            }
        } else {
            return total_events;
        }

        curr_event = events.next();
    }
}

pub fn delayed_events_list_item<'e>(
    events: &mut impl Iterator<Item = EventIteratorItem<'e>>,
) -> Vec<(pulldown_cmark::Event<'e>, Range<usize>)> {
    // The caller has just consumed the outer `Tag::Item` before invoking this
    // helper, so we start one level deep. Nested `Tag::Item` events
    // increment, `TagEnd::Item` events decrement. When depth would drop to
    // zero we return — the matching outer `TagEnd::Item` (and everything
    // inside the item, including nested lists) is captured.
    //
    // The previous implementation returned at the FIRST `TagEnd::Item`. When
    // an item contained a nested sub-list, that was the inner item's close
    // and the remainder of the outer item (further inner items, the inner
    // `TagEnd::List`, and the outer `TagEnd::Item`) leaked back to the outer
    // render loop. Two consequences followed:
    //   1. The outer loop processed those leaked events without the matching
    //      parent state, eventually calling `List::start_item` with an empty
    //      stack and panicking via `unreachable!()`.
    //   2. The leaked events were registered as `show_scrollable` split
    //      points; on later paints the viewport-skip path landed iteration
    //      mid-list, reproducing the same panic.
    let mut depth: i32 = 1;
    let mut total_events = Vec::new();
    for (_, (event, range)) in events {
        let is_item_start =
            matches!(&event, pulldown_cmark::Event::Start(pulldown_cmark::Tag::Item));
        let is_item_end =
            matches!(&event, pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Item));
        total_events.push((event, range));
        if is_item_start {
            depth += 1;
        } else if is_item_end {
            depth -= 1;
            if depth <= 0 {
                return total_events;
            }
        }
    }
    // Iterator drained before the matching close — return what we collected.
    total_events
}

type Column<'e> = Vec<(pulldown_cmark::Event<'e>, Range<usize>)>;
type Row<'e> = Vec<Column<'e>>;

pub struct Table<'e> {
    pub header: Row<'e>,
    pub rows: Vec<Row<'e>>,
}

fn parse_row<'e>(
    events: &mut impl Iterator<Item = (pulldown_cmark::Event<'e>, Range<usize>)>,
) -> Vec<Column<'e>> {
    let mut row = Vec::new();
    let mut column = Vec::new();

    for (e, src_span) in events.by_ref() {
        if let pulldown_cmark::Event::End(pulldown_cmark::TagEnd::TableCell) = e {
            row.push(column);
            column = Vec::new();
        }

        if let pulldown_cmark::Event::End(pulldown_cmark::TagEnd::TableHead) = e {
            break;
        }

        if let pulldown_cmark::Event::End(pulldown_cmark::TagEnd::TableRow) = e {
            break;
        }

        column.push((e, src_span));
    }

    row
}

pub fn parse_table<'e>(events: &mut impl Iterator<Item = EventIteratorItem<'e>>) -> Table<'e> {
    let mut all_events = delayed_events(events, |end| matches!(end, pulldown_cmark::TagEnd::Table))
        .into_iter()
        .peekable();

    let header = parse_row(&mut all_events);

    let mut rows = Vec::new();
    while all_events.peek().is_some() {
        let row = parse_row(&mut all_events);
        rows.push(row);
    }

    Table { header, rows }
}

/// try to parse events as an alert quote block. This ill modify the events
/// to remove the parsed text that should not be rendered.
/// Assumes that the first element is a Paragraph
pub fn parse_alerts<'a>(
    alerts: &'a AlertBundle,
    events: &mut Vec<(pulldown_cmark::Event<'_>, Range<usize>)>,
) -> Option<&'a Alert> {
    // no point in parsing if there are no alerts to render
    if !alerts.is_empty() {
        let mut alert_ident = "".to_owned();
        let mut alert_ident_ends_at = 0;
        let mut has_extra_line = false;

        for (i, (e, _src_span)) in events.iter().enumerate() {
            if let pulldown_cmark::Event::End(_) = e {
                // > [!TIP]
                // >
                // > Detect the first paragraph
                // In this case the next text will be within a paragraph so it is better to remove
                // the entire paragraph
                alert_ident_ends_at = i;
                has_extra_line = true;
                break;
            }

            if let pulldown_cmark::Event::SoftBreak = e {
                // > [!NOTE]
                // > this is valid and will produce a soft break
                alert_ident_ends_at = i;
                break;
            }

            if let pulldown_cmark::Event::HardBreak = e {
                // > [!NOTE]<whitespace>
                // > this is valid and will produce a hard break
                alert_ident_ends_at = i;
                break;
            }

            if let pulldown_cmark::Event::Text(text) = e {
                alert_ident += text;
            }
        }

        let alert = try_get_alert(alerts, &alert_ident);

        if alert.is_some() {
            // remove the text that identifies it as an alert so that it won't end up in the
            // render
            //
            // FIMXE: performance improvement potential
            if has_extra_line {
                for _ in 0..=alert_ident_ends_at {
                    events.remove(0);
                }
            } else {
                for _ in 0..alert_ident_ends_at {
                    // the first element must be kept as it _should_ be Paragraph
                    events.remove(1);
                }
            }
        }

        alert
    } else {
        None
    }
}

/// Supported pulldown_cmark options
#[inline]
pub fn parser_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_DEFINITION_LIST
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};

    /// Helper: parse markdown, advance the iterator past the first `Tag::Item`
    /// (mirroring what `event()` does for `Tag::Item` before `item_list_wrapping`
    /// is called), then return the iterator positioned to feed
    /// `delayed_events_list_item`.
    fn parse_advance_past_first_item(
        md: &str,
    ) -> (
        Vec<(Event<'static>, std::ops::Range<usize>)>,
        usize,
    ) {
        let events: Vec<_> = Parser::new_ext(md, parser_options())
            .into_offset_iter()
            .map(|(e, r)| (e.into_static(), r))
            .collect();
        // Find the first Tag::Item, advance past it.
        let pos = events
            .iter()
            .position(|(e, _)| matches!(e, Event::Start(Tag::Item)))
            .expect("test markdown must contain at least one list item");
        (events, pos + 1)
    }

    #[test]
    fn delayed_events_list_item_simple_item() {
        let md = "- alpha\n- beta\n";
        let (events, start) = parse_advance_past_first_item(md);
        let mut iter = events
            .clone()
            .into_iter()
            .enumerate()
            .skip(start);
        let collected = delayed_events_list_item(&mut iter);
        // Should contain the contents of the FIRST item and stop AT the
        // matching `TagEnd::Item` for it (inclusive).
        assert!(
            matches!(
                collected.last(),
                Some((Event::End(TagEnd::Item), _))
            ),
            "expected last event to be TagEnd::Item, got {:?}",
            collected.last()
        );
        // The next item start should still be in the iterator (not consumed).
        let next = iter.next();
        assert!(
            matches!(next, Some((_, (Event::Start(Tag::Item), _)))),
            "expected next iterator event to be Tag::Item for the second item, got {next:?}"
        );
    }

    #[test]
    fn delayed_events_list_item_nested_sublist() {
        // Outer item 1 contains a nested sub-list with two items, then there
        // is a second outer item. The pre-fix implementation stopped at the
        // FIRST `TagEnd::Item` (the inner item's close), leaking the rest of
        // outer-item-1 (the inner list close and the outer-item-1 close) back
        // to the caller's iterator.
        let md = "\
- outer-1
  - inner-1a
  - inner-1b
- outer-2
";
        let (events, start) = parse_advance_past_first_item(md);
        let mut iter = events
            .clone()
            .into_iter()
            .enumerate()
            .skip(start);
        let collected = delayed_events_list_item(&mut iter);

        // The captured slice must include the inner list's close (otherwise
        // the renderer's state stays inconsistent across iterations).
        let saw_inner_end_list = collected
            .iter()
            .any(|(e, _)| matches!(e, Event::End(TagEnd::List(_))));
        assert!(
            saw_inner_end_list,
            "collected events must include the inner list close: {collected:?}"
        );

        // It must stop AT the outer item's `TagEnd::Item`, so the last entry
        // is `TagEnd::Item` and after it the outer iterator yields the next
        // outer item's start.
        assert!(
            matches!(collected.last(), Some((Event::End(TagEnd::Item), _))),
            "last event must be the outer item's close, got {:?}",
            collected.last()
        );
        let next = iter.next();
        assert!(
            matches!(next, Some((_, (Event::Start(Tag::Item), _)))),
            "after returning, the outer iterator must be positioned at the second outer item; got {next:?}"
        );
    }

    #[test]
    fn delayed_events_list_item_drained_iterator() {
        // Iterator that drains before reaching a `TagEnd::Item` must return
        // the partial collection without panicking.
        let events: Vec<(Event<'static>, std::ops::Range<usize>)> = vec![
            (Event::Text("orphaned".into()), 0..8),
            (Event::SoftBreak, 8..9),
        ];
        let mut iter = events.into_iter().enumerate();
        let collected = delayed_events_list_item(&mut iter);
        assert_eq!(collected.len(), 2);
    }
}
