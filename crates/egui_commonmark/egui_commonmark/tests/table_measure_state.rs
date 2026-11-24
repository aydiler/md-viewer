//! A table's column widths do not depend on whether a measuring pass laid it
//! out first (devlog 122).
//!
//! `show_scrollable` measures the document in an invisible pass, then paints
//! the viewport slice in the same frame. Tables persist their widths and bound
//! between frames; when the measuring pass stored them, the painted table saw
//! "last frame's" widths on its first paint, capped its columns by them while
//! its own egui_extras state was fresh, and every column shrank by 2 pt one
//! frame earlier than on main. In the visual-regression fixture that put the
//! shrink before the reflow that follows the math images, so a note column
//! wrapped that fits on main.
//!
//! The shrink itself also happens on main, one frame later (frame 2 here); it
//! is a separate table-width issue (devlog 122, Future Improvements). This
//! test pins only that a measuring pass leaves the painted table alone.

mod common;
#[allow(unused_imports)]
use common::{FrameExt as _, PanelExt as _};

use egui::{Context, Id, Pos2, Rect};
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};

const WIDTH: f32 = 600.0;

fn document() -> (String, usize) {
    let mut md = String::from("# Table\n\nIntro paragraph before the table.\n\n");
    let table_start = md.len();
    md.push_str("| Signal | Type | Notes |\n|---|---|---|\n");
    for i in 0..30 {
        md.push_str(&format!(
            "| signal_{i} | type_{i} | a fairly long table-cell note for row {i} |\n"
        ));
    }
    md.push_str("\n## After the table\n\nClosing paragraph.\n");
    (md, table_start)
}

fn stored_widths(ctx: &Context, table_start: usize) -> Option<Vec<f32>> {
    let key = Id::unique("doc")
        .with("_markdown_table")
        .with(table_start)
        .with("_column_widths");
    ctx.data(|data| data.get_temp::<Vec<f32>>(key))
}

fn render(ctx: &Context, cache: &mut CommonMarkCache, md: &str) {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(WIDTH + 40.0, 500.0),
        )),
        ..Default::default()
    };
    ctx.run_frame(input, |ctx| {
        egui::CentralPanel::default().show_ctx(ctx, |ui| {
            ui.set_max_width(WIDTH);
            CommonMarkViewer::new()
                .default_width(Some(WIDTH as usize))
                .table_max_width(Some(WIDTH as usize))
                .show_scrollable("doc", ui, cache, md);
        });
    });
}

#[test]
fn a_measuring_pass_leaves_the_painted_table_alone() {
    let (md, table_start) = document();
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    // Frame 0 measures the document and paints the slice; frame 1 paints
    // the slice from the state frame 0's painted table left.
    render(&ctx, &mut cache, &md);
    let first = stored_widths(&ctx, table_start).expect("the painted table stores its widths");
    render(&ctx, &mut cache, &md);
    let second = stored_widths(&ctx, table_start).expect("the painted table stores its widths");
    assert_eq!(
        second, first,
        "the frame after the measuring frame changed the table's column widths"
    );
}
