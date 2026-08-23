# Feature: Phase 3 — Live Preview with Block-Level Editing

**Status:** ✅ Complete (v1)
**Branch:** `feature/editing`
**Date:** 2026-02
**Lines Changed:** ~+330 / -30 in `src/main.rs`, `crates/egui_commonmark/{egui_commonmark, egui_commonmark_backend}/src`

## Summary

The flagship editing experience from `docs/research/editing-approaches.md`
§3-B: in **Live mode**, the document renders exactly as before except the
block you click into, which becomes an inline raw-markdown editor. Click
another block to move there; Esc returns to the fully rendered view. Ctrl+E
toggles Rendered↔Live. Phase 2 (section-level click-to-edit) was absorbed —
sections are just coarser blocks of the same mechanism.

## Features

- [x] Block segmentation from the fork's cached event spans (no re-parsing)
- [x] Inline editor swap-in via additive, opt-in fork APIs
- [x] Click-to-edit hit-testing through recorded block-boundary positions
- [x] Per-frame splice-back of edited text; caret anchor stays stable
- [x] Esc deactivates; search jumps / outline clicks activate blocks
- [x] EditMode enum {Rendered, Source, Live} replaces the source_mode bool

## Key Discoveries

### 1. Never lend the source string twice

The natural design — fork paints a TextEdit bound to a slice of the caller's
`String` — is impossible: `show_scrollable` already borrows the content as
`&str`. Instead the editor's working buffer lives in **egui temp state**
under an id (`source_editor_id().with("live")`); the fork reads/writes it,
and the app splices feedback (`cache.take_edit_feedback()`) back into
`tab.content` when it changed this frame. Re-seeding is keyed by
`(content_version, block_start)` so typing is never clobbered by a re-seed.

### 2. Skip-painting events is only safe at top-level boundaries

The event loop maintains a state machine (`self.list`, `is_table`,
`is_blockquote`). Dropping events mid-container corrupts it (the split-point
panic documented in devlog 027). The edit region therefore MUST align to a
complete depth-0 block — which is precisely what `top_level_block_spans()`
produces, so the contract is enforced by construction on the app side.

### 3. Anchor-based active-block tracking survives re-segmentation

Typing changes byte offsets everywhere after the block. Storing an index into
a block list would go stale between frames; storing one **byte anchor inside
the active block** and re-resolving `spans.find(contains(anchor))` each frame
is self-healing. After a splice, `anchor_after_splice()` keeps the caret's
relative position clamped to the new length.

### 4. Hit-testing rides on existing boundary recording

Safe boundaries were already detected for split points; recording their
`end_position.y` plus the following byte offset (`BlockBoundary`) costs
nothing extra and turns clicks into byte ranges:
`content_y = pointer.y - inner_rect.min.y + scroll_offset`, then
`block_span_at_content_y()`.

## Limitations / v2 candidates

- Arrow keys do not yet cross block boundaries (click or Esc+click instead);
  needs caret-edge feedback out of the fork.
- The inline editor is plain monospace TextEdit (no syntax hiding of inline
  marks outside the caret, unlike CM6 decorations).
- Conflict watcher banner shows only for the active tab.
- One-frame latency between click and editor appearance (imperceptible).

## Verification

- `cargo test`: app 56 ✓ · backend 18 (+4 new segmentation tests) ✓
- `cargo clippy`: no warnings in app code
- GUI smoke test still not possible in this sandbox (Xvfb aborts) — manual
  desktop check recommended: click blocks in Live mode, type across a save,
  confirm watcher silence.
