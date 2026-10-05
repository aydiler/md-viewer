//! Renders a real-world document and reports every painted row wider than
//! the reading column, with its text prefix — used to diagnose right-edge
//! clipping reported on a long project README.

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

const PANE_WIDTH: f32 = 1284.0;
const COLUMN_WIDTH: usize = 600;

#[test]
fn real_document_rows_stay_within_column() {
    let path = std::env::var("MDV_REAL_DOC")
        .unwrap_or_else(|_| "/Users/phillipchaffee/git/GLM-5/experiments/proxy-run/README.md".into());
    let Ok(doc) = std::fs::read_to_string(&path) else {
        eprintln!("skip: cannot read {path}");
        return;
    };
    eprintln!("doc: {path} ({} bytes)", doc.len());

    let ctx = Context::default();
    let mut style = (*ctx.style()).clone();
    style
        .text_styles
        .insert(TextStyle::Body, egui::FontId::proportional(16.0));
    ctx.set_style(style);

    let mut cache = CommonMarkCache::default();
    let mut overflow_rows: Vec<(String, f32)> = Vec::new();
    let mut min_left = f32::INFINITY;
    let mut max_left = f32::NEG_INFINITY;
    let mut max_right = f32::NEG_INFINITY;
    let mut last_text_bottom = f32::NEG_INFINITY;
    let mut content_size_y = f32::NAN;
    let expected_left = (PANE_WIDTH - COLUMN_WIDTH as f32) / 2.0;
    let column_right = expected_left + COLUMN_WIDTH as f32;

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(PANE_WIDTH, 3600.0));

    // Bootstrap frame + slice frames (cache warm; extra frames let async
    // math textures land and the re-bootstrap settle before measuring).
    for frame in 0..5 {
        let full = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let out = CommonMarkViewer::new()
                        .default_width(Some(COLUMN_WIDTH))
                        .table_max_width(Some(COLUMN_WIDTH))
                        .show_scrollable("real-doc", ui, &mut cache, &doc);
                    content_size_y = out.content_size.y;
                });
            },
        );
        let mut texts = Vec::new();
        for clipped in full.shapes {
            collect(&clipped.shape, &mut texts);
        }
        if frame == 0 {
            continue; // bootstrap frame; slices are what the user sees
        }
        if frame < 4 {
            continue; // measure only the settled frames
        }
        for t in &texts {
            min_left = min_left.min(t.pos.x);
            max_left = max_left.max(t.pos.x);
            let right = t.pos.x + t.galley.rect.right();
            max_right = max_right.max(right);
            last_text_bottom = last_text_bottom.max(t.pos.y + t.galley.rect.bottom());
            if right > column_right + 4.0 {
                let prefix: String = t.text.chars().take(60).collect();
                overflow_rows.push((prefix, right));
            }
        }
        eprintln!(
            "frame {frame}: {} texts, text x-range=[{min_left:.1}, {max_right:.1}], \
             max label left={max_left:.1}",
            texts.len()
        );
        if frame == 4 {
            for t in &texts {
                if t.text.starts_with("vLLM >= 0.11")
                    || t.text.starts_with("--disable-log-requests")
                    || t.text.starts_with("; newer vLLM")
                    || t.text.starts_with("— build the regen")
                {
                    eprintln!(
                        "SEGMENT x={:.1} w={:.1} text={:?}",
                        t.pos.x,
                        t.galley.rect.width(),
                        t.text.chars().take(40).collect::<String>()
                    );
                }
            }
        }
    }

    eprintln!(
        "content_size.y={content_size_y:.1} last_text_bottom={last_text_bottom:.1} \
         bottom_padding={:.1}",
        content_size_y - last_text_bottom
    );

    // Column must be centered: left margin ≈ right margin.
    eprintln!(
        "expected column left ≈ {expected_left:.1} (margin {:.1}), column right ≈ {column_right:.1}",
        expected_left
    );
    assert!(
        (min_left - expected_left).abs() <= 40.0,
        "column not centered: min_left={min_left:.1}, expected ≈{expected_left:.1}"
    );

    overflow_rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    overflow_rows.truncate(15);
    for (prefix, right) in &overflow_rows {
        eprintln!("OVERFLOW past pane right={right:.1} text={prefix:?}");
    }
    assert!(
        overflow_rows.is_empty(),
        "{} painted rows exceed the pane right edge",
        overflow_rows.len()
    );

    // Bottom breathing room: the scrollable extent must not cut into the last
    // glyphs. The 2-line padding after the final block is part of the measured
    // extent (see the add_space after the event loop); async math textures can
    // transiently outgrow it by an item spacing until the re-bootstrap lands.
    assert!(
        content_size_y >= last_text_bottom - 8.0,
        "extent cuts into the last text: content_size.y={content_size_y:.1}, \
         last text bottom={last_text_bottom:.1}"
    );
}