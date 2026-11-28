//! A markdown table keeps its measurements across frames (devlog 131).
//!
//! Painting a table was already limited to the rows on screen, but every
//! frame measured every row: the reserved height and the body's row heights,
//! each cell's natural width and a digest of the whole table. Scrolling
//! inside a 1 MB table cost 265 ms per frame. Now a settled table measures
//! nothing while it scrolls, and anything that changes a measurement (the
//! table's text, the font, the column widths) measures it again.
//!
//! A stale row height does not show in the document's extent: the cell's text
//! is laid out where it really wraps, and the extent follows it. It shows as
//! rows painted over each other, which is what these tests check.

mod common;
#[allow(unused_imports)]
use common::{FrameExt as _, PanelExt as _};

use egui::{Context, FontId, Pos2, Rect, Shape, TextStyle};
use egui_commonmark_extended::{table_cells_measured_for_test, CommonMarkCache, CommonMarkViewer};

const ROWS: usize = 300;
/// A row near the top, painted without scrolling.
const EDITED_ROW: usize = 2;

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

struct Frame {
    height: f32,
    /// Painted text: the galley's text and its screen rect.
    texts: Vec<(String, Rect)>,
}

fn render(
    ctx: &Context,
    cache: &mut CommonMarkCache,
    md: &str,
    width: f32,
    offset: Option<f32>,
) -> Frame {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(width + 40.0, 1000.0),
        )),
        ..Default::default()
    };
    let mut height = 0.0;
    let full = ctx.run_frame(input, |ctx| {
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
    let mut texts = Vec::new();
    for clipped in full.shapes {
        collect_texts(&clipped.shape, &mut texts);
    }
    Frame { height, texts }
}

fn collect_texts(shape: &Shape, texts: &mut Vec<(String, Rect)>) {
    match shape {
        Shape::Text(text) => texts.push((
            text.galley.text().to_owned(),
            text.galley.rect.translate(text.pos.to_vec2()),
        )),
        Shape::Vec(shapes) => shapes.iter().for_each(|s| collect_texts(s, texts)),
        _ => {}
    }
}

/// Every painted row starts below the text of the row above it. A row height
/// measured for other text, another font or other column widths lets a cell
/// that wraps onto more lines than its row reserved run into the next row.
fn assert_rows_stacked(frame: &Frame, what: &str) {
    let find = |wanted: &dyn Fn(&str) -> bool| {
        frame
            .texts
            .iter()
            .find(|(text, _)| wanted(text))
            .map(|(_, rect)| *rect)
    };
    let mut pairs = 0;
    for row in 0..ROWS - 1 {
        let note_end = format!("row {row}");
        let next_key = format!("key_{}", row + 1);
        let (Some(note), Some(key)) = (
            find(&|text: &str| text.trim_end().ends_with(&note_end)),
            find(&|text: &str| text == next_key),
        ) else {
            continue;
        };
        assert!(
            key.top() >= note.bottom() - 0.5,
            "{what}: row {} starts at {:.1}, inside row {row}'s note, which ends at {:.1}",
            row + 1,
            key.top(),
            note.bottom()
        );
        pairs += 1;
    }
    assert!(
        pairs >= 5,
        "{what}: only {pairs} painted row pairs to compare"
    );
}

/// The layout after enough frames for a table's column widths to settle: they
/// still move on its second and third painted frames (the stored widths and
/// egui_extras' own state catch up with each other), and each move measures
/// the rows again, with or without the measurement cache.
fn settle(ctx: &Context, cache: &mut CommonMarkCache, md: &str, width: f32) -> Frame {
    for _ in 0..5 {
        render(ctx, cache, md, width, None);
    }
    render(ctx, cache, md, width, None)
}

fn fresh_height(md: &str, width: f32, body_size: Option<f32>) -> f32 {
    let ctx = context(body_size);
    settle(&ctx, &mut CommonMarkCache::default(), md, width).height
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
    settle(&ctx, &mut cache, &md, 600.0);

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
/// cache), so its rows are checked against each other rather than against a
/// fresh layout, whose optimizer may pick other widths.
#[test]
fn a_longer_cell_is_measured_again() {
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let before = settle(&ctx, &mut cache, &document(None), 600.0);
    assert_rows_stacked(&before, "before the edit");

    let measured_before = table_cells_measured_for_test();
    let after = settle(&ctx, &mut cache, &document(Some(EDITED_ROW)), 600.0);
    assert!(
        table_cells_measured_for_test() > measured_before,
        "the edited table was not measured again"
    );
    assert!(
        after.height > before.height + 30.0,
        "the longer note did not make the table taller ({:.1} -> {:.1})",
        before.height,
        after.height
    );
    assert_rows_stacked(&after, "after the edit");
}

/// The stored column widths survive a font change too (with or without the
/// measurement cache), so, as after an edit, the rows are checked against
/// each other rather than against a fresh layout.
#[test]
fn a_larger_font_measures_the_rows_again() {
    let md = document(Some(EDITED_ROW));
    let ctx = context(Some(14.0));
    let mut cache = CommonMarkCache::default();
    let small = settle(&ctx, &mut cache, &md, 600.0);
    assert_rows_stacked(&small, "at the smaller font");

    set_body_size(&ctx, 24.0);
    let large = settle(&ctx, &mut cache, &md, 600.0);
    assert_rows_stacked(&large, "at the larger font");
    assert!(
        large.height > small.height * 1.3,
        "{:.1} -> {:.1}",
        small.height,
        large.height
    );
}

#[test]
fn a_narrower_pane_rewraps_the_rows() {
    let md = document(Some(EDITED_ROW));
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let wide = settle(&ctx, &mut cache, &md, 600.0);
    assert_rows_stacked(&wide, "in the wider pane");

    let narrow = settle(&ctx, &mut cache, &md, 360.0);
    assert_rows_stacked(&narrow, "in the narrower pane");
    assert!(
        narrow.height > wide.height,
        "{:.1} -> {:.1}",
        wide.height,
        narrow.height
    );
    assert_close(
        narrow.height,
        fresh_height(&md, 360.0, None),
        "in the narrower pane",
    );
}
