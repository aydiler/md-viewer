//! Headless end-to-end verification of live-preview editing:
//! paint → record boundaries → simulated click → inline editor appears →
//! simulated keystroke → feedback reports changed text.
//!
//! Runs entirely on CPU via `Context::run` — no window system needed.

use std::cell::RefCell;

use egui::{
    scroll_area::ScrollSource, CentralPanel, Context, Event, PointerButton, Pos2, RawInput, Rect,
    Vec2,
};
use egui_commonmark_extended::{
    CommonMarkCache, CommonMarkViewer, EditFeedback, EditRegionConfig,
};

const MARKDOWN: &str =
    "# Heading One\n\nFirst paragraph to click.\n\n## Heading Two\n\nSecond paragraph here.\n";

#[derive(Clone, Copy)]
struct FrameGeom {
    inner_min_y: f32,
    scroll_offset_y: f32,
}

/// Paint one frame of the document through `show_scrollable`, exactly the way
/// `render_tab_content` configures it in Live mode. Returns viewport geometry
/// plus the inline-editor feedback stashed by this frame, if any.
fn run_frame(
    ctx: &Context,
    markdown: &str,
    cache: &RefCell<CommonMarkCache>,
    edit_region: Option<EditRegionConfig>,
    record_layout: bool,
    events: Vec<Event>,
) -> (FrameGeom, Option<EditFeedback>) {
    let geom = std::cell::Cell::new(FrameGeom {
        inner_min_y: 0.0,
        scroll_offset_y: 0.0,
    });
    let feedback = std::cell::Cell::new(None);

    ctx.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
            events,
            ..Default::default()
        },
        |ctx| {
            let mut cache = cache.borrow_mut();
            let mut viewer = CommonMarkViewer::new()
                .record_block_layout(record_layout)
                .scroll_source(ScrollSource {
                    scroll_bar: true,
                    drag: false,
                    mouse_wheel: true,
                });
            if let Some(cfg) = edit_region.clone() {
                viewer = viewer.edit_region(Some(cfg));
            }
            CentralPanel::default().show(ctx, |ui| {
                let out = viewer.show_scrollable("test-doc", ui, &mut cache, markdown);
                geom.set(FrameGeom {
                    inner_min_y: out.inner_rect.min.y,
                    scroll_offset_y: out.state.offset.y,
                });
            });
            feedback.set(cache.take_edit_feedback());
        },
    );
    (geom.get(), feedback.into_inner())
}

/// Screen-space position of the vertical middle of block `index`, found by
/// probing the hit-tester (same mapping the app performs for real clicks).
fn screen_pos_of_block(
    ctx: &Context,
    cache: &RefCell<CommonMarkCache>,
    id: egui::Id,
    index: usize,
    geom: FrameGeom,
) -> Pos2 {
    let target_start = cache
        .borrow_mut()
        .top_level_block_spans(&id)[index]
        .start;
    for step in 0..2000u32 {
        let content_y = step as f32 * 4.0;
        let hit = cache
            .borrow_mut()
            .block_span_at_content_y(&id, content_y)
            .map(|s| s.start == target_start)
            .unwrap_or(false);
        if hit {
            return Pos2::new(
                400.0,
                geom.inner_min_y + content_y - geom.scroll_offset_y,
            );
        }
    }
    panic!("no y resolves to block {index}");
}

#[test]
fn live_preview_click_activates_and_types() {
    let ctx = Context::default();
    let cache = RefCell::new(CommonMarkCache::default());
    let doc_id = egui::Id::new("test-doc");

    // Frame 1: Live-mode-style paint records block layout.
    let (geom, fb) = run_frame(&ctx, MARKDOWN, &cache, None, true, vec![]);
    assert!(fb.is_none(), "no editor without an active region");

    let spans = cache.borrow_mut().top_level_block_spans(&doc_id);
    assert_eq!(spans.len(), 4, "h1, para, h2, para: {spans:?}");
    assert!(
        cache.borrow_mut().has_block_layout(&doc_id),
        "boundaries recorded"
    );

    // Click the FIRST PARAGRAPH (block index 1) — same math as the app:
    // content_y = pointer.y - inner_rect.min.y + scroll_offset.
    let click_screen = screen_pos_of_block(&ctx, &cache, doc_id, 1, geom);
    let content_y = click_screen.y - geom.inner_min_y + geom.scroll_offset_y;
    let hit = cache
        .borrow_mut()
        .block_span_at_content_y(&doc_id, content_y)
        .expect("click maps to a block");
    assert_eq!(hit, spans[1], "click resolves to the clicked paragraph");
    drop(spans);

    // Activate exactly like the app: seed buffer + set edit_region.
    let cfg = EditRegionConfig {
        src: hit.clone(),
        id: doc_id.with("live_editor"),
    };
    ctx.data_mut(|d| d.insert_temp(cfg.id, MARKDOWN[hit.clone()].to_string()));

    // Frame 2: editor paints in place of the block.
    let (_, fb) = run_frame(&ctx, MARKDOWN, &cache, Some(cfg.clone()), true, vec![]);
    assert!(fb.is_some(), "inline editor painted for the active block");
    assert!(!fb.unwrap().changed, "no phantom change without input");

    // Frame 3: focus the editor the way a real click would leave it.
    // (Synthetic same-frame press/release pairs don't run egui's full click
    // pipeline, so grant focus explicitly — native input paths are what the
    // app exercises.)
    let _ = run_frame(&ctx, MARKDOWN, &cache, Some(cfg.clone()), true, vec![]);
    ctx.memory_mut(|mem| mem.request_focus(cfg.id));

    // Frame 4: keystroke lands in the focused inline editor.
    let (_, fb) = run_frame(
        &ctx,
        MARKDOWN,
        &cache,
        Some(cfg.clone()),
        true,
        vec![Event::Text("Z".into())],
    );
    let fb = fb.expect("editor still painted");
    assert!(fb.changed, "keystroke reported as change");

    // The working buffer diverged from the source slice — the app would now
    // splice `fb.text` back into its String.
    let edited = ctx.data_mut(|d| d.get_temp::<String>(cfg.id)).unwrap();
    assert!(edited.contains('Z'), "keystroke reached buffer: {edited:?}");
    assert_ne!(edited, MARKDOWN[hit], "buffer diverged from disk text");
}
