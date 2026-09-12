//! Regression tests for wide-block clip discipline at the visible pane edge.
//!
//! egui clips CentralPanel content to the whole window (only side panels clip
//! themselves to their own rect), and `Ui::new_child` inherits the parent
//! clip — so the renderer's wide-block carve-outs (tables, code blocks) used
//! to paint past the content pane and straight over the right sidebar. Every
//! carved-out scope is now capped at the scroll viewport's right edge.
//!
//! These tests render through `show_scrollable` — the production path with
//! the renderer-owned ScrollArea — inside a blockquote, whose indentation is
//! what pushed the carve-out past the pane in the first place. They assert
//! that nothing paints, or is clipped to anything, meaningfully right of the
//! pane. `min(shape.rect.right, shape.clip_rect.right)` is the widest pixel
//! the shape could visibly occupy; egui adds `visuals.clip_rect_margin`
//! (3px) around scroll content clips, so a few px of slack is expected.

use egui::{Context, Rect};
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};

const PANE_WIDTH: f32 = 300.0;
/// egui wraps every scroll content in a clip expanded by
/// `visuals.clip_rect_margin` (3px); the document ScrollArea and a nested
/// block scroller each contribute one, so up to ~6px past the viewport is
/// legitimate clip allowance. Round up to 8.
const CLIP_SLACK: f32 = 8.0;

const INDENTED_WIDE_BLOCKS: &str = r#"
> |AAAAAAAAAAAAAAAAAAAAAAAAA|BBBBBBBBBBBBBBBBBBBBBBBBB|CCCCCCCCCCCCCCCCCCCCCCCCC|
> |-------------------------|--------------------------|-------------------------|
> |alpha                    |beta                      |gamma                    |

```
let long_code_line = "an_unbreakable_token_that_is_far_wider_than_the_pane";
```
"#;

#[derive(Debug)]
struct Painted {
    rect: Rect,
    clip: Rect,
}

fn collect(shape: &egui::Shape, clip: Rect, painted: &mut Vec<Painted>) {
    match shape {
        egui::Shape::Rect(rect_shape) => painted.push(Painted {
            rect: rect_shape.rect,
            clip,
        }),
        egui::Shape::Text(text) => painted.push(Painted {
            rect: text.galley.rect.translate(text.pos.to_vec2()),
            clip,
        }),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect(shape, clip, painted);
            }
        }
        _ => {}
    }
}

fn widest_visible_pixel(painted: &[Painted], surface_width: f32) -> f32 {
    painted
        .iter()
        // Window-spanning backgrounds (panel fill etc.) are not content.
        .filter(|p| p.rect.width() < surface_width * 0.5)
        .map(|p| p.rect.right().min(p.clip.right()))
        .fold(f32::NEG_INFINITY, f32::max)
}

fn render_indented_wide_blocks() -> (Vec<Painted>, f32, f32) {
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let mut painted = Vec::new();
    let mut pane_right = f32::NEG_INFINITY;
    let surface_width = ctx.input(|i| i.content_rect().width());

    // Both passes must obey the pane: pass 0 is the renderer's bootstrap
    // full paint, pass 1 the steady-state viewport slice that re-anchors the
    // recorded geometry at an offset content column — the path that used to
    // leak over the sidebar.
    for _pass in 0..2 {
        ctx.begin_pass(Default::default());
        egui::CentralPanel::default().show(&ctx, |ui| {
            // Production constrains the document viewport with a panel plus
            // margins; emulate it with an explicit scope (the default test
            // surface is huge, so the panel itself is not a pane).
            let pane = Rect::from_min_size(
                ui.max_rect().min,
                egui::vec2(PANE_WIDTH, ui.max_rect().height()),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(pane), |ui| {
                let out = CommonMarkViewer::new()
                    .default_width(Some(PANE_WIDTH as usize))
                    .table_max_width(Some(PANE_WIDTH as usize))
                    .show_scrollable("pane_clip_test", ui, &mut cache, INDENTED_WIDE_BLOCKS);
                pane_right = pane_right.max(out.inner_rect.right());
            });
        });
        let output = ctx.end_pass();
        for clipped in output.shapes {
            collect(&clipped.shape, clipped.clip_rect, &mut painted);
        }
    }
    (painted, pane_right, surface_width)
}

#[test]
fn indented_wide_blocks_do_not_paint_past_the_pane() {
    let (painted, pane_right, surface_width) = render_indented_wide_blocks();
    assert!(
        pane_right.is_finite(),
        "renderer never produced a scroll viewport"
    );
    let widest = widest_visible_pixel(&painted, surface_width);
    assert!(
        widest <= pane_right + CLIP_SLACK,
        "content reaches x={widest:.1} but the pane ends at {pane_right:.1} \
         (slack {CLIP_SLACK}px): a wide block escaped the pane and would \
         paint over the right sidebar"
    );
}

#[test]
fn wide_block_content_actually_reaches_the_pane_edge() {
    // Guard against the leak test passing vacuously: the fixture's columns
    // must fill the pane budget so a carve-out overshoot would be visible.
    let (painted, pane_right, surface_width) = render_indented_wide_blocks();
    let widest = widest_visible_pixel(&painted, surface_width);
    assert!(
        widest >= pane_right - 60.0,
        "fixture regressed: content stops at x={widest:.1}, pane at \
         {pane_right:.1} — it no longer exercises the pane edge"
    );
}
