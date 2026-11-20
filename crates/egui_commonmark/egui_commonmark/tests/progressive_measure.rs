//! Progressive measurement (devlog 103): a document measured over several
//! frames must end with exactly the geometry a one-frame bootstrap records,
//! and no frame along the way may paint past what is measured or paint
//! nothing where the viewport is.

mod common;
#[allow(unused_imports)]
use common::{FrameExt as _, PanelExt as _};

use egui::{Context, Id, Pos2, Rect, Shape, Vec2};
use egui_commonmark_backend_extended::misc::{header_position_key, scroll_cache};
use egui_commonmark_backend_extended::pulldown::MeasureFrontier;
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};

const VIEWPORT_HEIGHT: f32 = 500.0;

struct Frame {
    out: egui::scroll_area::ScrollAreaOutput<()>,
    texts: Vec<Rect>,
}

#[derive(Clone, Copy)]
struct Mode {
    progressive: bool,
    blocks: Option<usize>,
    force_full_render: bool,
}

const ONE_SHOT: Mode = Mode {
    progressive: false,
    blocks: None,
    force_full_render: false,
};

const PROGRESSIVE: Mode = Mode {
    progressive: true,
    blocks: Some(2),
    force_full_render: false,
};

fn render(ctx: &Context, cache: &mut CommonMarkCache, md: &str, width: f32, mode: Mode) -> Frame {
    let mut out = None;
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(width + 40.0, VIEWPORT_HEIGHT),
        )),
        ..Default::default()
    };
    let full = ctx.run_frame(input, |ctx| {
        egui::CentralPanel::default().show_ctx(ctx, |ui| {
            ui.set_max_width(width);
            out = Some(
                CommonMarkViewer::new()
                    .default_width(Some(width as usize))
                    .table_max_width(Some(width as usize))
                    .progressive_measure(mode.progressive)
                    .progressive_measure_blocks(mode.blocks)
                    .force_full_render(mode.force_full_render)
                    .show_scrollable("doc", ui, cache, md),
            );
        });
    });
    let mut texts = Vec::new();
    for clipped in full.shapes {
        collect_texts(&clipped.shape, &mut texts);
    }
    Frame {
        out: out.expect("panel ran once"),
        texts,
    }
}

fn collect_texts(shape: &Shape, texts: &mut Vec<Rect>) {
    match shape {
        Shape::Text(text) => texts.push(text.galley.rect.translate(text.pos.to_vec2())),
        Shape::Vec(shapes) => shapes.iter().for_each(|s| collect_texts(s, texts)),
        _ => {}
    }
}

fn visible_count(frame: &Frame) -> usize {
    let window = frame.out.inner_rect.expand(1.0);
    frame.texts.iter().filter(|r| r.intersects(window)).count()
}

/// Headings, prose, lists, code, tables and blockquotes: every block kind the
/// split logic treats differently, over many screens.
fn document() -> String {
    let mut md = String::new();
    for i in 1..=40 {
        md.push_str(&format!("## Section {i}\n\n"));
        md.push_str(&format!(
            "Section {i} opens with a paragraph long enough to wrap across several \
             lines at the narrow width, so that measurement depends on line breaking \
             and not only on block count.\n\n"
        ));
        md.push_str("- first item\n- second item with more words in it\n  - nested item\n\n");
        if i % 3 == 0 {
            md.push_str("```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n\n");
        }
        if i % 4 == 0 {
            md.push_str("| key | value |\n|---|---|\n| alpha | a longer cell that wraps |\n| beta | b |\n\n");
        }
        if i % 5 == 0 {
            md.push_str("> A quoted remark that spans\n> two source lines.\n\n");
        }
        md.push_str(&format!("Closing words of section {i}.\n\n"));
    }
    md
}

fn heading_starts(md: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut offset = 0;
    for line in md.split_inclusive('\n') {
        if line.starts_with("## ") {
            starts.push(offset);
        }
        offset += line.len();
    }
    starts
}

struct Geometry {
    splits: Vec<(usize, f32, f32)>,
    page_height: f32,
    headers: Vec<Option<f32>>,
}

fn geometry(cache: &mut CommonMarkCache, md: &str) -> Geometry {
    let headers = heading_starts(md)
        .into_iter()
        .map(|start| cache.get_header_position(&header_position_key(start)))
        .collect();
    let sc = scroll_cache(cache, &Id::unique("doc"));
    Geometry {
        splits: sc
            .split_points
            .iter()
            .map(|(index, start, end)| (*index, start.y, end.y))
            .collect(),
        page_height: sc.page_size.map_or(0.0, |s| s.y),
        headers,
    }
}

fn frontier(cache: &mut CommonMarkCache) -> Option<MeasureFrontier> {
    scroll_cache(cache, &Id::unique("doc")).measure_frontier
}

fn one_shot_geometry(md: &str, width: f32) -> Geometry {
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    for _ in 0..3 {
        render(&ctx, &mut cache, md, width, ONE_SHOT);
    }
    geometry(&mut cache, md)
}

