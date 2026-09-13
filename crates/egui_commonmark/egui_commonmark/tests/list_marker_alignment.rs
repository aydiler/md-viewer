//! Regression tests for list-marker vertical alignment (#196).
//!
//! Markers used to be placed from their own allocated box (`bottom - raw/2`),
//! a compensation tuned at one set of font metrics. The measured result was a
//! bullet dot ~1.7 px above the item text's lowercase optical centre at a 16 px
//! body with a 1.5× line height. The fix derives the position from the item
//! text's own first-line layout, so every test here pins painted geometry:
//! the dot's centre must land within a pixel of `baseline − x-height/2`,
//! computed from the first item-text glyph's own metrics (never a constant).

use std::sync::Arc;

use egui::{Context, Pos2, Shape, TextStyle};
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};

struct PaintedText {
    text: String,
    pos: Pos2,
    galley: Arc<egui::Galley>,
}

struct PaintedDot {
    centre: Pos2,
}

fn collect(shape: &Shape, texts: &mut Vec<PaintedText>, dots: &mut Vec<PaintedDot>) {
    match shape {
        Shape::Text(t) => texts.push(PaintedText {
            text: t.galley.job.text.clone(),
            pos: t.pos,
            galley: t.galley.clone(),
        }),
        Shape::Circle(c) => dots.push(PaintedDot { centre: c.center }),
        Shape::Vec(shapes) => {
            for shape in shapes {
                collect(shape, texts, dots);
            }
        }
        _ => {}
    }
}

/// How the item text's line box is configured for a render.
enum LineBox {
    /// `CommonMarkViewer::line_height(multiplier)`
    Multiplier(f32),
    /// `CommonMarkViewer::line_height_px(pixels)`
    Pixels(f32),
}

struct Render {
    texts: Vec<PaintedText>,
    dots: Vec<PaintedDot>,
}

fn render(markdown: &str, width: f32, body_size: f32, line_box: LineBox) -> Render {
    let ctx = Context::default();
    let mut style = (*ctx.style()).clone();
    style
        .text_styles
        .insert(TextStyle::Body, egui::FontId::proportional(body_size));
    ctx.set_style(style);

    let mut cache = CommonMarkCache::default();
    let mut out = Render {
        texts: Vec::new(),
        dots: Vec::new(),
    };

    // Two passes so font/layout caches settle; only final-pass geometry counts.
    for pass in 0..2 {
        out.texts.clear();
        out.dots.clear();
        let full = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_max_width(width);
                let viewer = CommonMarkViewer::new();
                let viewer = viewer
                    .default_width(Some(width as usize))
                    .table_max_width(Some(width as usize));
                let viewer = match line_box {
                    LineBox::Multiplier(m) => viewer.line_height(m),
                    LineBox::Pixels(px) => viewer.line_height_px(px),
                };
                viewer.show(ui, &mut cache, markdown);
            });
        });
        if pass == 1 {
            for clipped in full.shapes {
                collect(&clipped.shape, &mut out.texts, &mut out.dots);
            }
        }
    }
    out
}

/// The painted baseline of a text shape's first row.
fn first_row_baseline(text: &PaintedText) -> f32 {
    let row = text.galley.rows.first().expect("galleys are never empty");
    let glyph = row.glyphs.first().expect("rows have glyphs");
    text.pos.y + row.pos.y + glyph.pos.y
}

fn find_text<'a>(render: &'a Render, prefix: &str) -> &'a PaintedText {
    render
        .texts
        .iter()
        .find(|t| t.text.starts_with(prefix))
        .unwrap_or_else(|| panic!("no painted text starting with {prefix:?}"))
}

/// Lowercase optical centre of an item text whose first glyph is an `x`:
/// baseline minus half the glyph's own x-height. All item texts in these
/// documents deliberately start with `x` so the metric comes from the painted
/// shape itself rather than a constant.
fn optical_centre_y(text: &PaintedText) -> f32 {
    let row = text.galley.rows.first().expect("galleys are never empty");
    let glyph = row.glyphs.first().expect("rows have glyphs");
    let baseline = text.pos.y + row.pos.y + glyph.pos.y;
    baseline - glyph.uv_rect.size.y / 2.0
}

