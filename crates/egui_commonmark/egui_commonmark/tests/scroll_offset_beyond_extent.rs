//! Regression probe for #140: a stored scroll offset that exceeds the
//! document's updated content extent must not produce a frame with no visible
//! document content.
//!
//! X11 screenshot guards sample settled states and md-viewer repaints on
//! demand, so a one-frame artifact is outside their reach by construction
//! (see the issue thread). This test drives single frames in-process and
//! inspects every painted shape, which is the only way to see the frame the
//! issue describes.

use egui::{Context, Pos2, Rect, Shape, Vec2};
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};

struct Frame {
    out: egui::scroll_area::ScrollAreaOutput<()>,
    /// World-space rects of every painted text shape, with their text.
    texts: Vec<(String, Rect)>,
}

const VIEWPORT_WIDTH: f32 = 600.0;
const VIEWPORT_HEIGHT: f32 = 500.0;

fn render_frame(ctx: &Context, cache: &mut CommonMarkCache, md: &str) -> Frame {
    let mut out = None;
    let mut texts: Vec<(String, Rect)> = Vec::new();
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(VIEWPORT_WIDTH + 40.0, VIEWPORT_HEIGHT),
        )),
        ..Default::default()
    };
    let full = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.set_max_width(VIEWPORT_WIDTH);
            out = Some(
                CommonMarkViewer::new()
                    .default_width(Some(600))
                    .table_max_width(Some(600))
                    .show_scrollable("doc", ui, cache, md),
            );
        });
    });
    let out = out.expect("panel ran once");
    for clipped in full.shapes {
        if let Shape::Text(text) = &clipped.shape {
            let rect = text.galley.rect.translate(text.pos.to_vec2());
            let label: String = text.galley.job.text.chars().take(24).collect();
            texts.push((label, rect));
        }
        if let Shape::Vec(shapes) = &clipped.shape {
            for shape in shapes {
                if let Shape::Text(text) = shape {
                    let rect = text.galley.rect.translate(text.pos.to_vec2());
                    let label: String = text.galley.job.text.chars().take(24).collect();
                    texts.push((label, rect));
                }
            }
        }
    }
    Frame { out, texts }
}

fn visible_texts(frame: &Frame) -> Vec<&(String, Rect)> {
    // Anything the clip actually shows: the text rect intersects the scroll
    // area's inner rect (small vertical slack for anti-aliased edges).
    let window = frame.out.inner_rect;
    frame
        .texts
        .iter()
        .filter(|(_, rect)| rect.intersects(window.expand(1.0)))
        .collect()
}

fn document() -> String {
    (1..=60)
        .map(|i| format!("Paragraph {i} of sixty. Distinct stable prose for geometry probes.\n\n"))
        .collect()
}

#[test]
fn offset_beyond_extent_frame_still_paints_visible_content() {
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let md = document();

    // Bootstrap plus settle: by frame 3 the sliced path is active with a
    // measured page size and a settled scroll state.
    let mut settled = None;
    for _ in 0..3 {
        settled = Some(render_frame(&ctx, &mut cache, &md));
    }
    let settled = settled.unwrap();
    let content_height = settled.out.content_size.y;
    assert!(
        content_height > settled.out.inner_rect.height() * 3.0,
        "fixture document must scroll (content {content_height:.1} px, viewport {:.1} px)",
        settled.out.inner_rect.height()
    );
    assert!(
        !visible_texts(&settled).is_empty(),
        "sanity: settled frame shows content"
    );
    let viewport_bottom = settled.out.state.offset.y + settled.out.inner_rect.height();
    assert!(
        viewport_bottom <= content_height + 0.5,
        "sanity: settled offset within extent (bottom {viewport_bottom:.1} of {content_height:.1})"
    );

    // The issue's precondition: a stored offset beyond the updated extent.
    // In the field this arrives when async layout (image decode, font
    // fallback) shortens the document while the offset sits deep; here it is
    // injected directly so the critical frame is deterministic.
    let stale_offset = content_height + 2000.0;
    let mut stale = settled.out.state;
    stale.offset = Vec2::new(0.0, stale_offset);
    stale.store(&ctx, settled.out.id);

    // The frame under test: egui computes the viewport from the stored offset
    // before the renderer selects its slice (scroll_area.rs: `viewport =
    // Rect::from_min_size(ZERO + state.offset, inner_size)`, unclamped).
    let critical = render_frame(&ctx, &mut cache, &md);

    let visible = visible_texts(&critical);
    let viewport = critical.out.inner_rect;
    eprintln!(
        "critical frame: offset_in {:.1} offset_out {:.1} content {:.1} viewport [{:.1}..{:.1}] texts_total {} texts_visible {}",
        stale_offset,
        critical.out.state.offset.y,
        content_height,
        viewport.min.y,
        viewport.max.y,
        critical.texts.len(),
        visible.len(),
    );
    for (label, rect) in visible.iter().take(6) {
        eprintln!(
            "  visible: {label:?} at y {:.1}..{:.1}",
            rect.top(),
            rect.bottom()
        );
    }

    assert!(
        !visible.is_empty(),
        "a frame whose stored offset exceeds the content extent painted nothing \
         visible — the one-frame blank of #140"
    );
    assert!(
        !visible_texts(&render_frame(&ctx, &mut cache, &md)).is_empty(),
        "the frame after the critical one must recover to visible content"
    );
}
