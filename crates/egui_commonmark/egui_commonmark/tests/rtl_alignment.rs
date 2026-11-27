//! Right-to-left paragraphs and headings sit flush with the right edge of the
//! reading column (#108), in both the bootstrap and the viewport-slice pass.
//!
//! egui's default fonts have no Hebrew glyphs, so the text lays out as
//! replacement glyphs. That does not matter here: the direction is decided by
//! the renderer from the characters, and only the geometry is asserted.

mod common;
#[allow(unused_imports)]
use common::{FrameExt as _, PanelExt as _};

use std::sync::Arc;

use egui::{vec2, Context, Pos2, RawInput, Rect, Shape, TextStyle};
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};

const PANE_WIDTH: f32 = 1000.0;
const COLUMN_WIDTH: usize = 600;

/// A wrapped English paragraph: its left edge is the column's left edge.
const ENGLISH: &str = "Reference paragraph that is long enough to wrap onto a second \
row of the reading column so that its left edge marks where the column starts, \
whatever the pane width and the scrollbar reservation.";

struct Painted {
    text: String,
    pos: Pos2,
    galley: Arc<egui::Galley>,
}

impl Painted {
    fn left(&self) -> f32 {
        self.pos.x + self.galley.rect.left()
    }
    fn right(&self) -> f32 {
        self.pos.x + self.galley.rect.right()
    }
}

fn collect(shape: &Shape, out: &mut Vec<Painted>) {
    match shape {
        Shape::Text(t) => out.push(Painted {
            text: t.galley.job.text.clone(),
            pos: t.pos,
            galley: t.galley.clone(),
        }),
        Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
        _ => {}
    }
}

fn context() -> Context {
    let ctx = Context::default();
    let mut style = (*ctx.global_style()).clone();
    style
        .text_styles
        .insert(TextStyle::Body, egui::FontId::proportional(16.0));
    ctx.set_global_style(style);
    ctx
}

fn input() -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(PANE_WIDTH, 800.0))),
        ..Default::default()
    }
}

fn frame(
    ctx: &Context,
    cache: &mut CommonMarkCache,
    markdown: &str,
    input: RawInput,
) -> Vec<Painted> {
    let full = ctx.run_frame(input, |ctx| {
        egui::CentralPanel::default().show_ctx(ctx, |ui| {
            CommonMarkViewer::new()
                .default_width(Some(COLUMN_WIDTH))
                .show_scrollable("rtl-doc", ui, cache, markdown);
        });
    });
    let mut painted = Vec::new();
    for clipped in full.shapes {
        collect(&clipped.shape, &mut painted);
    }
    painted
}

/// Bootstrap pass, then a viewport-slice pass.
fn render(markdown: &str) -> [Vec<Painted>; 2] {
    let ctx = context();
    let mut cache = CommonMarkCache::default();
    let bootstrap = frame(&ctx, &mut cache, markdown, input());
    let slice = frame(&ctx, &mut cache, markdown, input());
    [bootstrap, slice]
}

fn find<'a>(painted: &'a [Painted], marker: &str) -> &'a Painted {
    painted
        .iter()
        .find(|p| p.text.contains(marker))
        .unwrap_or_else(|| {
            let texts: Vec<&str> = painted.iter().map(|p| p.text.as_str()).collect();
            panic!("{marker:?} was not painted; painted: {texts:#?}")
        })
}

/// (left, right) of the reading column, from the English reference paragraph.
fn column(painted: &[Painted]) -> (f32, f32) {
    let left = find(painted, "Reference paragraph").left();
    (left, left + COLUMN_WIDTH as f32)
}

#[test]
fn rtl_paragraph_sits_at_the_right_edge() {
    let doc = format!("{ENGLISH}\n\nשלום עולם\n");
    for (pass, painted) in ["bootstrap", "slice"].iter().zip(render(&doc)) {
        let (left, right) = column(&painted);
        let rtl = find(&painted, "שלום");
        assert!(
            (rtl.right() - right).abs() <= 4.0,
            "{pass}: RTL paragraph should end at the column's right edge {right:.1}, \
             ends at {:.1}",
            rtl.right()
        );
        assert!(
            rtl.left() > left + 100.0,
            "{pass}: a short RTL paragraph should not start at the left edge {left:.1}, \
             starts at {:.1}",
            rtl.left()
        );
    }
}

