# Feature: Shift+wheel horizontal table scroll

**Status:** ✅ Complete
**Branch:** `feature/table-shift-wheel`
**Date:** 2026-05-24

## Summary

PR #23 removed `forward_wheel_to_horizontal_scroll` to fix issue #22 (cursor-over-table nudging wide tables sideways during normal document scrolling). That removal also reverted the issue #4 ergonomic — wide-table horizontal access regressed to "grab the bottom scrollbar" / native horizontal input only.

This change reintroduces the wheel→x redirect but gates it on the Shift modifier, so plain wheel still scrolls the document (#22) while Shift+wheel is an opt-in for sideways table scrolling (#4).

## Change

New helper `forward_shift_wheel_to_horizontal_scroll` in `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`. Identical edge-passthrough logic to the original helper, with one extra early-return:

```rust
if !ui.ctx().input(|i| i.modifiers.shift) {
    return;
}
```

Wired into both the markdown-table and HTML-table call sites by capturing the `ScrollAreaOutput` from the nested `ScrollArea::horizontal()` and passing it to the helper after the table renders.

## Why Shift

- Ctrl is taken by zoom (`Ctrl+wheel`).
- Shift+wheel for horizontal scroll matches the convention in Firefox/Chrome.
- Users without prior knowledge still get correct default behavior; discovery cost is low.

## Testing Notes

- `cargo check` — clean (two pre-existing warnings unrelated to this change).
- `cargo clippy --all-targets` — TODO before push.
- Manual: cursor-over-table + plain wheel scrolls the document past the table without horizontal nudge.
- Manual: cursor-over-table + Shift+wheel scrolls the table sideways; edge pass-through hands the wheel back to the document at the table's left/right edge.

## Files

- `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`
- `docs/KEYBOARD_SHORTCUTS.md`
- `docs/TARGET_METRICS.md`
- `docs/ARCHITECTURE.md`
- `docs/LESSONS.md`
