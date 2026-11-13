//! Regression test for the centered reading column.
//!
//! The bootstrap pass must allocate the document column centered in the
//! pane, and BOTH the bootstrap and the viewport-slice pass must wrap text
//! at the column width — never past the pane's right edge (which visually
//! clips under the scrollbar / outline sidebar).

use std::sync::Arc;

use egui::{Context, Pos2, Shape, TextStyle};
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};

struct PaintedText {
    text: String,
    pos: Pos2,
    galley: Arc<egui::Galley>,
}

fn collect(shape: &Shape, texts: &mut Vec<PaintedText>) {
    match shape {
        Shape::Text(t) => texts.push(PaintedText {
            text: t.galley.job.text.clone(),
            pos: t.pos,
            galley: t.galley.clone(),
        }),
        Shape::Vec(shapes) => {
            for shape in shapes {
                collect(shape, texts);
            }
        }
        _ => {}
    }
}

const PANE_WIDTH: f32 = 1000.0;
const COLUMN_WIDTH: usize = 600;
const PARAGRAPH: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do \
eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, \
quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. \
Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu \
fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in \
culpa qui officia deserunt mollit anim id est laborum.";

/// Render two frames: the first is the bootstrap pass, the second paints
/// through the viewport-slice pass (page_size is cached after frame one).
fn render_two_frames() -> (Vec<PaintedText>, Vec<PaintedText>) {
    let ctx = Context::default();
    let mut style = (*ctx.style()).clone();
    style
        .text_styles
        .insert(TextStyle::Body, egui::FontId::proportional(16.0));
    ctx.set_style(style);

    let mut cache = CommonMarkCache::default();
    let mut bootstrap = Vec::new();
    let mut slice = Vec::new();

    for frame in 0..2 {
        let full = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_max_width(PANE_WIDTH);
                CommonMarkViewer::new()
                    .default_width(Some(COLUMN_WIDTH))
                    .table_max_width(Some(COLUMN_WIDTH))
                    .show_scrollable("test-doc", ui, &mut cache, PARAGRAPH);
            });
        });
        let sink = if frame == 0 { &mut bootstrap } else { &mut slice };
        sink.clear();
        for clipped in full.shapes {
            collect(&clipped.shape, sink);
        }
    }
    (bootstrap, slice)
}

fn assert_column_geometry(pass: &str, texts: &[PaintedText]) {
    let body: Vec<&PaintedText> = texts
        .iter()
        .filter(|t| t.text.contains("Lorem ipsum"))
        .collect();
    assert!(
        !body.is_empty(),
        "{pass}: no paragraph text painted at all"
    );

    let left = body
        .iter()
        .map(|t| t.pos.x)
        .fold(f32::INFINITY, f32::min);
    let right = body
        .iter()
        .map(|t| t.pos.x + t.galley.rect.right())
        .fold(f32::NEG_INFINITY, f32::max);

    // Centered: the column must start in the left half, well past the pane edge.
    let expected_left = (PANE_WIDTH - COLUMN_WIDTH as f32) / 2.0;
    assert!(
        left > expected_left - 30.0,
        "{pass}: column not centered — left={left:.1}, expected ≈{expected_left:.1}"
    );

    // Wrapped: no painted row may extend past column_left + column_width
    // (plus small slop for fractional advance widths).
    let column_right = left + COLUMN_WIDTH as f32 + 4.0;
    assert!(
        right <= column_right,
        "{pass}: text overflows the column — left={left:.1}, right={right:.1}, \
         column_right={column_right:.1} (overflow = {:.1}px)",
        right - column_right
    );
}

#[test]
fn centered_column_bootstraps_and_slices_within_column() {
    let (bootstrap, slice) = render_two_frames();
    assert_column_geometry("bootstrap", &bootstrap);
    assert_column_geometry("slice", &slice);
}

