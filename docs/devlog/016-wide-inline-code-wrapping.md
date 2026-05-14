# Feature: Wide Inline Code Wrapping

**Status:** ✅ Complete
**Branch:** `fix/wide-inline-wrap-overlap`
**Date:** 2026-05-14
**Lines Changed:** `src/main.rs`, `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`, `crates/egui_commonmark/egui_commonmark/tests/wrapping.rs`

## Summary

Fixed Markdown content overlap when notes contain long inline-code paths. The viewer now lets Markdown fill the available content pane width while splitting oversized inline-code chunks at path-friendly breakpoints so they wrap instead of expanding beyond the viewport.

## Features

- [x] Use the current content pane width for `CommonMarkViewer::default_width` instead of a fixed 600 px width.
- [x] Split long inline-code text into row-wrapped segments at `/`, `\\`, `-`, `_`, or whitespace after a safe chunk length.
- [x] Add a regression test for long inline-code path wrapping.
- [x] Verify manually on a wide System Notes window under Hyprland/Wayland.

## Key Discoveries

### Fixed content width caused wasted space

The application passed a hard-coded `default_width(Some(600))` to `CommonMarkViewer`, so rendered Markdown stayed confined even when the content pane had much more room. Switching to `ui.available_width()` lets notes use the full pane.

### Long inline-code widgets can expand egui rows

Long inline code, especially file paths in metadata, renders as one inline widget. If that widget remains too wide, the paragraph can exceed the viewport and overlap/cut off adjacent rendered chunks. Splitting only long inline-code chunks avoids changing normal paragraph text and keeps the fix targeted.

```rust
for segment in inline_code_wrap_segments(&text) {
    self.event_text(segment.into(), ui, options);
    if wraps_inline_code {
        ui.end_row();
    }
}
```

## Architecture

### Modified Rendering Flow

| Location | Change |
|----------|--------|
| `src/main.rs` | Pass current `ui.available_width()` into `CommonMarkViewer::default_width`. |
| `pulldown.rs` | Add `inline_code_wrap_segments()` helper and use it for `Event::Code`. |
| `wrapping.rs` | Add focused regression test for long inline-code path wrapping. |

### Why the fix stays narrow

Only inline code receives forced segmentation. Normal text, headings, links, code blocks, images, and table behavior continue through the existing renderer path.

## Testing Notes

Commands run:

```bash
cargo test -p egui_commonmark_extended --test wrapping
cargo check
git diff --check
```

Manual verification:

```bash
cargo run -- "/home/akiro/Coding/System Notes/10-19 Infrastructure Core/10-Architecture/10-K3s-Plex-Legacy/10-Ansible-K3s-Plex/10.24-Ansible-K3s-Plex-Process-Flow.md"
```

Observed behavior: long inline-code metadata no longer overlaps or cuts off, and normal body content fills the wider pane.

## Future Improvements

- [ ] Consider a richer inline text layout path if links and inline code need fully proportional wrapping without forced line breaks.
- [ ] Add a visual snapshot or UI harness test if the project adopts one for egui rendering regressions.