/// Run progressive frames until measurement completes; returns the frame
/// count it took.
fn run_to_completion(ctx: &Context, cache: &mut CommonMarkCache, md: &str, width: f32) -> usize {
    for frame in 1..=2000 {
        render(ctx, cache, md, width, PROGRESSIVE);
        if frontier(cache).is_none() {
            return frame;
        }
    }
    panic!("progressive measurement did not finish in 2000 frames");
}

fn assert_same_geometry(progressive: &Geometry, reference: &Geometry, what: &str) {
    let indices = |g: &Geometry| g.splits.iter().map(|s| s.0).collect::<Vec<_>>();
    assert_eq!(
        indices(progressive),
        indices(reference),
        "{what}: split point event indices differ"
    );
    for (p, r) in progressive.splits.iter().zip(&reference.splits) {
        assert!(
            (p.1 - r.1).abs() <= 1.0 && (p.2 - r.2).abs() <= 1.0,
            "{what}: split point at event {} is at {:.1}..{:.1}, one-shot {:.1}..{:.1}",
            p.0,
            p.1,
            p.2,
            r.1,
            r.2
        );
    }
    assert!(
        (progressive.page_height - reference.page_height).abs() <= 1.0,
        "{what}: extent {:.1}, one-shot {:.1}",
        progressive.page_height,
        reference.page_height
    );
    for (i, (p, r)) in progressive.headers.iter().zip(&reference.headers).enumerate() {
        let (Some(p), Some(r)) = (p, r) else {
            panic!("{what}: heading {i} position progressive {p:?}, one-shot {r:?}");
        };
        assert!(
            (p - r).abs() <= 1.0,
            "{what}: heading {i} at {p:.1}, one-shot {r:.1}"
        );
    }
}

#[test]
fn progressive_measurement_ends_with_the_one_shot_geometry() {
    let md = document();
    for width in [600.0, 380.0] {
        let reference = one_shot_geometry(&md, width);
        assert!(
            reference.page_height > VIEWPORT_HEIGHT * 8.0,
            "fixture must span many screens (extent {:.1})",
            reference.page_height
        );

        let ctx = Context::default();
        let mut cache = CommonMarkCache::default();
        let frames = run_to_completion(&ctx, &mut cache, &md, width);
        assert!(
            frames > 5,
            "at width {width} measurement finished in {frames} frames: the fixture never \
             exercised the frontier"
        );
        // Header positions of the first screens come from frame 1; the rest
        // from the measuring passes. One more frame paints the top again.
        render(&ctx, &mut cache, &md, width, PROGRESSIVE);
        assert_same_geometry(
            &geometry(&mut cache, &md),
            &reference,
            &format!("width {width}"),
        );
    }
}

#[test]
fn first_frame_measures_only_the_top_and_paints_it() {
    let md = document();
    let reference = one_shot_geometry(&md, 600.0);

    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let first = render(&ctx, &mut cache, &md, 600.0, PROGRESSIVE);
    let after_first = geometry(&mut cache, &md);
    let frontier = frontier(&mut cache).expect("a large document is not measured in one frame");

    assert!(
        after_first.splits.len() < reference.splits.len() / 4,
        "frame 1 recorded {} of {} split points",
        after_first.splits.len(),
        reference.splits.len()
    );
    assert!(
        frontier.y >= 2.0 * VIEWPORT_HEIGHT * 0.9,
        "frame 1 must measure two screens, reached {:.1}",
        frontier.y
    );
    assert!(visible_count(&first) > 0, "frame 1 painted nothing visible");
    assert!(
        after_first.page_height > frontier.y,
        "estimated extent {:.1} must exceed the measured part {:.1}",
        after_first.page_height,
        frontier.y
    );
}

#[test]
fn no_frame_paints_past_the_measured_part() {
    let md = document();
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    render(&ctx, &mut cache, &md, 600.0, PROGRESSIVE);
    for _ in 0..10 {
        let frame = render(&ctx, &mut cache, &md, 600.0, PROGRESSIVE);
        let Some(frontier) = frontier(&mut cache) else {
            panic!("fixture finished measuring within 10 frames");
        };
        let content_top = frame.out.inner_rect.top() - frame.out.state.unclamped_offset().y;
        let deepest = frame
            .texts
            .iter()
            .map(|r| r.bottom() - content_top)
            .fold(0.0_f32, f32::max);
        assert!(
            deepest <= frontier.y + 1.0,
            "painted text reaches content y {deepest:.1}, past the frontier at {:.1}",
            frontier.y
        );
    }
}