/// After a window resize the cached geometry must be re-recorded (or the
/// slice clamped) so text can never paint past the visible pane — the
/// user-visible failure mode was lines clipped at the pane's right edge.
#[test]
fn centered_column_survives_resize_in_both_directions() {
    let ctx = Context::default();
    let mut style = (*ctx.style()).clone();
    style
        .text_styles
        .insert(TextStyle::Body, egui::FontId::proportional(16.0));
    ctx.set_style(style);

    let mut cache = CommonMarkCache::default();
    let rect = |w: f32| egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, 900.0));

    for frame in 0..6 {
        let pane = match frame {
            0 => 900.0,  // bootstrap narrow
            1 => 900.0,  // slice narrow
            2 => 1400.0, // grew — slice with stale cache
            3 => 1400.0, // settled wide
            4 => 900.0,  // shrank — slice with stale cache
            _ => 900.0,  // settled narrow
        };
        let full = ctx.run(
            egui::RawInput {
                screen_rect: Some(rect(pane)),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    CommonMarkViewer::new()
                        .default_width(Some(COLUMN_WIDTH))
                        .table_max_width(Some(COLUMN_WIDTH))
                        .show_scrollable("resize-doc", ui, &mut cache, PARAGRAPH);
                });
            },
        );
        let mut texts = Vec::new();
        for clipped in full.shapes {
            collect(&clipped.shape, &mut texts);
        }
        let body: Vec<&PaintedText> = texts
            .iter()
            .filter(|t| t.text.contains("Lorem ipsum"))
            .collect();
        assert!(!body.is_empty(), "frame {frame}: nothing painted");
        let left = body
            .iter()
            .map(|t| t.pos.x)
            .fold(f32::INFINITY, f32::min);
        let right = body
            .iter()
            .map(|t| t.pos.x + t.galley.rect.right())
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            right <= pane - 1.0,
            "frame {frame} (pane {pane}): text overflows the pane — \
             left={left:.1} right={right:.1} (clip = {:.1}px)",
            right - (pane - 1.0)
        );
        // Once the layout has settled at this width, the column must be
        // centered again (margin = (pane - column) / 2).
        let expected_left = (pane - COLUMN_WIDTH as f32) / 2.0;
        let settled = frame == 3 || frame == 5;
        if settled {
            assert!(
                (left - expected_left).abs() <= 30.0,
                "frame {frame}: column not re-centered after resize — \
                 left={left:.1}, expected ≈{expected_left:.1}"
            );
        }
    }
}
/// Rects of painted textures (the formula's raster), however they are nested.
#[cfg(feature = "math")]
fn collect_textured(shape: &Shape, rects: &mut Vec<egui::Rect>) {
    match shape {
        Shape::Rect(r) if r.fill_texture_id() != egui::TextureId::default() => rects.push(r.rect),
        Shape::Mesh(m) if m.texture_id != egui::TextureId::default() => {
            rects.push(egui::Rect::from_points(&m.vertices.iter().map(|v| v.pos).collect::<Vec<_>>()))
        }
        Shape::Vec(shapes) => {
            for shape in shapes {
                collect_textured(shape, rects);
            }
        }
        _ => {}
    }
}

/// #236: a display formula must not widen the reading column. Its centered
/// box used to be sized from the ambient width instead of the column, and
/// every later paragraph then wrapped against the wider edge. Two identical
/// paragraphs, one on each side of a narrow formula, must wrap identically.
#[cfg(feature = "math")]
#[test]
fn prose_after_display_math_wraps_at_the_column() {
    // Short words, so a wrap-width change of a few pixels moves a break.
    let prose = "a bb ccc dd e fff gg hhhh i jj kkk l mm nnn o pp qqq r ss ttt ".repeat(4);
    let doc = format!("{prose}\n\n$$x + y$$\n\n{prose}\n");

    let ctx = Context::default();
    let mut style = (*ctx.style()).clone();
    style
        .text_styles
        .insert(TextStyle::Body, egui::FontId::proportional(16.0));
    ctx.set_style(style);
    let mut cache = CommonMarkCache::default();
    let screen = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(PANE_WIDTH, 900.0));

    let mut texts = Vec::new();
    let mut image_rects = Vec::new();
    let mut frames_with_formula = 0;
    for _ in 0..500 {
        let full = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    CommonMarkViewer::new()
                        .default_width(Some(COLUMN_WIDTH))
                        .table_max_width(Some(COLUMN_WIDTH))
                        .show_scrollable("math-doc", ui, &mut cache, &doc);
                });
            },
        );
        texts.clear();
        image_rects.clear();
        for clipped in &full.shapes {
            collect(&clipped.shape, &mut texts);
            collect_textured(&clipped.shape, &mut image_rects);
        }
        if !image_rects.is_empty() {
            frames_with_formula += 1;
            if frames_with_formula >= 3 {
                break;
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    assert!(
        frames_with_formula >= 3,
        "the formula never finished rendering; the test would compare placeholder layout"
    );

    let rows = |t: &PaintedText| -> Vec<String> {
        t.galley
            .rows
            .iter()
            .map(|r| r.row.glyphs.iter().map(|g| g.chr).collect::<String>())
            .collect()
    };
    let paragraphs: Vec<&PaintedText> = texts.iter().filter(|t| t.text.contains("hhhh")).collect();
    assert_eq!(paragraphs.len(), 2, "expected both paragraphs painted: {:?}",
        texts.iter().map(|t| &t.text).collect::<Vec<_>>());
    let (before, after) = (rows(paragraphs[0]), rows(paragraphs[1]));
    assert!(before.len() > 1, "the paragraph must wrap for the test to mean anything");
    assert_eq!(before, after, "prose after the formula wraps differently");

    // The formula stays centered in the column the prose occupies.
    let left = paragraphs[0].pos.x;
    let column_center = left + COLUMN_WIDTH as f32 / 2.0;
    let formula = image_rects[0];
    assert!(
        (formula.center().x - column_center).abs() <= 2.0,
        "formula centered at {:.1}, column center {column_center:.1}",
        formula.center().x
    );
}
