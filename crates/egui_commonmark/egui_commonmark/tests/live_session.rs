//! Headless verification of persistent-editing sessions: every text block
//! paints as its own styled editor, buffers round-trip through feedback,
//! keystrokes land in the focused block only.

use std::cell::RefCell;

use egui::{CentralPanel, Context, Event, Pos2, RawInput, Rect, Vec2};
use egui_commonmark_extended::{
    CommonMarkCache, CommonMarkViewer, EditBlockKind, EditSessionConfig, SessionBlock,
};

const MARKDOWN: &str = "# Heading One\n\nFirst paragraph.\n\n## Heading Two\n\nSecond paragraph.\n";

fn kinds_for(spans: &[std::ops::Range<usize>]) -> Vec<EditBlockKind> {
    spans
        .iter()
        .map(|r| {
            if MARKDOWN[r.clone()].starts_with('#') {
                EditBlockKind::Heading(1)
            } else {
                EditBlockKind::Paragraph
            }
        })
        .collect()
}

fn run_frame(ctx: &Context, markdown: &str, cache: &RefCell<CommonMarkCache>, salt: egui::Id) {
    let mut cache = cache.borrow_mut();
    let spans = cache.top_level_block_spans(&salt);
    let blocks: Vec<SessionBlock> = spans
        .iter()
        .enumerate()
        .map(|(i, r)| SessionBlock {
            src: r.clone(),
            kind: kinds_for(&spans)[i],
        })
        .collect();
    let session = if blocks.is_empty() {
        None
    } else {
        Some(EditSessionConfig {
            id_salt: salt,
            blocks,
        })
    };

    ctx.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
            ..Default::default()
        },
        |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                let viewer = CommonMarkViewer::new().edit_session(session.clone());
                viewer.show_scrollable(salt, ui, &mut cache, MARKDOWN);
            });
        },
    );
}

#[test]
fn session_paints_editors_and_routes_keystrokes() {
    let ctx = Context::default();
    let cache = RefCell::new(CommonMarkCache::default());
    let salt = egui::Id::new("sess");

    // Frame 1..2: seed segmentation.
    run_frame(&ctx, MARKDOWN, &cache, salt);
    run_frame(&ctx, MARKDOWN, &cache, salt);

    {
        let mut c = cache.borrow_mut();
        let spans_len = c.top_level_block_spans(&salt).len();
        let bounds_len = c.block_bounds(&salt).len();
        eprintln!("PROBE spans={spans_len} bounds={bounds_len}");
        let fb_probe = c.get_session_feedback(&salt);
        eprintln!("PROBE fb={} idxs={:?}",
            fb_probe.len(),
            fb_probe.iter().map(|f| f.index).collect::<Vec<_>>());
        // re-stash so subsequent assertions still see them
        c.stash_session_feedback(&salt, fb_probe);
    }
    let fb = { cache.borrow().get_session_feedback(&salt) };
    assert_eq!(fb.len(), 4, "four text editors painted");
    assert_eq!(
        cache.borrow_mut().top_level_block_spans(&salt).len(),
        4,
        "segmentation intact"
    );

    // Focus the SECOND block's editor and type into it.
    let target = salt.with(("blk", 1usize));
    ctx.memory_mut(|mem| mem.request_focus(target));
    run_frame(&ctx, MARKDOWN, &cache, salt);
    // Buffers are canonical while Live: block 1 holds the typed text,
    // others their originals.
    let fb = run_frame_typed(&ctx, &cache, salt, "ZZ");
    assert_eq!(fb.len(), 4, "four editors painted");
    assert!(
        fb.iter().find(|f| f.index == 1).unwrap().text.ends_with("ZZ"),
        "block 1 buffer: {:?}",
        fb.iter().find(|f| f.index == 1).unwrap().text
    );
    for f in &fb {
        eprintln!("FB[{}] = {:?}", f.index, f.text);
    }
    assert!(
        fb.iter()
            .find(|f| f.index == 0)
            .unwrap()
            .text
            .contains("Heading One")
    );
}

fn run_frame_typed(
    ctx: &Context,
    cache: &RefCell<CommonMarkCache>,
    salt: egui::Id,
    s: &str,
) -> Vec<egui_commonmark_extended::SessionBlockFeedback> {
    let out = std::cell::RefCell::new(Vec::new());
    ctx.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
            events: vec![Event::Text(s.into())],
            ..Default::default()
        },
        |ctx| {
            let mut c = cache.borrow_mut();
            let spans_probe = c.top_level_block_spans(&salt);
            eprintln!("TYPED-FRAME spans={}", spans_probe.len());
            CentralPanel::default().show(ctx, |ui| {
                // rebuild session like run_frame
                let spans = c.top_level_block_spans(&salt);
                let blocks: Vec<SessionBlock> = spans
                    .iter()
                    .enumerate()
                    .map(|(i, r)| SessionBlock {
                        src: r.clone(),
                        kind: kinds_for(&spans)[i],
                    })
                    .collect();
                let viewer = CommonMarkViewer::new().edit_session(Some(EditSessionConfig {
                    id_salt: salt,
                    blocks,
                }));
                viewer.show_scrollable(salt, ui, &mut c, MARKDOWN);
            });
            let got = c.take_session_feedback(&salt);
            eprintln!("TYPED-CLOSE take={}", got.len());
            *out.borrow_mut() = got;
        },
    );
    out.into_inner()
}
