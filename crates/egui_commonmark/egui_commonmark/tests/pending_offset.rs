//! A requested scroll offset past the deepest position (devlog 122).
//!
//! md-viewer jumps to a heading near the end of a document by asking for the
//! heading's offset, which can lie past the deepest position the document can
//! scroll to. egui 0.37 treats an offset past its bounds as overscroll and
//! springs back with a velocity; the renderer's clamp then set the offset to
//! the deepest position on the next frame, but the velocity stayed and slid
//! the view a few hundred points up, past the heading the jump was for.
//!
//! The requested offset must land at the bottom of the measured document and
//! stay there. (The scroll range then reaches a little further, by the
//! renderer's trailing padding, which the extent learns once the bottom is
//! painted; landing on the measured bottom shows the document's last line.)

mod common;
#[allow(unused_imports)]
use common::{FrameExt as _, PanelExt as _};

use egui::{Context, Id, Pos2, Rect};
use egui_commonmark_backend_extended::misc::scroll_cache;
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};

const WIDTH: f32 = 600.0;
const VIEWPORT_HEIGHT: f32 = 500.0;

fn document() -> String {
    let mut md = String::new();
    for i in 1..=60 {
        md.push_str(&format!("## Section {i}\n\n"));
        md.push_str(&format!(
            "Section {i} opens with a paragraph long enough to wrap across several \
             lines at this width, so that the document is many screens tall.\n\n"
        ));
    }
    md
}

struct Frame {
    offset: f32,
    velocity: f32,
    /// Deepest offset that shows the measured bottom of the document.
    measured_bottom: f32,
}

fn render(
    ctx: &Context,
    cache: &mut CommonMarkCache,
    md: &str,
    progressive: bool,
    pending: Option<f32>,
) -> Frame {
    let mut out = None;
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(WIDTH + 40.0, VIEWPORT_HEIGHT),
        )),
        ..Default::default()
    };
    ctx.run_frame(input, |ctx| {
        egui::CentralPanel::default().show_ctx(ctx, |ui| {
            ui.set_max_width(WIDTH);
            out = Some(
                CommonMarkViewer::new()
                    .default_width(Some(WIDTH as usize))
                    .progressive_measure(progressive)
                    .progressive_measure_blocks(progressive.then_some(2))
                    .pending_scroll_offset(pending)
                    .show_scrollable("doc", ui, cache, md),
            );
        });
    });
    let out = out.expect("panel ran once");
    let page_height = scroll_cache(cache, &Id::unique("doc"))
        .page_size
        .map_or(0.0, |size| size.y);
    Frame {
        offset: out.state.unclamped_offset().y,
        velocity: out.state.velocity().y,
        measured_bottom: (page_height - out.inner_rect.height()).max(0.0),
    }
}

/// Frames after the request: the view must reach the measured bottom and
/// not move from it.
fn assert_settles_at_the_bottom(
    ctx: &Context,
    cache: &mut CommonMarkCache,
    md: &str,
    progressive: bool,
    what: &str,
) {
    let mut frames = Vec::new();
    for _ in 0..60 {
        frames.push(render(ctx, cache, md, progressive, None));
    }
    for (i, frame) in frames.iter().enumerate().skip(2) {
        assert!(
            (frame.offset - frame.measured_bottom).abs() <= 1.0 && frame.velocity == 0.0,
            "{what}: frame {i} after the request is at {:.1} moving at {:.1}/s, \
             the measured bottom is at {:.1}",
            frame.offset,
            frame.velocity,
            frame.measured_bottom
        );
    }
}

#[test]
fn an_offset_past_the_deepest_position_lands_there_and_stays() {
    let md = document();
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let mut measured = render(&ctx, &mut cache, &md, false, None);
    for _ in 0..3 {
        measured = render(&ctx, &mut cache, &md, false, None);
    }
    assert!(
        measured.measured_bottom > 2.0 * VIEWPORT_HEIGHT,
        "fixture too short"
    );
    // Past the deepest position, inside the document: a heading near the end.
    let request = measured.measured_bottom + 0.6 * VIEWPORT_HEIGHT;
    render(&ctx, &mut cache, &md, false, Some(request));
    assert_settles_at_the_bottom(&ctx, &mut cache, &md, false, "measured document");
}

#[test]
fn an_offset_requested_while_measuring_lands_at_the_bottom_and_stays() {
    let md = document();
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    render(&ctx, &mut cache, &md, true, None);
    assert!(
        scroll_cache(&mut cache, &Id::unique("doc"))
            .measure_frontier
            .is_some(),
        "the first frame measures part of the document"
    );
    let estimate = scroll_cache(&mut cache, &Id::unique("doc"))
        .page_size
        .map_or(0.0, |size| size.y);
    // Past the estimated extent, so the frame measures the whole document.
    render(&ctx, &mut cache, &md, true, Some(estimate));
    assert_settles_at_the_bottom(&ctx, &mut cache, &md, true, "measuring document");
}
