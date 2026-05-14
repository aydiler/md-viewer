use egui::{Context, Rect};
use egui_commonmark_extended::{CommonMarkCache, CommonMarkViewer};

fn markdown_body_rect_for(markdown: &str, width: f32) -> Rect {
    let ctx = Context::default();
    let mut body_rect = Rect::NOTHING;

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

    body_rect
}

#[test]
fn short_inline_code_remains_single_row() {
    let rect = markdown_body_rect_for("prefix `short-code` suffix", 540.0);

    assert!(
        rect.height() <= 23.0,
        "short inline code wrapped unexpectedly: {rect:?}"
    );
}

#[test]
fn path_like_inline_code_wraps_into_multiple_rows() {
    let markdown = "`10-19 Infrastructure Core/10-Architecture/10-K3s-Plex-Legacy/10-Ansible-K3s-Plex/10.25-Ansible-K3s-Plex-Runbooks.md`";
    let rect = markdown_body_rect_for(markdown, 540.0);

    assert!(
        rect.height() > 23.0,
        "path-like inline code did not wrap: {rect:?}"
    );
}

#[test]
fn unbroken_long_inline_code_wraps_into_multiple_rows() {
    let markdown = format!("`{}`", "A".repeat(180));
    let rect = markdown_body_rect_for(&markdown, 540.0);

    assert!(
        rect.height() > 23.0,
        "unbroken long inline code did not wrap: {rect:?}"
    );
}