#[test]
fn scrolling_into_the_estimated_region_paints_content() {
    let md = document();
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let first = render(&ctx, &mut cache, &md, 600.0, PROGRESSIVE);
    let frontier_before = frontier(&mut cache).expect("partly measured after frame 1");
    let estimate = geometry(&mut cache, &md).page_height;

    // A scrollbar drag deep into the part that is only estimated.
    let target = (estimate * 0.6).max(frontier_before.y + 3.0 * VIEWPORT_HEIGHT);
    common::store_offset(&ctx, first.out.id, first.out.state, Vec2::new(0.0, target));
    let deep = render(&ctx, &mut cache, &md, 600.0, PROGRESSIVE);

    let offset = deep.out.state.unclamped_offset().y;
    assert!(
        offset > frontier_before.y,
        "the offset {offset:.1} must land past the first frame's frontier {:.1}",
        frontier_before.y
    );
    assert!(visible_count(&deep) > 0, "the deep frame painted nothing visible");
    if let Some(after) = frontier(&mut cache) {
        assert!(
            after.y >= offset + deep.out.inner_rect.height(),
            "the frontier {:.1} must cover the viewport ending at {:.1}",
            after.y,
            offset + deep.out.inner_rect.height()
        );
    }
}

#[test]
fn force_full_render_completes_measurement_at_once() {
    let md = document();
    let reference = one_shot_geometry(&md, 600.0);
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    render(&ctx, &mut cache, &md, 600.0, PROGRESSIVE);
    assert!(frontier(&mut cache).is_some());
    render(
        &ctx,
        &mut cache,
        &md,
        600.0,
        Mode {
            force_full_render: true,
            ..PROGRESSIVE
        },
    );
    assert!(frontier(&mut cache).is_none(), "force_full_render left the document partly measured");
    let g = geometry(&mut cache, &md);
    assert_eq!(g.splits.len(), reference.splits.len());
}

#[test]
fn invalidation_mid_measurement_restarts_cleanly() {
    let md = document();
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    for _ in 0..4 {
        render(&ctx, &mut cache, &md, 600.0, PROGRESSIVE);
    }
    assert!(frontier(&mut cache).is_some(), "still measuring at width 600");

    // A width change mid-measurement.
    run_to_completion(&ctx, &mut cache, &md, 380.0);
    render(&ctx, &mut cache, &md, 380.0, PROGRESSIVE);
    assert_same_geometry(
        &geometry(&mut cache, &md),
        &one_shot_geometry(&md, 380.0),
        "after a resize mid-measurement",
    );

    // A content change mid-measurement.
    let changed = format!("# A new first heading\n\n{md}");
    for _ in 0..4 {
        render(&ctx, &mut cache, &changed, 380.0, PROGRESSIVE);
    }
    assert!(frontier(&mut cache).is_some(), "still measuring the changed text");
    run_to_completion(&ctx, &mut cache, &changed, 380.0);
    render(&ctx, &mut cache, &changed, 380.0, PROGRESSIVE);
    let after = geometry(&mut cache, &changed);
    let indices: Vec<usize> = after.splits.iter().map(|s| s.0).collect();
    assert!(
        indices.windows(2).all(|w| w[0] < w[1]),
        "split points not strictly increasing after a content change: {indices:?}"
    );
    assert_same_geometry(
        &after,
        &one_shot_geometry(&changed, 380.0),
        "after a content change mid-measurement",
    );
}

/// A block taller than a screen right below the viewport leaves exactly one
/// split point past it, and without the cap at the frontier the slice would
/// then run to the end of the document.
#[test]
fn a_tall_block_at_the_frontier_does_not_paint_the_rest_of_the_document() {
    let mut md = String::new();
    for i in 1..=30 {
        md.push_str(&format!("Opening paragraph {i}, short.\n\n"));
    }
    md.push_str("```text\n");
    for i in 1..=80 {
        md.push_str(&format!("code line {i}\n"));
    }
    md.push_str("```\n\n");
    md.push_str(&document());

    // Where the tall block starts, from a one-shot measurement.
    let reference = one_shot_geometry(&md, 600.0);
    let tall = reference
        .splits
        .iter()
        .find(|(_, start, end)| end - start > VIEWPORT_HEIGHT)
        .expect("fixture has a block taller than the viewport");

    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let first = render(&ctx, &mut cache, &md, 600.0, PROGRESSIVE);
    assert!(frontier(&mut cache).is_some());
    // Viewport bottom just inside the tall block.
    let offset = (tall.1 - VIEWPORT_HEIGHT + 40.0).max(0.0);
    common::store_offset(&ctx, first.out.id, first.out.state, Vec2::new(0.0, offset));
    let frame = render(&ctx, &mut cache, &md, 600.0, PROGRESSIVE);

    let frontier = frontier(&mut cache).expect("the document is still being measured");
    let content_top = frame.out.inner_rect.top() - frame.out.state.unclamped_offset().y;
    let deepest = frame
        .texts
        .iter()
        .map(|r| r.bottom() - content_top)
        .fold(0.0_f32, f32::max);
    assert!(visible_count(&frame) > 0, "the frame painted nothing visible");
    assert!(
        deepest <= frontier.y + 1.0,
        "painted text reaches content y {deepest:.1}, past the frontier at {:.1}",
        frontier.y
    );
}
