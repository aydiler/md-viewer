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
fn long_inline_code_does_not_expand_past_content_width() {
    let markdown = "`10-19 Infrastructure Core/10-Architecture/10-K3s-Plex-Legacy/10-Ansible-K3s-Plex/10.25-Ansible-K3s-Plex-Runbooks.md`";

    let body_rect = markdown_body_rect_for(markdown, 540.0);

    assert!(
        body_rect.height() > 23.0,
        "long inline code stayed on one row instead of wrapping: rect={body_rect:?}"
    );
}
