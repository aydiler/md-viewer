//! The code-block copy button (#256): its hover frame hugs the icon, and it
//! stays clear of a floating scrollbar at the block's right edge.

mod common;
#[allow(unused_imports)]
use common::{FrameExt as _, PanelExt as _};

use egui::{Context, Event, Pos2, Rect, Shape};
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer, COPY_ICON};

fn frame(ctx: &Context, cache: &mut CommonMarkCache, events: Vec<Event>) -> Vec<Shape> {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 400.0))),
        events,
        ..Default::default()
    };
    let out = ctx.run_frame(input, |ctx| {
        egui::CentralPanel::default().show_ctx(ctx, |ui| {
            CommonMarkViewer::new().show(ui, cache, "Text.\n\n```rust\nfn main() {}\n```\n");
        });
    });
    let mut shapes = Vec::new();
    fn flatten(shape: &Shape, out: &mut Vec<Shape>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| flatten(s, out)),
            s => out.push(s.clone()),
        }
    }
    for clipped in &out.shapes {
        flatten(&clipped.shape, &mut shapes);
    }
    shapes
}

#[test]
fn copy_button_frame_hugs_the_icon_and_clears_a_floating_scrollbar() {
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    frame(&ctx, &mut cache, vec![]);
    let shapes = frame(&ctx, &mut cache, vec![]);

    let (icon, rows, row_height) = shapes
        .iter()
        .find_map(|s| match s {
            Shape::Text(t) if t.galley.job.text == COPY_ICON => Some((
                t.galley.rect.translate(t.pos.to_vec2()),
                t.galley.rows.len(),
                t.galley.rows[0].height(),
            )),
            _ => None,
        })
        .expect("copy icon painted");
    // Measured against zero available width, the two-character label wrapped
    // onto two rows, which is what made the frame twice as tall.
    assert_eq!(rows, 1, "copy icon label wrapped onto {rows} rows");
    // The block's background is the smallest filled rect around the icon (the
    // panel's background also contains it, and the button has no fill). Its
    // colour is not a fixed visual: with `better_syntax_highlighting` it comes
    // from the syntect theme.
    let block = shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Rect(r)
                if r.fill.a() > 0
                    && r.rect.contains(icon.center())
                    && r.rect.width() > 4.0 * icon.width() =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("code block background painted");

    // Hover the icon so the button paints its frame.
    let hovered = frame(&ctx, &mut cache, vec![Event::PointerMoved(icon.center())]);
    let hover_frame = hovered
        .iter()
        .filter_map(|s| match s {
            Shape::Rect(r)
                if r.stroke.width > 0.0
                    && r.rect.contains(icon.center())
                    && r.rect.width() < block.width() / 2.0 =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("hover frame painted around the icon");

    let style = ctx.global_style();
    let padding = style.spacing.button_padding.y;
    assert!(
        hover_frame.height() <= row_height + 2.0 * padding + 2.0,
        "hover frame {hover_frame:?} is taller than one row of the icon ({row_height})"
    );
    let scroll = style.spacing.scroll;
    if scroll.floating {
        assert!(
            hover_frame.right() <= block.right() - scroll.bar_width,
            "button {hover_frame:?} reaches under a floating scrollbar at the block edge {}",
            block.right()
        );
    }
}
