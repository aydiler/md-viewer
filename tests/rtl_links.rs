//! Links inside a right-to-left paragraph (#108) follow on click, and the
//! paragraph's plain text does not.
//!
//! The renderer draws such a paragraph as one galley and finds the clicked
//! character with `Galley::cursor_from_pos`. That mapping is only right for
//! right-to-left text with the bidi-aware epaint in `crates/epaint`, which is
//! why this test lives in the root workspace (the renderer's own tests build
//! against epaint from crates.io).

use egui::{vec2, CentralPanel, Context, Event, PointerButton, Pos2, RawInput, Rect, Shape};
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};
use std::sync::Arc;

const DOC: &str = "English paragraph first.\n\nשלום [קישור](other.md) עולם\n";

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 800.0))),
        events,
        ..Default::default()
    }
}

/// One frame; returns every painted text galley with its position.
fn frame(
    ctx: &Context,
    cache: &mut CommonMarkCache,
    events: Vec<Event>,
) -> Vec<(Pos2, Arc<egui::Galley>)> {
    let mut out = ctx.run_pass(input(events), |ctx| {
        ctx.root_ui(|root| {
            CentralPanel::default().show(root, |ui| {
                CommonMarkViewer::new()
                    .default_width(Some(600))
                    .show_scrollable("rtl-links", ui, cache, DOC);
            });
        });
    });
    out.textures_delta.clear();
    let mut texts = Vec::new();
    fn collect(shape: &Shape, texts: &mut Vec<(Pos2, Arc<egui::Galley>)>) {
        match shape {
            Shape::Text(t) => texts.push((t.pos, t.galley.clone())),
            Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, texts)),
            _ => {}
        }
    }
    for clipped in out.shapes {
        collect(&clipped.shape, &mut texts);
    }
    texts
}

/// Click the centre of the first glyph `chr` of the RTL paragraph and report
/// whether the link's hook fired.
fn click_glyph(chr: char) -> Option<bool> {
    let ctx = Context::default();
    let mut cache = CommonMarkCache::default();
    cache.add_link_hook("other.md");
    let painted = frame(&ctx, &mut cache, vec![]);
    let (pos, galley) = painted
        .iter()
        .find(|(_, g)| g.job.text.contains("קישור"))
        .expect("RTL paragraph painted");
    assert!(
        galley.job.text.contains("שלום") && galley.job.text.contains("עולם"),
        "paragraph was not drawn as one galley: {:?}",
        galley.job.text
    );
    let (row, glyph) = galley
        .rows
        .iter()
        .find_map(|row| row.glyphs.iter().find(|g| g.chr == chr).map(|g| (row, g)))
        .expect("glyph");
    let at =
        *pos + row.pos.to_vec2() + vec2(glyph.pos.x + glyph.advance_width / 2.0, row.size.y / 2.0);
    let button = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(
        &ctx,
        &mut cache,
        vec![Event::PointerMoved(at), button(true)],
    );
    frame(&ctx, &mut cache, vec![button(false)]);
    cache.get_link_hook("other.md")
}

#[test]
fn clicking_a_link_inside_an_rtl_paragraph_follows_it() {
    // 'ק' occurs only in the link text.
    assert_eq!(click_glyph('ק'), Some(true));
}

#[test]
fn clicking_plain_rtl_text_does_not_follow_the_link() {
    // 'ל' occurs only outside the link (in שלום and עולם).
    assert_eq!(click_glyph('ל'), Some(false));
}
