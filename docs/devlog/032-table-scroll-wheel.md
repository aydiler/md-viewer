# Table Scroll Wheel Fix

**Date:** 2026-05-23
**Branch:** fix/table-scroll-wheel
**Status:** Implemented

## Scope

Fix issue #22 where wide tables shifted horizontally when vertical wheel scrolling past them.

## Root Cause

Wide markdown and HTML tables used a nested horizontal `ScrollArea` plus `forward_wheel_to_horizontal_scroll`. The helper read `smooth_scroll_delta.y`, applied it to the inner table `offset.x`, stored the state, and consumed vertical delta. That made ordinary document scrolling change table horizontal position whenever the cursor crossed a wide table.

## Change

Removed custom vertical-wheel forwarding. Wide tables still use `ScrollArea::horizontal()` for bottom scrollbar and native horizontal input, but vertical wheel remains document scroll.

## Testing Notes

- Pre-fix static guard found `forward_wheel_to_horizontal_scroll`, `smooth_scroll_delta.y`, `out.state.offset.x`, and helper call sites in `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`.
- Post-fix static guard clean: `! grep -n "forward_wheel_to_horizontal_scroll\|smooth_scroll_delta.y\|out.state.offset.x" crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`.
- `cargo fmt --check` exit 0.
- `cargo check` exit 0 with existing warnings.
- `cargo clippy --all-targets --all-features` exit 0 with existing warnings.
- Manual: `cargo run -- /tmp/md-viewer-wide-table.md --no-watch` launched.
- Manual: confirmed by user on real display with `docs/DEVELOPMENT_PLAN.md`. Reported "fixed."

## Impact

Verified behavior: normal mouse wheel over tables scrolls the document instead of horizontally nudging wide tables. Horizontal table access remains available via bottom scrollbar/native horizontal input.
