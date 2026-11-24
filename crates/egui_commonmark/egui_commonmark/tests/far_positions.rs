//! Positions far down a very tall document (devlog 122).
//!
//! An f32 has a spacing of 1/32 px up to 2^19 px and 1/4 px at 2^21 px, while
//! egui snaps layout to a 1/32 px grid. A layout pass placed millions of
//! pixels from the screen origin therefore rounds every row, and the error
//! adds up: in md-viewer an outline jump to the last heading of a 2.16 M px
//! document landed about 458 px off. Every measuring pass must lay its rows
//! out near the screen origin and add the content offset once.
//!
//! The fixture is one section repeated, so heading `k` must sit exactly
//! `k` section heights below heading 0. The text styles are large so that a
//! debug build reaches more than 2 M px with few blocks; the text is
//! non-breaking spaces, so no huge glyphs are rasterized.

mod common;
#[allow(unused_imports)]
use common::{FrameExt as _, PanelExt as _};

use egui::{Context, FontId, Id, Pos2, Rect, TextStyle};
use egui_commonmark_backend_extended::misc::{header_position_key, scroll_cache};
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};

const WIDTH: f32 = 600.0;
const VIEWPORT_HEIGHT: f32 = 500.0;
const BODY_SIZE: f32 = 300.3;
/// 125 % display scaling. Rows snap to physical pixels (0.8 pt), which egui
/// then rounds to its 1/32 pt grid: values f32 cannot hold millions of pixels
/// down. At 1.0 every row height here is a multiple of 0.5 pt, which f32
/// holds exactly at any height, and the test could not fail.
const PIXELS_PER_POINT: f32 = 1.25;
const SECTIONS: usize = 1_600;
const SECTION: &str = "## &nbsp;\n\n&nbsp;\n\n";

#[derive(Clone, Copy, Default)]
struct Mode {
    progressive: bool,
    blocks: Option<usize>,
    pending_offset: Option<f32>,
    force_full_render: bool,
    target: Option<usize>,
}

fn context() -> Context {
    let ctx = Context::default();
    let mut style = (*ctx.global_style()).clone();
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(BODY_SIZE));
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(BODY_SIZE * 1.5));
    ctx.set_global_style(style);
    ctx.set_pixels_per_point(PIXELS_PER_POINT);
    ctx
}

fn document() -> String {
    SECTION.repeat(SECTIONS)
}

fn heading_starts() -> Vec<usize> {
    (0..SECTIONS).map(|k| k * SECTION.len()).collect()
}

fn render(ctx: &Context, cache: &mut CommonMarkCache, md: &str, mode: Mode) {
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
            CommonMarkViewer::new()
                .default_width(Some(WIDTH as usize))
                .progressive_measure(mode.progressive)
                .progressive_measure_blocks(mode.blocks)
                .pending_scroll_offset(mode.pending_offset)
                .force_full_render(mode.force_full_render)
                .measure_to_source(mode.target)
                .show_scrollable("doc", ui, cache, md);
        });
    });
}

fn measuring(cache: &mut CommonMarkCache) -> bool {
    scroll_cache(cache, &Id::unique("doc"))
        .measure_frontier
        .is_some()
}

fn page_height(cache: &mut CommonMarkCache) -> f32 {
    scroll_cache(cache, &Id::unique("doc"))
        .page_size
        .map_or(0.0, |size| size.y)
}

/// Heading positions must step by one section height all the way down.
fn assert_evenly_spaced(cache: &mut CommonMarkCache, what: &str) {
    let ys: Vec<f32> = heading_starts()
        .into_iter()
        .map(|start| {
            cache
                .get_header_position(&header_position_key(start))
                .unwrap_or_else(|| panic!("{what}: heading at byte {start} has no position"))
        })
        .collect();
    // Sections 1 and 2 are laid out near the top, where f32 is exact.
    let step = ys[2] - ys[1];
    let last = ys.len() - 1;
    assert!(
        ys[last] > 2_000_000.0,
        "{what}: the fixture must reach past 2 M px, last heading at {:.1}",
        ys[last]
    );
    let worst = (1..ys.len())
        .map(|k| (k, ys[k] - (ys[1] + (k as f32 - 1.0) * step)))
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .unwrap();
    assert!(
        worst.1.abs() <= 1.0,
        "{what}: heading {} is {:.2} px off its place (step {step:.3}, last at {:.1}, extent {:.1})",
        worst.0,
        worst.1,
        ys[last],
        page_height(cache)
    );
}

