//! Regression tests for inline-code wrapping. The renderer used to lay out a
//! long inline-code token as a single overflowing widget, which clipped or
//! overlapped surrounding text at narrow widths. See pulldown.rs
//! `inline_code_wrap_segments`.

use egui::{Context, Rect, TextStyle};
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};

fn render(markdown: &str, width: f32) -> (Rect, f32) {
    let ctx = Context::default();
    let mut body_rect = Rect::NOTHING;

    // Two passes: egui caches font/layout state on the first pass.
    for _ in 0..2 {
        ctx.begin_pass(Default::default());
        egui::CentralPanel::default().show(&ctx, |ui| {
            ui.set_width(width);
            let mut cache = CommonMarkCache::default();
            let response = CommonMarkViewer::new().show(ui, &mut cache, markdown);
            body_rect = response.response.rect;
        });
        let _ = ctx.end_pass();
    }

    let body_id = TextStyle::Body.resolve(&ctx.style());
    let row_height = ctx.fonts_mut(|f| f.row_height(&body_id));
    (body_rect, row_height)
}

#[test]
fn short_inline_code_stays_on_one_row() {
    let (rect, row_height) = render("prefix `short-code` suffix", 540.0);
    assert!(
        rect.height() <= row_height * 1.5,
        "short inline code wrapped unexpectedly: rect={rect:?} row_height={row_height}"
    );
}

#[test]
fn path_like_inline_code_wraps() {
    let md = "`10-19 Infrastructure Core/10-Architecture/10-K3s-Plex-Legacy/10-Ansible-K3s-Plex/10.25-Ansible-K3s-Plex-Runbooks.md`";
    let (rect, row_height) = render(md, 540.0);
    assert!(
        rect.height() > row_height * 1.5,
        "path-like inline code did not wrap: rect={rect:?} row_height={row_height}"
    );
}

#[test]
fn unbreakable_long_inline_code_wraps() {
    let md = format!("`{}`", "A".repeat(180));
    let (rect, row_height) = render(&md, 540.0);
    assert!(
        rect.height() > row_height * 1.5,
        "unbroken long inline code did not wrap: rect={rect:?} row_height={row_height}"
    );
}
