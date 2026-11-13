//! Confirms the viewport-slice path is active and cheaper than a full paint.
mod common;
#[allow(unused_imports)]
use common::{FrameExt as _, PanelExt as _};

use egui::Context;
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};
use std::time::Instant;

fn big_doc() -> String {
    let mut m = String::from("# Large\n\n");
    for s in 0..300 {
        m.push_str(&format!("\n## Section {s}\n\nParagraph {s} with prose text to wrap.\n"));
        if s % 5 == 0 {
            m.push_str("\n| A | B | C | D |\n|---|---|---|---|\n");
            for r in 0..20 {
                m.push_str(&format!("| sig_{s}_{r} | t_{r} | {r}.5 | a fairly long note for row {r} |\n"));
            }
        }
        if s % 4 == 0 {
            m.push_str(&format!("\n```rust\nfn f_{s}() {{ println!(\"{s}\"); }}\n```\n"));
        }
    }
    m
}

#[test]
fn slice_paint_is_cheaper_than_full_paint() {
    let md = big_doc();
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let mut initial_h = 0.0;
    let mut bootstrap_ms = 0.0;
    let mut slice_times = Vec::new();

    for pass in 0..14 {
        let start = Instant::now();
        ctx.begin_pass(Default::default());
        egui::CentralPanel::default().show_ctx(&ctx, |ui| {
            ui.set_width(700.0);
            ui.set_height(500.0);
            let out = CommonMarkViewer::new().show_scrollable("perf", ui, &mut cache, &md);
            if pass == 0 {
                initial_h = out.content_size.y;
            }
            // Park deep in the document and stay there.
            if pass >= 1 {
                common::store_offset(ui.ctx(), out.id, out.state, egui::vec2(0.0, initial_h * 0.6));
            }
        });
        let _ = ctx.end_frame();
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        if pass == 0 { bootstrap_ms = ms; }
        if pass >= 4 { slice_times.push(ms); }   // let it settle first
    }

    slice_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = slice_times[slice_times.len() / 2];
    println!("document height   : {initial_h:.0}px");
    println!("bootstrap paint   : {bootstrap_ms:.1} ms");
    println!("slice paint median: {median:.1} ms  ({:.1}x faster)", bootstrap_ms / median);
    assert!(
        median < bootstrap_ms,
        "slice paint ({median:.1} ms) is not cheaper than a full paint ({bootstrap_ms:.1} ms) — virtualization is not doing anything"
    );
}
