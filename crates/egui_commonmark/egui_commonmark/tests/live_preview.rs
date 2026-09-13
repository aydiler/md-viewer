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
    CommonMarkCache, CommonMarkViewer, EditFeedback, EditRegionConfig, EditSessionConfig,
    SessionBlockFeedback,
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
    let source_id = egui::Id::new("test-doc");

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
                let out = viewer.show_scrollable(source_id, ui, &mut cache, markdown);
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

/// Paint one Live-mode frame through `show_scrollable` with the given
/// persistent-editing session. Returns viewport geometry plus the per-block
/// feedback stashed by this frame.
fn run_session_frame(
    ctx: &Context,
    markdown: &str,
    cache: &RefCell<CommonMarkCache>,
    session: Option<EditSessionConfig>,
    events: Vec<Event>,
) -> (FrameGeom, Option<Vec<SessionBlockFeedback>>) {
    let geom = std::cell::Cell::new(FrameGeom {
        inner_min_y: 0.0,
        scroll_offset_y: 0.0,
    });
    let feedback = std::cell::Cell::new(None);
    let source_id = egui::Id::new("test-doc");

    ctx.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
            events,
            ..Default::default()
        },
        |ctx| {
            let mut cache = cache.borrow_mut();
            let mut viewer = CommonMarkViewer::new()
                .record_block_layout(true)
                .scroll_source(ScrollSource {
                    scroll_bar: true,
                    drag: false,
                    mouse_wheel: true,
                });
            if let Some(cfg) = &session {
                viewer = viewer.edit_session(Some(cfg.clone()));
            }
            CentralPanel::default().show(ctx, |ui| {
                let out = viewer.show_scrollable(source_id, ui, &mut cache, markdown);
                geom.set(FrameGeom {
                    inner_min_y: out.inner_rect.min.y,
                    scroll_offset_y: out.state.offset.y,
                });
            });
            let salt = session.as_ref().map(|s| s.id_salt);
            let fb = salt.map(|salt| cache.take_session_feedback(&salt));
            feedback.set(fb);
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

/// Session-based port of the original click→activate→type story: a click
/// resolves to the clicked paragraph via the recorded boundaries, the
/// per-block session editor receives a keystroke, and the feedback reports
/// exactly that block as changed. (The legacy `edit_region` single-editor
/// option never had a feedback producer and is superseded by sessions.)
#[test]
fn live_session_click_types_into_block() {
    use egui_commonmark_extended::{EditBlockKind, SessionBlock};

    let ctx = Context::default();
    let cache = RefCell::new(CommonMarkCache::default());
    let doc_id = egui::Id::new("test-doc");
    let salt = doc_id.with("pse");

    // Frame 1: Rendered-style paint records block layout.
    let (geom, _) = run_frame(&ctx, MARKDOWN, &cache, None, true, vec![]);
    let spans = cache.borrow_mut().top_level_block_spans(&doc_id);
    assert_eq!(spans.len(), 4, "h1, para, h2, para: {spans:?}");

    // Build the session the way the app seeds it: one block per span, kind
    // sniffed from the first line.
    let blocks: Vec<SessionBlock> = spans
        .iter()
        .enumerate()
        .map(|(_, span)| SessionBlock {
            src: span.clone(),
            kind: if MARKDOWN[span.clone()].starts_with("# ") {
                EditBlockKind::Heading(1)
            } else if MARKDOWN[span.clone()].starts_with("## ") {
                EditBlockKind::Heading(2)
            } else {
                EditBlockKind::Paragraph
            },
        })
        .collect();
    assert_eq!(blocks[0].kind, EditBlockKind::Heading(1));
    assert_eq!(blocks[1].kind, EditBlockKind::Paragraph);
    let session = Some(EditSessionConfig {
        id_salt: salt,
        blocks,
    });

    // Frame 2: Live paint. All four text blocks seed; nothing changed yet.
    let (_, fb) = run_session_frame(&ctx, MARKDOWN, &cache, session.clone(), vec![]);
    let fb = fb.expect("session feedback stashed on the first Live frame");
    assert_eq!(fb.len(), 4, "h1, para, h2, para paint editors: {fb:?}");
    assert!(fb.iter().all(|f| !f.changed), "seed frame is not a change");

    // Click the FIRST PARAGRAPH (block index 1) — same math as the app:
    // content_y = pointer.y - inner_rect.min.y + scroll_offset_y.
    let click_screen = screen_pos_of_block(&ctx, &cache, doc_id, 1, geom);
    let content_y = click_screen.y - geom.inner_min_y + geom.scroll_offset_y;
    let hit = cache
        .borrow_mut()
        .block_span_at_content_y(&doc_id, content_y)
        .expect("click maps to a block");
    assert_eq!(hit, spans[1], "click resolves to the clicked paragraph");
    drop(spans);

    // Frame 3: focus the clicked block's editor the way a real click would.
    let _ = run_session_frame(&ctx, MARKDOWN, &cache, session.clone(), vec![]);
    ctx.memory_mut(|mem| mem.request_focus(salt.with(("blk", 1))));

    // Frame 4: keystroke lands in the focused block editor only.
    let (_, fb) = run_session_frame(
        &ctx,
        MARKDOWN,
        &cache,
        session.clone(),
        vec![Event::Text("Z".into())],
    );
    let fb = fb.expect("editors still painted");
    eprintln!("DEBUG fb={fb:?}");
    let typed = fb
        .iter()
        .find(|f| f.index == 1)
        .expect("clicked block reported");
    assert!(typed.changed, "keystroke reported as change");
    assert!(typed.text.contains('Z'), "keystroke reached buffer: {typed:?}");
    assert!(
        fb.iter().filter(|f| f.index != 1).all(|f| !f.changed),
        "other blocks untouched: {fb:?}"
    );

    // The working buffer diverged from the source slice — the app would now
    // splice `typed.text` back into its String.
    let edited = ctx
        .data_mut(|d| d.get_temp::<String>(salt.with(("blk", 1))))
        .unwrap();
    assert_ne!(edited, MARKDOWN[hit.clone()], "buffer diverged from disk");
}

/// Regression: the app passes an arbitrary pre-built `egui::Id` as the
/// source id. `show_scrollable` wraps it via `Id::new(source_id)` for cache
/// keying — the public accessors must apply the identical wrapping or they
/// read a different, always-empty entry (clicks then never resolve).
#[test]
fn accessors_match_show_scrollable_keying_for_arbitrary_ids() {
    let ctx = Context::default();
    // Deliberately NOT derived from a &str the way the other test does:
    // this is exactly how src/main.rs builds Tab::id from a PathBuf.
    let tab_id = egui::Id::new(std::path::PathBuf::from("/some/absolute/note.md"));
    let cache = RefCell::new(CommonMarkCache::default());

    run_frame_with_source_id(&ctx, MARKDOWN, &cache, tab_id);

    assert!(
        cache.borrow_mut().has_block_layout(&tab_id),
        "accessors must see boundaries recorded under show_scrollable's wrapped key"
    );
    let spans = cache.borrow_mut().top_level_block_spans(&tab_id);
    assert_eq!(spans.len(), 4);
}

fn run_frame_with_source_id(
    ctx: &Context,
    markdown: &str,
    cache: &RefCell<CommonMarkCache>,
    source_id: egui::Id,
) {
    run_frame_with_source_id_record(ctx, markdown, cache, source_id, true)
}

fn run_frame_with_source_id_record(
    ctx: &Context,
    markdown: &str,
    cache: &RefCell<CommonMarkCache>,
    source_id: egui::Id,
    record: bool,
) {
    ctx.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
            ..Default::default()
        },
        |ctx| {
            let mut cache = cache.borrow_mut();
            CentralPanel::default().show(ctx, |ui| {
                CommonMarkViewer::new()
                    .record_block_layout(record)
                    .show_scrollable(source_id, ui, &mut cache, markdown);
            });
        },
    );
}

