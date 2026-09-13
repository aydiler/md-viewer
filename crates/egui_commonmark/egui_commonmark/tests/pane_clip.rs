//! Regression tests for wide-block clip discipline at the visible pane edge.
//!
//! Two egui facts combine into the leak these tests pin down:
//!
//! 1. egui clips `SidePanel`s to their own rect, but `CentralPanel` content to
//!    `ctx.content_rect()` — the whole window — and `Ui::new_child` clones the
//!    parent painter, so every descendant of the document inherits that
//!    window-wide clip. The renderer's vertical `ScrollArea` only tightens the
//!    *scrolled* (Y) dimension of its content clip, so the X extent stays
//!    window-wide all the way down to tables and code blocks.
//! 2. `ScrollArea::begin` applies drag deltas to its offset *before* the
//!    content closure paints and only clamps the offset in `end` — a rightward
//!    drag drives `offset.x` negative and translates the wide block past the
//!    pane for every frame of the drag.
//!
//! So the renderer must bound both the carved-out layout widths (at
//! `max_rect`, which *is* the pane — `clip_rect` is window-wide and a no-op
//! bound) and the painter clip of every wide-block scope. All tests here
//! render through `show_scrollable` — the production path with the
//! renderer-owned ScrollArea; `.show()` never creates the pane-bounded
//! geometry and cannot reproduce any of this.

use egui::{Context, Rect};
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};

const PANE_WIDTH: f32 = 300.0;
/// egui wraps scroll content in a clip expanded by `visuals.clip_rect_margin`
/// (3px); the document ScrollArea and a nested block scroller each contribute
/// one, so up to ~6px past the viewport is legitimate clip allowance.
const CLIP_SLACK: f32 = 8.0;
/// Above this a painted rect is a window-spanning background (panel fill),
/// not content.
const BACKGROUND_WIDTH: f32 = 5000.0;

/// A plain table whose minimum column widths exceed the pane: at rest its
/// content is ~2.3x the viewport wide, so the horizontal scroller reports
/// `content_is_too_large` and drag input engages — the production scenario
/// for "drag the table over the sidebar".
const PLAIN_WIDE_TABLE: &str = r#"
|AAAAAAAAAAAAAAAAAAAAAAAAA|BBBBBBBBBBBBBBBBBBBBBBBBB|CCCCCCCCCCCCCCCCCCCCCCCCC|
|-------------------------|--------------------------|-------------------------|
|p1|p2|p3|
"#;

/// The same block carved out from an indented (blockquote) context, plus a
/// wide code block: exercises the viewport-slice re-anchor path where the
/// recorded geometry carries a left offset.
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
    text: Option<String>,
}

fn collect(shape: &egui::Shape, clip: Rect, painted: &mut Vec<Painted>) {
    match shape {
        egui::Shape::Rect(rect_shape) => painted.push(Painted {
            rect: rect_shape.rect,
            clip,
            text: None,
        }),
        egui::Shape::Text(text) => painted.push(Painted {
            rect: text.galley.rect.translate(text.pos.to_vec2()),
            clip,
            text: Some(text.galley.job.text.clone()),
        }),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect(shape, clip, painted);
            }
        }
        _ => {}
    }
}

/// The widest x any shape could visibly occupy: painting stops at the shape's
/// own right edge or its clip, whichever is smaller.
fn widest_visible_pixel(painted: &[Painted]) -> f32 {
    painted
        .iter()
        .filter(|p| p.rect.width() < BACKGROUND_WIDTH)
        .map(|p| p.rect.right().min(p.clip.right()))
        .fold(f32::NEG_INFINITY, f32::max)
}

/// One render of the fixture: begin with `events`, render, end, return the
/// shapes. The `begin_pass` must be the single one for the frame — a second
/// call with empty input would zero out pointer deltas.
fn render_doc_frame(
    ctx: &Context,
    cache: &mut CommonMarkCache,
    pane_right: &mut f32,
    events: Vec<egui::Event>,
    markdown: &str,
) -> Vec<egui::epaint::ClippedShape> {
    ctx.begin_pass(egui::RawInput {
        events,
        ..Default::default()
    });
    egui::CentralPanel::default().show(ctx, |ui| {
        // Production constrains the document viewport with panels plus
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
                .show_scrollable("pane_clip_test", ui, cache, markdown);
            *pane_right = out.inner_rect.right();
        });
    });
    ctx.end_pass().shapes
}

