//! A markdown table keeps its measurements across frames (devlog 131).
//!
//! Painting a table was already limited to the rows on screen, but every
//! frame measured every row: the reserved height and the body's row heights,
//! each cell's natural width and a digest of the whole table. Scrolling
//! inside a 1 MB table cost 265 ms per frame. Now a settled table measures
//! nothing while it scrolls, and anything that changes a measurement (the
//! table's text, the font, the column widths) measures it again.

mod common;
#[allow(unused_imports)]
use common::{FrameExt as _, PanelExt as _};

use egui::{Context, FontId, Pos2, Rect, TextStyle};
use egui_commonmark_extended::{table_cells_measured_for_test, CommonMarkCache, CommonMarkViewer};

const ROWS: usize = 300;

/// A two-column table whose note column is wider than the pane allows, so its
/// width is capped and a longer note wraps onto more lines instead of
/// widening the column.
fn document(long_row: Option<usize>) -> String {
    let mut md =
        String::from("# Table\n\nIntro paragraph before the table.\n\n| Key | Note |\n|---|---|\n");
    for i in 0..ROWS {
        let note = if Some(i) == long_row {
            "a note that is much longer than the others and wraps onto several more lines \
             in the capped note column, because the column cannot grow past the pane and \
             the words have to go somewhere, so the row grows taller instead of wider and \
             the table with it"
        } else {
            "a table cell note that is long enough to fill the capped column on its own"
        };
        md.push_str(&format!("| key_{i} | {note} row {i} |\n"));
    }
    md.push_str("\n## After the table\n\nClosing paragraph.\n");
    md
}

fn render(
    ctx: &Context,
    cache: &mut CommonMarkCache,
    md: &str,
    width: f32,
    offset: Option<f32>,
) -> f32 {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(width + 40.0, 500.0),
        )),
        ..Default::default()
    };
    let mut height = 0.0;
    ctx.run_frame(input, |ctx| {
        egui::CentralPanel::default().show_ctx(ctx, |ui| {
            ui.set_max_width(width);
            height = CommonMarkViewer::new()
                .default_width(Some(width as usize))
                .table_max_width(Some(width as usize))
                .pending_scroll_offset(offset)
                .show_scrollable("doc", ui, cache, md)
                .content_size
                .y;
        });
    });
    height
}

/// The document's height once its layout has settled. A table's column
/// widths still move on its second and third painted frames (the stored
/// widths and egui_extras' own state catch up with each other), and each move
/// measures the rows again, with or without the measurement cache.
fn settled_height(ctx: &Context, cache: &mut CommonMarkCache, md: &str, width: f32) -> f32 {
    let mut height = 0.0;
    for _ in 0..6 {
        height = render(ctx, cache, md, width, None);
    }
    height
}

fn fresh_height(md: &str, width: f32, body_size: Option<f32>) -> f32 {
    let ctx = context(body_size);
    settled_height(&ctx, &mut CommonMarkCache::default(), md, width)
}

fn context(body_size: Option<f32>) -> Context {
    let ctx = Context::default();
    if let Some(size) = body_size {
        set_body_size(&ctx, size);
    }
    ctx
}

fn set_body_size(ctx: &Context, size: f32) {
    let mut style = (*ctx.global_style()).clone();
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(size));
    ctx.set_global_style(style);
}

fn assert_close(got: f32, want: f32, what: &str) {
    assert!(
        (got - want).abs() <= 0.5,
        "{what}: {got:.1}, a fresh layout gives {want:.1}"
    );
}

#[test]
fn a_settled_table_measures_no_cell_while_it_scrolls() {
    let md = document(None);
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    settled_height(&ctx, &mut cache, &md, 600.0);

    let before = table_cells_measured_for_test();
    for step in 1..=10 {
        render(&ctx, &mut cache, &md, 600.0, Some(step as f32 * 400.0));
    }
    let measured = table_cells_measured_for_test() - before;
    assert_eq!(
        measured, 0,
        "scrolling a settled {ROWS}-row table measured {measured} cells"
    );
}

/// The edited table keeps the column widths it had before (they are stored per
/// table and survive a content change, with or without the measurement
/// cache), so its height is compared with the unedited table at those widths
/// rather than with a fresh layout, whose optimizer may pick other widths.
#[test]
fn a_longer_cell_makes_the_table_taller() {
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let before = settled_height(&ctx, &mut cache, &document(None), 600.0);

    let edited = document(Some(150));
    render(&ctx, &mut cache, &edited, 600.0, None);
    let after = render(&ctx, &mut cache, &edited, 600.0, None);
    // The longer note wraps onto at least two more lines.
    assert!(
        after > before + 30.0,
        "the longer note did not make the table taller ({before:.1} -> {after:.1})"
    );
}

#[test]
fn a_larger_font_measures_the_rows_again() {
    let md = document(None);
    let ctx = context(Some(14.0));
    let mut cache = CommonMarkCache::default();
    let small = settled_height(&ctx, &mut cache, &md, 600.0);

    set_body_size(&ctx, 24.0);
    let large = settled_height(&ctx, &mut cache, &md, 600.0);
    assert!(large > small * 1.3, "{small:.1} -> {large:.1}");
    assert_close(
        large,
        fresh_height(&md, 600.0, Some(24.0)),
        "at the larger font",
    );
}

#[test]
fn a_narrower_pane_rewraps_the_rows() {
    let md = document(None);
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let wide = settled_height(&ctx, &mut cache, &md, 600.0);

    let narrow = settled_height(&ctx, &mut cache, &md, 360.0);
    assert!(narrow > wide, "{wide:.1} -> {narrow:.1}");
    assert_close(
        narrow,
        fresh_height(&md, 360.0, None),
        "in the narrower pane",
    );
}