#[test]
fn every_row_of_a_wrapped_rtl_paragraph_ends_at_the_right_edge() {
    let hebrew = "שלום עולם זה משפט ארוך ".repeat(12);
    let doc = format!("{ENGLISH}\n\n{hebrew}\n");
    for (pass, painted) in ["bootstrap", "slice"].iter().zip(render(&doc)) {
        let (_, right) = column(&painted);
        let rtl = find(&painted, "שלום");
        let rows: Vec<_> = rtl
            .galley
            .rows
            .iter()
            .filter(|r| !r.glyphs.is_empty())
            .collect();
        assert!(
            rows.len() > 1,
            "{pass}: fixture must wrap, got {} row(s)",
            rows.len()
        );
        for (i, row) in rows.iter().enumerate() {
            let row_right = rtl.pos.x + row.rect().right();
            assert!(
                (row_right - right).abs() <= 4.0,
                "{pass}: row {i} ends at {row_right:.1}, expected the right edge {right:.1}"
            );
        }
    }
}

#[test]
fn ltr_paragraphs_keep_the_left_edge() {
    // A paragraph whose first strong character is Latin is left to right,
    // even when Hebrew follows.
    let doc = format!("{ENGLISH}\n\nEnglish first, then עברית.\n");
    for (pass, painted) in ["bootstrap", "slice"].iter().zip(render(&doc)) {
        let (left, _) = column(&painted);
        let mixed = find(&painted, "English first");
        assert!(
            (mixed.left() - left).abs() <= 1.0,
            "{pass}: LTR paragraph moved: starts at {:.1}, column left {left:.1}",
            mixed.left()
        );
    }
}

#[test]
fn rtl_heading_sits_at_the_right_edge() {
    let doc = format!("{ENGLISH}\n\n## כותרת ראשית\n\nEnglish after.\n");
    for (pass, painted) in ["bootstrap", "slice"].iter().zip(render(&doc)) {
        let (left, right) = column(&painted);
        let heading = find(&painted, "כותרת");
        assert!(
            (heading.right() - right).abs() <= 4.0,
            "{pass}: RTL heading ends at {:.1}, expected {right:.1}",
            heading.right()
        );
        // The block after it is back on the left.
        let after = find(&painted, "English after");
        assert!(
            (after.left() - left).abs() <= 1.0,
            "{pass}: following paragraph moved"
        );
        assert!(
            after.pos.y > heading.pos.y,
            "{pass}: following paragraph overlaps the heading"
        );
    }
}

#[test]
fn rtl_paragraph_with_inline_image_keeps_the_old_path() {
    // An image cannot live inside one galley, so this paragraph is not merged:
    // its first text piece still starts at the left edge.
    let doc = format!("{ENGLISH}\n\nשלום ![x](missing.png) עולם\n");
    let [painted, _] = render(&doc);
    let (left, _) = column(&painted);
    let first = find(&painted, "שלום");
    assert!(
        (first.left() - left).abs() <= 1.0,
        "ineligible RTL paragraph should keep the per-label path, starts at {:.1}",
        first.left()
    );
    assert!(
        !first.text.contains("עולם"),
        "pieces were merged: {:?}",
        first.text
    );
}

#[test]
fn search_highlight_survives_inside_an_rtl_paragraph() {
    let doc = format!("{ENGLISH}\n\nשלום עולם\n");
    let start = doc.find("עולם").unwrap();
    let ctx = context();
    let mut cache = CommonMarkCache::default();
    let word = start..start + "עולם".len();
    cache.set_search_ranges(vec![word]);
    let painted = frame(&ctx, &mut cache, &doc, input());
    let rtl = find(&painted, "שלום");
    let highlighted = rtl
        .galley
        .job
        .sections
        .iter()
        .any(|s| s.format.background != egui::Color32::TRANSPARENT);
    assert!(
        highlighted,
        "no highlighted section in the RTL galley: {:?}",
        rtl.galley.job.sections
    );
}

