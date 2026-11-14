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
