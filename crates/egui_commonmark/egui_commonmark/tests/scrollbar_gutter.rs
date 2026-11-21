//! In Full Width the document column used to span the whole scroll area, and
//! egui draws a floating scrollbar *on top of* that area: code-block
//! backgrounds, table frames and the ends of long lines ran under the bar
//! (devlog 111). The renderer now keeps a gutter as wide as the bar's reach
//! on the right of the column it lays out in its own scroll area.

mod common;
#[allow(unused_imports)]
use common::{FrameExt as _, PanelExt as _};

use egui::{Context, Rect, Shape};
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};

const PANE_WIDTH: f32 = 520.0;
const PANE_HEIGHT: f32 = 400.0;

/// Long prose, a fenced code block, a markdown table and an HTML table,
/// repeated so the document is taller than the pane and the bar is shown.
fn fixture() -> String {
    let block = r#"
A paragraph long enough to wrap across the whole column several times, so its rows end as close to the right edge as the layout allows, which is where a floating scrollbar sits.

```rust
fn main() { println!("a code block whose background spans the column"); }
```

| Column one | Column two | Column three |
|------------|------------|--------------|
| alpha      | beta       | gamma        |

<table><tr><th>Html</th><th>Table</th></tr><tr><td>one</td><td>two</td></tr></table>
"#;
    block.repeat(4)
}

fn collect(shape: &Shape, clip: Rect, out: &mut Vec<(Rect, Rect)>, texts: &mut Vec<Rect>) {
    match shape {
        Shape::Rect(r) => out.push((r.rect, clip)),
        Shape::Text(t) => {
            let rect = t.galley.rect.translate(t.pos.to_vec2());
            out.push((rect, clip));
            texts.push(rect);
        }
        Shape::Vec(v) => v.iter().for_each(|s| collect(s, clip, out, texts)),
        _ => {}
    }
}

/// Render the fixture through `show_scrollable` (the renderer-owned scroll
/// area) a few frames, Full Width: no `default_width`, tables capped at the
/// pane like md-viewer does. Returns the painted rects and the scroll area's
/// inner rect.
fn render(floating: bool, default_width: Option<usize>) -> (Vec<(Rect, Rect)>, Vec<Rect>, Rect) {
    let ctx = Context::default();
    ctx.global_style_mut(|style| {
        style.spacing.scroll = if floating {
            egui::style::ScrollStyle::floating()
        } else {
            egui::style::ScrollStyle::solid()
        };
    });
    let mut cache = CommonMarkCache::default();
    let markdown = fixture();
    let mut painted = Vec::new();
    let mut texts = Vec::new();
    let mut inner = Rect::NOTHING;
    for _ in 0..3 {
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            ..Default::default()
        });
        egui::CentralPanel::default().show_ctx(&ctx, |ui| {
            let pane = Rect::from_min_size(ui.max_rect().min, egui::vec2(PANE_WIDTH, PANE_HEIGHT));
            ui.scope_builder(egui::UiBuilder::new().max_rect(pane), |ui| {
                let out = CommonMarkViewer::new()
                    .default_width(default_width)
                    .table_max_width(Some(PANE_WIDTH as usize))
                    .show_scrollable("gutter_test", ui, &mut cache, &markdown);
                inner = out.inner_rect;
            });
        });
        let output = ctx.end_frame();
        painted.clear();
        texts.clear();
        for clipped in &output.shapes {
            collect(&clipped.shape, clipped.clip_rect, &mut painted, &mut texts);
        }
    }
    (painted, texts, inner)
}

/// The rightmost visible pixel of anything that starts left of `line`: the
/// bar's own shapes start inside its strip and are excluded.
fn content_right(painted: &[(Rect, Rect)], inner: Rect, line: f32) -> f32 {
    painted
        .iter()
        .filter(|(rect, _)| {
            // Content of the pane only: the central panel's own background
            // starts left of it and spans the window.
            rect.left() >= inner.left() - 1.0
                && rect.left() < line - 1.0
                && rect.top() < inner.bottom()
                && rect.bottom() > inner.top()
        })
        .map(|(rect, clip)| rect.right().min(clip.right()))
        .fold(f32::NEG_INFINITY, f32::max)
}

fn bar_reach(scroll: &egui::style::ScrollStyle) -> f32 {
    scroll.bar_inner_margin + scroll.bar_width + scroll.bar_outer_margin
}

#[test]
fn full_width_content_stays_clear_of_the_floating_scrollbar() {
    let (painted, _, inner) = render(true, None);
    let reach = bar_reach(&egui::style::ScrollStyle::floating());
    let line = inner.right() - reach;
    let right = content_right(&painted, inner, line);
    assert!(right.is_finite(), "nothing painted: {painted:?}");
    assert!(
        right <= line + 0.5,
        "content reaches {right}, under the floating bar that starts at {line} (pane {inner:?})"
    );
    // ... and still uses the column: the widest block ends near the gutter.
    assert!(
        right > line - 40.0,
        "content ends at {right}, far left of the gutter line {line}: the column shrank too much"
    );
}

#[test]
fn a_solid_scrollbar_gets_no_extra_gutter() {
    // egui already shrinks the content for a non-floating bar; the renderer
    // must not reserve the bar's width a second time.
    let (painted, _, inner) = render(false, None);
    let right = content_right(&painted, inner, inner.right());
    assert!(
        // Code-block backgrounds fill the column exactly, so the column's
        // right edge is the content's.
        right > inner.right() - 1.0,
        "content ends at {right}, well short of the content edge {}: a second gutter",
        inner.right()
    );
}

#[test]
fn the_reading_column_is_unchanged() {
    // With a reading width narrower than the pane, the gutter never binds:
    // the column stays centred where it was, 400 px wide.
    let (_, texts, inner) = render(true, Some(400));
    let left = texts
        .iter()
        .filter(|rect| rect.left() >= inner.left() - 1.0 && rect.top() < inner.bottom())
        .map(|r| r.left())
        .fold(f32::INFINITY, f32::min);
    let expected_left = inner.left() + (inner.width() - 400.0) / 2.0;
    assert!(
        (left - expected_left).abs() < 1.5,
        "reading column starts at {left}, expected {expected_left} (pane {inner:?})"
    );
}