#[test]
fn links_keep_their_look_inside_an_rtl_paragraph() {
    // Clicking them is tested in the root workspace (`tests/rtl_links.rs`):
    // mapping a pointer to a character in right-to-left text needs the
    // bidi-aware epaint that only md-viewer builds with.
    let doc = format!("{ENGLISH}\n\nשלום [קישור](other.md) עולם\n");
    let [painted, _] = render(&doc);
    let rtl = find(&painted, "קישור");
    assert!(
        rtl.text.contains("שלום") && rtl.text.contains("עולם"),
        "not merged: {:?}",
        rtl.text
    );
    let link_start = rtl.text.find("קישור").unwrap();
    let link_section = rtl
        .galley
        .job
        .sections
        .iter()
        .find(|s| s.byte_range.contains(&link_start.into()))
        .expect("section for the link text");
    assert!(
        link_section.format.underline.width > 0.0,
        "link is not underlined: {link_section:?}"
    );
}

/// Centres of the filled or hollow circles painted (bullet markers).
fn circles(ctx: &Context, cache: &mut CommonMarkCache, markdown: &str) -> Vec<Pos2> {
    fn walk(shape: &Shape, out: &mut Vec<Pos2>) {
        match shape {
            Shape::Circle(c) => out.push(c.center),
            Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let full = ctx.run_frame(input(), |ctx| {
        egui::CentralPanel::default().show_ctx(ctx, |ui| {
            CommonMarkViewer::new()
                .default_width(Some(COLUMN_WIDTH))
                .show_scrollable("rtl-doc", ui, cache, markdown);
        });
    });
    let mut out = Vec::new();
    for clipped in full.shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

#[test]
fn rtl_bullet_item_puts_text_and_marker_at_the_right_edge() {
    let doc = format!("{ENGLISH}\n\n- שלום עולם\n- English item\n");
    let ctx = context();
    let mut cache = CommonMarkCache::default();
    let _ = frame(&ctx, &mut cache, &doc, input());
    let painted = frame(&ctx, &mut cache, &doc, input());
    let bullets = circles(&ctx, &mut cache, &doc);
    let (left, right) = column(&painted);

    let rtl = find(&painted, "שלום");
    assert!(
        rtl.right() < right && rtl.right() > right - 60.0,
        "RTL item text should end just left of the marker at the right edge {right:.1}, \
         ends at {:.1}",
        rtl.right()
    );
    assert!(
        rtl.left() > left + 100.0,
        "RTL item starts at the left edge"
    );
    let rtl_mid = rtl.pos.y + rtl.galley.rect.center().y;
    let marker = bullets
        .iter()
        .find(|c| (c.y - rtl_mid).abs() < 12.0)
        .unwrap_or_else(|| panic!("no bullet on the RTL item's line; bullets {bullets:?}"));
    assert!(
        marker.x > rtl.right() && marker.x <= right,
        "RTL bullet should sit between the text {:.1} and the right edge {right:.1}, at {:.1}",
        rtl.right(),
        marker.x
    );

    // The English item in the same list keeps its marker and text on the left.
    let ltr = find(&painted, "English item");
    assert!(
        ltr.left() < left + 60.0,
        "LTR item moved to {:.1}",
        ltr.left()
    );
    let ltr_mid = ltr.pos.y + ltr.galley.rect.center().y;
    let ltr_marker = bullets
        .iter()
        .find(|c| (c.y - ltr_mid).abs() < 12.0)
        .expect("bullet on the LTR item's line");
    assert!(
        ltr_marker.x < ltr.left(),
        "LTR bullet is not left of its text"
    );
}

#[test]
fn nested_rtl_items_indent_from_the_right_as_far_as_ltr_items_do_from_the_left() {
    let doc =
        format!("{ENGLISH}\n\n- Outer item\n  - Inner item\n\n---\n\n- שלום עולם\n  - תת פריט\n");
    for (pass, painted) in ["bootstrap", "slice"].iter().zip(render(&doc)) {
        let ltr_step = find(&painted, "Inner item").left() - find(&painted, "Outer item").left();
        let outer = find(&painted, "שלום");
        let inner = find(&painted, "תת");
        let rtl_step = outer.right() - inner.right();
        assert!(
            (rtl_step - ltr_step).abs() <= 2.0,
            "{pass}: nested RTL item steps {rtl_step:.1} px in from its parent, \
             an LTR one {ltr_step:.1} px"
        );
        assert!(inner.pos.y > outer.pos.y, "{pass}: nested item overlaps");
    }
}

#[test]
fn rtl_ordered_item_paints_its_number_on_the_right() {
    let doc = format!("{ENGLISH}\n\n1. שלום עולם\n2. עוד פריט\n");
    let [painted, _] = render(&doc);
    let item = find(&painted, "שלום");
    let number = painted
        .iter()
        .find(|p| p.text.contains('1') && (p.pos.y - item.pos.y).abs() < 12.0)
        .unwrap_or_else(|| panic!("no number on the first item's line"));
    assert!(
        number.left() > item.right(),
        "number {:.1}..{:.1} should be right of the item text ending at {:.1}",
        number.left(),
        number.right(),
        item.right()
    );
}

#[test]
fn loose_rtl_list_items_sit_at_the_right_edge() {
    let doc = format!("{ENGLISH}\n\n- שלום עולם\n\n- עוד פריט\n");
    for (pass, painted) in ["bootstrap", "slice"].iter().zip(render(&doc)) {
        let (left, right) = column(&painted);
        for marker in ["שלום", "עוד"] {
            let item = find(&painted, marker);
            assert!(
                item.right() > right - 60.0 && item.left() > left + 100.0,
                "{pass}: loose item {marker} at {:.1}..{:.1}, column {left:.1}..{right:.1}",
                item.left(),
                item.right()
            );
        }
    }
}

#[test]
fn rtl_table_cell_aligns_its_text_right() {
    // Tables stretch to the pane (#211), so a column's right edge is measured
    // as the start of the column after it.
    let doc = format!(
        "{ENGLISH}\n\n| First header | Second header |\n|---|---|\n| שלום | x |\n| English cell | y |\n"
    );
    for (pass, painted) in ["bootstrap", "slice"].iter().zip(render(&doc)) {
        let first = find(&painted, "First header");
        let second = find(&painted, "Second header");
        let rtl = find(&painted, "שלום");
        let ltr = find(&painted, "English cell");
        assert!(
            rtl.right() < second.left() && rtl.right() > second.left() - 24.0,
            "{pass}: RTL cell should end just before the next column at {:.1}, ends at {:.1}",
            second.left(),
            rtl.right()
        );
        assert!(
            rtl.left() > first.left() + 40.0,
            "{pass}: RTL cell starts at the column's left edge"
        );
        assert!(
            (ltr.left() - first.left()).abs() <= 1.0,
            "{pass}: LTR cell moved"
        );
    }
}

/// Square rectangles of checkbox size: the boxes of task-list checkboxes.
fn checkboxes(ctx: &Context, cache: &mut CommonMarkCache, markdown: &str) -> Vec<Rect> {
    fn walk(shape: &Shape, out: &mut Vec<Rect>) {
        match shape {
            Shape::Rect(r)
                if (8.0..=24.0).contains(&r.rect.width())
                    && (r.rect.width() - r.rect.height()).abs() < 1.0 =>
            {
                out.push(r.rect)
            }
            Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let full = ctx.run_frame(input(), |ctx| {
        egui::CentralPanel::default().show_ctx(ctx, |ui| {
            CommonMarkViewer::new()
                .default_width(Some(COLUMN_WIDTH))
                .show_scrollable("rtl-doc", ui, cache, markdown);
        });
    });
    let mut out = Vec::new();
    for clipped in full.shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

/// Horizontal extent of the glyphs on a label's first row. A wrapping label's
/// galley starts at the row's left edge, before any widget on that row, so its
/// rect says nothing about where the text begins.
fn first_row_glyphs(text: &Painted) -> (f32, f32) {
    let row = text.galley.rows.first().expect("a row");
    let x = text.pos.x + row.pos.x;
    let left = row.glyphs.iter().map(|g| g.pos.x).fold(f32::MAX, f32::min);
    let right = row
        .glyphs
        .iter()
        .map(|g| g.pos.x + g.advance_width)
        .fold(f32::MIN, f32::max);
    (x + left, x + right)
}

fn on_row<'a>(rects: &'a [Rect], text: &Painted) -> &'a Rect {
    let mid = text.pos.y + text.galley.rect.center().y;
    rects
        .iter()
        .find(|r| (r.center().y - mid).abs() < 12.0)
        .unwrap_or_else(|| panic!("no checkbox on {:?}'s row; checkboxes {rects:?}", text.text))
}

#[test]
fn rtl_task_item_mirrors_the_ltr_task_item() {
    let doc = format!("{ENGLISH}\n\n- [ ] English task\n\n---\n\n- [x] שלום עולם\n");
    let ctx = context();
    let mut cache = CommonMarkCache::default();
    let _ = frame(&ctx, &mut cache, &doc, input());
    let painted = frame(&ctx, &mut cache, &doc, input());
    let boxes = checkboxes(&ctx, &mut cache, &doc);
    let bullets = circles(&ctx, &mut cache, &doc);
    let (left, right) = column(&painted);

    let ltr = find(&painted, "English task");
    let ltr_box = on_row(&boxes, ltr);
    let ltr_gap = first_row_glyphs(ltr).0 - ltr_box.right();

    let rtl = find(&painted, "שלום");
    let rtl_box = on_row(&boxes, rtl);
    let rtl_mid = rtl.pos.y + rtl.galley.rect.center().y;
    let marker = bullets
        .iter()
        .find(|c| (c.y - rtl_mid).abs() < 12.0)
        .unwrap_or_else(|| panic!("no bullet on the RTL task's row; bullets {bullets:?}"));
    assert!(
        rtl.left() > left + 100.0 && rtl.right() > right - 100.0,
        "RTL task text at {:.1}..{:.1} is not at the right of the column {left:.1}..{right:.1}",
        rtl.left(),
        rtl.right()
    );
    let rtl_text_right = first_row_glyphs(rtl).1;
    assert!(
        rtl_text_right <= rtl_box.left() && rtl_box.right() < marker.x && marker.x <= right,
        "expected text {rtl_text_right:.1} <= checkbox {:.1}..{:.1} < marker {:.1} <= right \
         edge {right:.1}",
        rtl_box.left(),
        rtl_box.right(),
        marker.x
    );
    let rtl_gap = rtl_box.left() - rtl_text_right;
    assert!(
        (rtl_gap - ltr_gap).abs() <= 2.0,
        "checkbox-to-text gap: RTL {rtl_gap:.1} px, LTR {ltr_gap:.1} px"
    );
    assert!(
        (rtl_box.center().y - ltr_box.center().y - (rtl.pos.y - ltr.pos.y)).abs() <= 2.0,
        "RTL checkbox sits {:.1} px from its text's row top, LTR {:.1} px",
        rtl_box.center().y - rtl.pos.y,
        ltr_box.center().y - ltr.pos.y
    );
}

#[test]
fn loose_rtl_task_items_put_their_checkboxes_right_of_the_text() {
    let doc = format!("{ENGLISH}\n\n- [ ] שלום עולם\n\n- [x] עוד פריט\n");
    let ctx = context();
    let mut cache = CommonMarkCache::default();
    let _ = frame(&ctx, &mut cache, &doc, input());
    let painted = frame(&ctx, &mut cache, &doc, input());
    let boxes = checkboxes(&ctx, &mut cache, &doc);
    let (_, right) = column(&painted);
    for marker in ["שלום", "עוד"] {
        let item = find(&painted, marker);
        let checkbox = on_row(&boxes, item);
        let text_right = first_row_glyphs(item).1;
        assert!(
            text_right <= checkbox.left() && checkbox.right() < right,
            "{marker}: text ends {text_right:.1}, checkbox {:.1}..{:.1}, right edge {right:.1}",
            checkbox.left(),
            checkbox.right()
        );
    }
}

#[test]
fn clicking_an_rtl_task_checkbox_toggles_it_in_the_source() {
    let ctx = context();
    let mut cache = CommonMarkCache::default();
    let mut text = "- [ ] שלום עולם\n".to_owned();
    let run = |input: RawInput, text: &mut String, cache: &mut CommonMarkCache| {
        let full = ctx.run_frame(input, |ctx| {
            egui::CentralPanel::default().show_ctx(ctx, |ui| {
                CommonMarkViewer::new()
                    .default_width(Some(COLUMN_WIDTH))
                    .show_mut(ui, cache, text);
            });
        });
        let mut boxes = Vec::new();
        fn walk(shape: &Shape, out: &mut Vec<Rect>) {
            match shape {
                Shape::Rect(r)
                    if (8.0..=24.0).contains(&r.rect.width())
                        && (r.rect.width() - r.rect.height()).abs() < 1.0 =>
                {
                    out.push(r.rect)
                }
                Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        for clipped in full.shapes {
            walk(&clipped.shape, &mut boxes);
        }
        boxes
    };
    let _ = run(input(), &mut text, &mut cache);
    let boxes = run(input(), &mut text, &mut cache);
    // egui's checkbox paints two concentric squares; one checkbox means one centre.
    let at = boxes.first().expect("a checkbox").center();
    assert!(
        boxes.iter().all(|b| (b.center() - at).length() < 3.0),
        "one checkbox: {boxes:?}"
    );
    assert!(
        at.x > PANE_WIDTH / 2.0,
        "the RTL checkbox is not on the right half: {at:?}"
    );
    let pointer = |pressed: bool| RawInput {
        events: vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ],
        ..input()
    };
    let _ = run(pointer(true), &mut text, &mut cache);
    let _ = run(pointer(false), &mut text, &mut cache);
    assert_eq!(text, "- [x] שלום עולם\n");
}

/// A paint step of a frame, in order: text galleys, filled rectangles and
/// textured rectangles (images).
#[cfg(feature = "math")]
#[derive(Debug)]
enum Step {
    Text(String, Rect, Vec<Rect>),
    Fill(Rect, egui::Color32),
    Image(Rect),
}

/// Render with `$x^2$` seeded as a rendered formula of `size`, then return the
/// paint steps of the last frame.
#[cfg(feature = "math")]
fn math_frames(markdown: &str, size: egui::Vec2) -> (Vec<Step>, egui::Color32) {
    fn walk(shape: &Shape, out: &mut Vec<Step>) {
        match shape {
            Shape::Text(t) => {
                // The ink of the formula placeholders (U+FFFC), in screen space.
                let mut ink = Vec::new();
                for row in &t.galley.rows {
                    for g in row.glyphs.iter().filter(|g| g.chr == '\u{FFFC}') {
                        let min = t.pos + row.pos.to_vec2() + g.pos.to_vec2() + g.uv_rect.offset;
                        ink.push(Rect::from_min_size(min, g.uv_rect.size));
                    }
                }
                out.push(Step::Text(
                    t.galley.job.text.clone(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                    ink,
                ))
            }
            Shape::Rect(r) if r.fill_texture_id() != egui::TextureId::default() => {
                out.push(Step::Image(r.rect))
            }
            Shape::Rect(r) => out.push(Step::Fill(r.rect, r.fill)),
            Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let ctx = context();
    let mut cache = CommonMarkCache::default();
    let mut steps = Vec::new();
    let mut panel_fill = egui::Color32::TRANSPARENT;
    for _ in 0..3 {
        let full = ctx.run_frame(input(), |ctx| {
            egui::CentralPanel::default().show_ctx(ctx, |ui| {
                panel_fill = ui.visuals().panel_fill;
                egui_commonmark_extended::seed_inline_math_size_for_test(
                    ui,
                    &mut cache,
                    "x^2",
                    &egui_commonmark_backend_extended::CommonMarkOptions::default(),
                    size,
                );
                CommonMarkViewer::new()
                    .default_width(Some(COLUMN_WIDTH))
                    .show_scrollable("rtl-doc", ui, &mut cache, markdown);
            });
        });
        steps.clear();
        for clipped in full.shapes {
            walk(&clipped.shape, &mut steps);
        }
    }
    (steps, panel_fill)
}

#[cfg(feature = "math")]
#[test]
fn rtl_paragraph_with_inline_math_sits_at_the_right_edge() {
    let doc = format!("{ENGLISH}\n\nשלום $x^2$ עולם\n");
    let (steps, _) = math_frames(&doc, vec2(30.0, 20.0));
    let english = steps
        .iter()
        .find_map(|s| match s {
            Step::Text(t, r, _) if t.contains("Reference paragraph") => Some(*r),
            _ => None,
        })
        .expect("reference paragraph");
    let (left, right) = (english.left(), english.left() + COLUMN_WIDTH as f32);
    let rtl = steps
        .iter()
        .find_map(|s| match s {
            Step::Text(t, r, _) if t.contains("שלום") => Some(*r),
            _ => None,
        })
        .expect("the RTL paragraph");
    assert!(
        rtl.right() > right - 4.0 && rtl.right() <= right + 0.5 && rtl.left() > left + 100.0,
        "RTL paragraph with a formula at {:.1}..{:.1}, column {left:.1}..{right:.1}",
        rtl.left(),
        rtl.right()
    );
    let image = steps
        .iter()
        .find_map(|s| match s {
            Step::Image(r) => Some(*r),
            _ => None,
        })
        .expect("the formula's image");
    assert!(
        rtl.contains_rect(image.shrink(0.5)) && (image.width() - 30.0).abs() < 0.5,
        "formula image {image:?} should lie inside the paragraph {rtl:?} at 30 px wide"
    );
    assert!(
        rtl.height() >= 20.0,
        "the paragraph's row ({:.1}) is lower than the formula",
        rtl.height()
    );
}

#[cfg(feature = "math")]
#[test]
fn rtl_inline_math_is_painted_over_a_patch_after_its_text() {
    let doc = format!("{ENGLISH}\n\nשלום $x^2$ עולם\n");
    let (steps, panel_fill) = math_frames(&doc, vec2(30.0, 20.0));
    let text_at = steps
        .iter()
        .position(|s| matches!(s, Step::Text(t, _, _) if t.contains("שלום")))
        .expect("the RTL paragraph");
    let image_at = steps
        .iter()
        .position(|s| matches!(s, Step::Image(_)))
        .expect("the formula's image");
    let Step::Image(image) = &steps[image_at] else {
        unreachable!()
    };
    // The patch hides the placeholder glyphs, which turn visible when selected.
    let patch_at = steps
        .iter()
        .position(|s| {
            matches!(s, Step::Fill(r, fill) if *fill == panel_fill
                && r.contains_rect(image.shrink(0.5))
                && (r.width() - image.width()).abs() < 1.0)
        })
        .unwrap_or_else(|| panic!("no patch under the formula; steps {steps:#?}"));
    assert!(
        text_at < patch_at && patch_at < image_at,
        "paint order text {text_at}, patch {patch_at}, image {image_at}"
    );
    let (Step::Text(_, _, ink), Step::Fill(patch, _)) = (&steps[text_at], &steps[patch_at]) else {
        unreachable!()
    };
    assert!(!ink.is_empty(), "no placeholder glyphs in the paragraph");
    for glyph in ink {
        assert!(
            patch.expand(0.5).contains_rect(*glyph),
            "placeholder ink {glyph:?} is not under the patch {patch:?}: it shows when selected"
        );
    }
}