#[track_caller]
fn assert_dot_on_optical_centre(render: &Render, item_prefix: &str) {
    let text = find_text(render, item_prefix);
    let expected = optical_centre_y(text);
    let dot = render
        .dots
        .iter()
        .min_by(|a, b| {
            (a.centre.y - expected)
                .abs()
                .total_cmp(&(b.centre.y - expected).abs())
        })
        .expect("the bullet paints a circle");
    let error = (dot.centre.y - expected).abs();
    assert!(
        error <= 1.0,
        "dot centre {:.2} vs lowercase optical centre {:.2} ({error:.2} px off, tolerance 1.0 px)",
        dot.centre.y,
        expected
    );
}

/// Baseline-aligned ordered markers, not centre-aligned: a digit centred on
/// the x-height midpoint would sink its baseline visibly below the text's.
#[track_caller]
fn assert_number_baseline_aligned(render: &Render, number: &str, item_prefix: &str) {
    let number = find_text(render, number);
    let item = find_text(render, item_prefix);
    let error = (first_row_baseline(number) - first_row_baseline(item)).abs();
    assert!(
        error <= 1.0,
        "number baseline {:.2} vs item text baseline {:.2} ({error:.2} px off, tolerance 1.0 px)",
        first_row_baseline(number),
        first_row_baseline(item)
    );
}

#[test]
fn bullet_dot_sits_on_lowercase_optical_centre() {
    let render = render(
        "Absatz.\n\n- xenial text\n",
        600.0,
        16.0,
        LineBox::Multiplier(1.5),
    );
    assert_dot_on_optical_centre(&render, "xenial");
}

#[test]
fn bullet_dot_matches_optical_centre_at_other_sizes_and_line_boxes() {
    for (size, line_box) in [
        (12.0, LineBox::Multiplier(1.5)),
        (20.0, LineBox::Multiplier(1.5)),
        (16.0, LineBox::Pixels(28.0)),
        (16.0, LineBox::Pixels(20.0)),
    ] {
        let render = render("- xenial text\n", 600.0, size, line_box.clone());
        assert_dot_on_optical_centre(&render, "xenial");
    }
}

#[test]
fn ordered_number_stays_baseline_aligned_with_item_text() {
    let render = render(
        "Absatz.\n\n1. xenon text\n",
        600.0,
        16.0,
        LineBox::Multiplier(1.5),
    );
    assert_number_baseline_aligned(&render, "1.", "xenon");
}

#[test]
fn bullet_aligns_after_a_task_checkbox() {
    // The checkbox joins the line between the marker slot and the text, so the
    // marker may only be positioned once the text is about to be laid out.
    let render = render(
        "Absatz.\n\n- [ ] xenial task\n",
        600.0,
        16.0,
        LineBox::Multiplier(1.5),
    );
    assert_dot_on_optical_centre(&render, "xenial");
}

#[test]
fn bullet_aligns_to_first_line_of_wrapped_item_text() {
    // Narrow enough that the item text wraps; the marker belongs on row one.
    let render = render(
        "- xenial text that certainly wraps somewhere\n",
        160.0,
        16.0,
        LineBox::Multiplier(1.5),
    );
    let text = find_text(&render, "xenial");
    assert!(text.galley.rows.len() > 1, "expected the item text to wrap");
    assert_dot_on_optical_centre(&render, "xenial");
}

#[test]
fn hollow_bullet_of_nested_lists_aligns_the_same_way() {
    let render = render(
        "Absatz.\n\n- - xenial nested\n",
        600.0,
        16.0,
        LineBox::Multiplier(1.5),
    );
    assert_dot_on_optical_centre(&render, "xenial");
}

impl Clone for LineBox {
    fn clone(&self) -> Self {
        match self {
            LineBox::Multiplier(m) => LineBox::Multiplier(*m),
            LineBox::Pixels(px) => LineBox::Pixels(*px),
        }
    }
}
