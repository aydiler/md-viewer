# Feature: Full Width Toggle

**Status:** ✅ Complete
**Branch:** `feature/full-width-toggle`
**Date:** 2026-05-15
**Lines Changed:** `src/main.rs`

## Summary

Add an opt-in global View menu toggle that lets markdown content use the available pane width instead of the default 600 px optimal reading-width cap.

This follows up on PR #5, where inline-code wrapping was accepted as the bug fix while cap removal was deferred as a separate UX option.

## Features

- [x] Add a persisted `full_width_content` app setting.
- [x] Add `View -> Full Width` toggle.
- [x] Keep default behavior at the current 600 px content cap.
- [x] Use full available content pane width when enabled.

## Key Discoveries

### Full width is separate from inline-code wrapping

`docs/devlog/018-wide-inline-code-wrapping.md` records that inline-code wrapping works independently from content-width cap changes. This feature should only add a user-controlled width mode.

## Architecture

### New/Modified Structs

```rust
struct MarkdownApp {
    full_width_content: bool,
    // existing fields...
}

struct PersistedState {
    full_width_content: Option<bool>,
    // existing fields...
}
```

### Rendering Flow

The render path maps `full_width_content` to `CommonMarkViewer::default_width(None)` for full-width mode or `Some(CONTENT_OPTIMAL_WIDTH as usize)` for capped mode.

## Testing Notes

Validation:

- `cargo test --manifest-path /home/akiro/Coding/md-viewer-full-width-toggle/Cargo.toml content_width -- --nocapture` — passed.
- `cargo test --manifest-path /home/akiro/Coding/md-viewer-full-width-toggle/Cargo.toml` — passed with pre-existing warnings.
- `cargo build --manifest-path /home/akiro/Coding/md-viewer-full-width-toggle/Cargo.toml` — passed with pre-existing warnings.
- `cargo clippy --manifest-path /home/akiro/Coding/md-viewer-full-width-toggle/Cargo.toml` — passed with pre-existing warnings: unused `max_width` in `crates/egui_commonmark/egui_commonmark_backend/src/elements.rs:125`, deprecated `Ui::allocate_ui_at_rect` in `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs:884`, and unused patch `egui_commonmark_macros_extended`.

## Future Improvements

- [ ] Consider adding keyboard shortcut only if users ask for faster toggling.
- [ ] Consider width presets only if full/capped proves too coarse.
