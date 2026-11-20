//! Code blocks laid out off screen are not syntax-highlighted until they are
//! visible (devlog 101).
//!
//! The scrollable bootstrap lays out every block of the document in its first
//! frame, and syntect dominated that frame on code-heavy documents. These tests
//! pin the two halves of the change: the first frame highlights only what can
//! be seen, and leaving the rest plain does not move anything.

mod common;
#[allow(unused_imports)]
use common::{FrameExt as _, PanelExt as _};

use egui::{Context, Pos2, Rect, Vec2};
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};

const WIDTH: f32 = 600.0;
const BLOCKS: usize = 60;

fn document() -> String {
    (0..BLOCKS)
        .map(|i| {
            format!(
                "Block {i} introduction.\n\n```rs\nfn f{i}(x: i32) -> i32 {{\n    // comment {i}\n    let y = x * {i};\n    let s = \"text {i}\";\n    y + s.len() as i32\n}}\n```\n\n"
            )
        })
        .collect()
}

fn frame(
    ctx: &Context,
    cache: &mut CommonMarkCache,
    md: &str,
    height: f32,
) -> egui::scroll_area::ScrollAreaOutput<()> {
    let mut out = None;
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(WIDTH + 40.0, height))),
        ..Default::default()
    };
    ctx.run_frame(input, |ctx| {
        egui::CentralPanel::default().show_ctx(ctx, |ui| {
            ui.set_max_width(WIDTH);
            out = Some(
                CommonMarkViewer::new()
                    .default_width(Some(600))
                    .show_scrollable("doc", ui, cache, md),
            );
        });
    });
    out.expect("panel ran once")
}

#[test]
fn first_frame_highlights_only_the_visible_code_blocks() {
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let md = document();

    let first = frame(&ctx, &mut cache, &md, 500.0);
    let highlighted = cache.highlighted_code_block_count();
    assert!(
        first.content_size.y > first.inner_rect.height() * 10.0,
        "fixture must be much taller than the viewport (content {:.0}, viewport {:.0})",
        first.content_size.y,
        first.inner_rect.height()
    );
    assert!(highlighted > 0, "the visible code blocks were not highlighted");
    assert!(
        highlighted < BLOCKS / 4,
        "the first frame highlighted {highlighted} of {BLOCKS} code blocks; \
         only the few inside the viewport should pay for syntect"
    );
}

#[test]
fn scrolling_to_a_block_highlights_it() {
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let md = document();

    let mut out = frame(&ctx, &mut cache, &md, 500.0);
    for _ in 0..2 {
        out = frame(&ctx, &mut cache, &md, 500.0);
    }
    let before = cache.highlighted_code_block_count();
    let deep = out.content_size.y * 0.6;
    common::store_offset(&ctx, out.id, out.state, Vec2::new(0.0, deep));
    for _ in 0..2 {
        frame(&ctx, &mut cache, &md, 500.0);
    }
    assert!(
        cache.highlighted_code_block_count() > before,
        "scrolling to blocks that were laid out plain did not highlight them \
         ({before} before, {} after)",
        cache.highlighted_code_block_count()
    );
}

#[test]
fn plain_off_screen_blocks_do_not_change_the_document_extent() {
    // Reference: a viewport tall enough to show the whole document, so every
    // block is highlighted in the measuring pass.
    let ctx_all = Context::default();
    let mut cache_all = CommonMarkCache::default();
    let md = document();
    let mut all = frame(&ctx_all, &mut cache_all, &md, 100_000.0);
    for _ in 0..2 {
        all = frame(&ctx_all, &mut cache_all, &md, 100_000.0);
    }
    assert_eq!(
        cache_all.highlighted_code_block_count(),
        BLOCKS,
        "reference run must highlight every block, or it compares nothing"
    );

    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let mut lazy = frame(&ctx, &mut cache, &md, 500.0);
    for _ in 0..2 {
        lazy = frame(&ctx, &mut cache, &md, 500.0);
    }
    assert!(cache.highlighted_code_block_count() < BLOCKS);
    assert!(
        (lazy.content_size.y - all.content_size.y).abs() < 0.5,
        "document extent differs: {:.2} with off-screen blocks plain, {:.2} with all highlighted",
        lazy.content_size.y,
        all.content_size.y
    );
}