/// Progressive measurement at the top of the document: the later chunks are
/// laid out in an invisible child.
#[test]
fn progressive_measurement_keeps_far_headings_in_place() {
    let md = document();
    let ctx = context();
    let mut cache = CommonMarkCache::default();
    let mode = Mode {
        progressive: true,
        blocks: Some(256),
        ..Mode::default()
    };
    for frame in 0.. {
        assert!(frame < 1_000, "measurement did not finish");
        render(&ctx, &mut cache, &md, mode);
        if frame > 0 && !measuring(&mut cache) {
            break;
        }
    }
    assert_evenly_spaced(&mut cache, "progressive");
}

/// A one-frame bootstrap of the whole document (library default).
#[test]
fn one_frame_bootstrap_keeps_far_headings_in_place() {
    let md = document();
    let ctx = context();
    let mut cache = CommonMarkCache::default();
    render(&ctx, &mut cache, &md, Mode::default());
    assert_evenly_spaced(&mut cache, "one-frame bootstrap");
}

/// Two frames of progressive measurement, then a requested offset deep in
/// the document but inside the estimated extent: the viewport pass measures
/// up to it in one go, later frames measure the rest.
#[test]
fn catching_up_to_a_deep_offset_keeps_far_headings_in_place() {
    let md = document();
    let ctx = context();
    let mut cache = CommonMarkCache::default();
    let progressive = Mode {
        progressive: true,
        blocks: Some(2),
        ..Mode::default()
    };
    render(&ctx, &mut cache, &md, progressive);
    assert!(measuring(&mut cache));
    let deep = 0.8 * page_height(&mut cache);
    assert!(deep > 1_500_000.0, "estimate {deep:.0} too small");
    render(
        &ctx,
        &mut cache,
        &md,
        Mode {
            pending_offset: Some(deep),
            ..progressive
        },
    );
    let mut frames = 0;
    while measuring(&mut cache) {
        frames += 1;
        assert!(frames < 2_000, "measurement did not finish");
        render(
            &ctx,
            &mut cache,
            &md,
            Mode {
                blocks: Some(256),
                ..progressive
            },
        );
    }
    assert_evenly_spaced(&mut cache, "catch-up to a deep offset");
}

/// A requested offset past the estimated extent measures the whole document
/// on the frame that scrolls there.
#[test]
fn an_offset_past_the_estimate_keeps_far_headings_in_place() {
    let md = document();
    let ctx = context();
    let mut cache = CommonMarkCache::default();
    let progressive = Mode {
        progressive: true,
        blocks: Some(2),
        ..Mode::default()
    };
    render(&ctx, &mut cache, &md, progressive);
    assert!(measuring(&mut cache));
    let past = page_height(&mut cache) + 1_000.0;
    render(
        &ctx,
        &mut cache,
        &md,
        Mode {
            pending_offset: Some(past),
            ..progressive
        },
    );
    assert!(
        !measuring(&mut cache),
        "an offset past the estimate measures it all"
    );
    assert_evenly_spaced(&mut cache, "offset past the estimate");
}

/// The search jump's `force_full_render` at a deep offset.
#[test]
fn force_full_render_at_a_deep_offset_keeps_far_headings_in_place() {
    let md = document();
    let ctx = context();
    let mut cache = CommonMarkCache::default();
    let progressive = Mode {
        progressive: true,
        blocks: Some(2),
        ..Mode::default()
    };
    render(&ctx, &mut cache, &md, progressive);
    let deep = 0.8 * page_height(&mut cache);
    render(
        &ctx,
        &mut cache,
        &md,
        Mode {
            pending_offset: Some(deep),
            force_full_render: true,
            ..progressive
        },
    );
    assert!(!measuring(&mut cache), "force_full_render measures it all");
    assert_evenly_spaced(&mut cache, "force_full_render at a deep offset");
}
