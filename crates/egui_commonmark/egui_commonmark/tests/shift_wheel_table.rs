//! Shift+wheel over a wide table scrolls it horizontally; a plain wheel does
//! not (#22, #24).
//!
//! egui turns Shift+wheel into a horizontal delta itself
//! (`InputOptions::horizontal_scroll_modifier`, default Shift), which the
//! table's own `ScrollArea::horizontal` consumes. The renderer's former
//! `forward_shift_wheel_to_horizontal_scroll` helper read the *vertical* delta
//! while Shift was held, which egui had already zeroed, so it never acted; it
//! was removed in the egui 0.37 port. This test pins the behaviour it was
//! meant to provide.

mod common;

use common::{FrameExt as _, PanelExt as _};
use egui::{
    Context, Event, Modifiers, MouseWheelSource, MouseWheelUnit, Pos2, RawInput, Rect, Shape,
    TouchPhase, vec2,
};
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};

const PANE: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(400.0, 600.0));

fn table() -> String {
    let headers: Vec<String> = (1..=12).map(|i| format!("HEADER_COLUMN_{i:02}")).collect();
    format!(
        "| {} |\n|{}\n| {} |\n",
        headers.join(" | "),
        "---|".repeat(12),
        (1..=12)
            .map(|i| format!("CELL_{i:02}"))
            .collect::<Vec<_>>()
            .join(" | ")
    )
}

/// Paint one frame with `events` and return the screen x of every text shape
/// whose text contains `marker`, plus that shape's rect.
fn frame(
    ctx: &Context,
    cache: &mut CommonMarkCache,
    md: &str,
    events: Vec<Event>,
    marker: &str,
) -> Option<Rect> {
    let out = ctx.run_frame(
        RawInput {
            screen_rect: Some(PANE),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show_ctx(ctx, |ui| {
                CommonMarkViewer::new()
                    .default_width(Some(380))
                    .table_max_width(Some(380))
                    .show_scrollable("shift-wheel-doc", ui, cache, md);
            });
        },
    );
    let mut found = None;
    let mut stack: Vec<&Shape> = out.shapes.iter().map(|c| &c.shape).collect();
    while let Some(shape) = stack.pop() {
        match shape {
            Shape::Text(text) if text.galley.job.text.contains(marker) => {
                found = Some(text.galley.rect.translate(text.pos.to_vec2()));
            }
            Shape::Vec(shapes) => stack.extend(shapes.iter()),
            _ => {}
        }
    }
    found
}

fn scrolled_cell_x(shift: bool) -> (f32, f32) {
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    let md = table();
    let mut start = None;
    for _ in 0..4 {
        start = frame(&ctx, &mut cache, &md, vec![], "CELL_01");
    }
    let start = start.expect("table cell painted");
    let pointer = Pos2::new(PANE.center().x, start.center().y);
    let modifiers = if shift { Modifiers::SHIFT } else { Modifiers::NONE };
    let wheel = Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, -120.0),
        phase: TouchPhase::Move,
        source: MouseWheelSource::Unknown,
        modifiers,
    };
    frame(
        &ctx,
        &mut cache,
        &md,
        // egui 0.37 keeps modifier state from `ModifiersChanged` events.
        vec![Event::ModifiersChanged(modifiers), Event::PointerMoved(pointer)],
        "CELL_01",
    );
    frame(&ctx, &mut cache, &md, vec![wheel], "CELL_01");
    // Wheel scrolling is smoothed over several frames.
    let mut end = start;
    for _ in 0..30 {
        if let Some(rect) = frame(&ctx, &mut cache, &md, vec![], "CELL_01") {
            end = rect;
        }
    }
    (start.min.x, end.min.x)
}

#[test]
fn shift_wheel_scrolls_a_wide_table_horizontally() {
    let (start, end) = scrolled_cell_x(true);
    assert!(
        end < start - 20.0,
        "Shift+wheel did not scroll the table left: x {start:.1} -> {end:.1}"
    );
}

#[test]
fn plain_wheel_leaves_a_wide_table_alone() {
    // Control: the same fixture and wheel event with Shift do scroll, so a
    // pass below cannot come from a table too narrow to scroll at all.
    let (shift_start, shift_end) = scrolled_cell_x(true);
    assert!(shift_end < shift_start - 20.0, "control: fixture does not scroll");

    let (start, end) = scrolled_cell_x(false);
    assert!(
        (end - start).abs() < 0.5,
        "plain wheel moved the table sideways (#22): x {start:.1} -> {end:.1}"
    );
}