/// Regression for the mode-transition bug: Rendered-mode frames
/// (`record=false`) fill split_points; when the app switches to Live
/// (`record=true`) every split point already exists, which used to skip
/// boundary recording entirely — clicks then never resolved. Boundaries must
/// dedupe independently and backfill on the first recorded frame.
#[test]
fn boundaries_backfill_after_switching_to_live() {
    let ctx = Context::default();
    let tab_id = egui::Id::new(std::path::PathBuf::from("/mode/switch/note.md"));
    let cache = RefCell::new(CommonMarkCache::default());

    // Rendered phase: no boundary recording requested yet.
    for _ in 0..2 {
        run_frame_with_source_id_record(&ctx, MARKDOWN, &cache, tab_id, false);
    }
    assert!(
        !cache.borrow_mut().has_block_layout(&tab_id),
        "no boundaries while recording is off"
    );

    // Switch to Live: first recorded frame must backfill ALL boundaries.
    run_frame_with_source_id_record(&ctx, MARKDOWN, &cache, tab_id, true);

    assert!(
        cache.borrow_mut().has_block_layout(&tab_id),
        "boundaries must appear on the first Live frame"
    );
    let spans = cache.borrow_mut().top_level_block_spans(&tab_id);
    assert_eq!(spans.len(), 4);

    // And the click path resolves against them immediately.
    let mid = cache
        .borrow_mut()
        .block_span_at_content_y(&tab_id, 10.0)
        .expect("hit-test works right after switch");
    assert_eq!(mid.start, spans[0].start, "topmost click hits first block");
}