#[test]
fn indented_wide_blocks_do_not_paint_past_the_pane() {
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let mut pane_right = f32::NAN;
    let mut painted = Vec::new();

    // Both passes must obey the pane: pass 0 is the renderer's bootstrap
    // full paint, pass 1 the steady-state viewport slice that re-anchors the
    // recorded geometry at an offset content column.
    for pass in 0..2 {
        let shapes = render_doc_frame(&ctx, &mut cache, &mut pane_right, Vec::new(), INDENTED_WIDE_BLOCKS);
        if pass == 1 {
            for clipped in shapes {
                collect(&clipped.shape, clipped.clip_rect, &mut painted);
            }
        }
    }
    assert!(pane_right.is_finite(), "renderer never produced a viewport");
    let widest = widest_visible_pixel(&painted);
    assert!(
        widest <= pane_right + CLIP_SLACK,
        "content reaches x={widest:.1} but the pane ends at {pane_right:.1} \
         (slack {CLIP_SLACK}px): a wide block escaped the pane and would \
         paint over the right sidebar"
    );
}

#[test]
fn wide_block_content_actually_reaches_the_pane_edge() {
    // Guard against the leak test passing vacuously: the fixture's content
    // must come close to the pane edge so an overshoot would be visible.
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let mut pane_right = f32::NAN;
    let mut painted = Vec::new();
    for pass in 0..2 {
        let shapes = render_doc_frame(&ctx, &mut cache, &mut pane_right, Vec::new(), INDENTED_WIDE_BLOCKS);
        if pass == 1 {
            for clipped in shapes {
                collect(&clipped.shape, clipped.clip_rect, &mut painted);
            }
        }
    }
    let widest = widest_visible_pixel(&painted);
    assert!(
        widest >= pane_right - 60.0,
        "fixture regressed: content stops at x={widest:.1}, pane at \
         {pane_right:.1} — it no longer exercises the pane edge"
    );
}

/// Drag a table's last column separator rightward with a simulated pointer
/// and assert nothing paints past the pane **during the drag frames**.
///
/// This is the user action behind "I can drag tables over the sidebar":
/// `egui_extras` grows the dragged column without shrinking its neighbours,
/// so the table's total width overshoots the pane while the inherited clip
/// is window-wide (CentralPanel quirk) — every dragged frame paints the
/// columns over the right sidebar. The press targets the last column
/// separator (the table frame's right edge, minus frame chrome) at a cell
/// row's vertical center.
#[test]
fn dragging_a_table_column_rightward_never_paints_past_the_pane() {
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let mut pane_right = f32::NAN;
    let mut settle_painted = Vec::new();

    for pass in 0..2 {
        let shapes = render_doc_frame(&ctx, &mut cache, &mut pane_right, Vec::new(), PLAIN_WIDE_TABLE);
        if pass == 1 {
            for clipped in shapes {
                collect(&clipped.shape, clipped.clip_rect, &mut settle_painted);
            }
        }
    }
    assert!(pane_right.is_finite(), "renderer never produced a viewport");
    let cell = settle_painted
        .iter()
        .find(|p| p.text.as_deref() == Some("p2"))
        .expect("table cell text 'p2' not found in settled frame");
    let frame_right = settle_painted
        .iter()
        .filter(|p| p.text.is_none())
        .map(|p| p.rect.right())
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(frame_right > cell.rect.left(), "table frame not found");
    // The last separator sits at the inner right edge of the frame: the
    // outermost stroke plus the cell margin lie between it and the frame's
    // painted right edge.
    let press = egui::pos2(frame_right - 6.0, cell.clip.center().y);

    for step in 0..6u32 {
        let events: Vec<egui::Event> = if step == 0 {
            vec![egui::Event::PointerButton {
                pos: press,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }]
        } else {
            vec![egui::Event::PointerMoved(
                press + egui::vec2(60.0 * step as f32, 0.0),
            )]
        };
        let shapes = render_doc_frame(&ctx, &mut cache, &mut pane_right, events, PLAIN_WIDE_TABLE);
        if step == 0 {
            continue; // press frame: nothing has moved yet
        }
        let mut painted = Vec::new();
        for clipped in &shapes {
            collect(&clipped.shape, clipped.clip_rect, &mut painted);
        }
        let (down, delta) = ctx.input(|i| {
            (
                i.pointer.button_down(egui::PointerButton::Primary),
                i.pointer.delta(),
            )
        });
        assert!(down, "drag step {step}: simulated button is not down");
        assert!(
            delta.x > 0.0,
            "drag step {step}: pointer delta is {delta:?} — the simulated \
             drag is not moving, the test cannot exercise the leak"
        );
        let widest = widest_visible_pixel(&painted);
        let fr = painted.iter().filter(|p| p.text.is_none()).map(|p| p.rect.right()).fold(f32::NEG_INFINITY, f32::max);
        eprintln!("DBG step={step} widest={widest:.1} frame_right={fr:.1} pane={pane_right:.1}");
        assert!(
            widest <= pane_right + CLIP_SLACK,
            "drag step {step}: table paints to x={widest:.1} but the pane \
             ends at {pane_right:.1} — the drag leaked over the sidebar"
        );
    }
}
