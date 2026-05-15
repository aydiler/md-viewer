# Feature: Wide Inline Code Wrapping

**Status:** ✅ Complete
**Branch:** `experiment/wrap-clean-fix`
**Date:** 2026-05-15
**Lines Changed:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`, `crates/egui_commonmark/egui_commonmark/tests/wrapping.rs`

## Summary

Long inline-code tokens (file paths, fully-qualified identifiers) used to render as a single oversized widget that overflowed the content column, clipping leading characters at narrow widths and overlapping adjacent text at wide widths. Split tokens longer than 56 characters into fixed-size chunks separated by row breaks so the row-wrap layout can place each chunk cleanly.

Reported in #5 by @aki1ro; their wrap approach is adopted here. The original PR also removed the 600 px typography cap from `src/main.rs`; that change is **not** applied — the cap is the project's intentional 55-75 CPL target (see `docs/LESSONS.md`).

## Features

- [x] Inline code with long paths wraps inside the 600 px content column.
- [x] Surrounding paragraph text continues normally after a wrapped inline-code token.
- [x] Unbreakable long runs (e.g. `AAAA…`) wrap by char-count.
- [x] Short inline code (`<=` 56 chars) stays on one row.
- [x] Tests covering the three regression cases above.

## Key Discoveries

### Blind char-count cut, not break-friendly chars

First attempt split at break-friendly characters (`/ \ - _ . :`) past 56 chars to keep paths readable. Visible diff that ended up in the PR's embedded `patches/0001-*.patch` artifact used the same approach. Both regress at narrow window widths: variable-length segments can exceed the column width and re-introduce egui's intra-widget wrap, which is what the original bug was caused by.

Fixed-size chunks always fit because they have a known upper bound. The PR's final commit also uses blind cut for the same reason.

### Cap removal is unrelated to the bug

Variant testing at 800 px (narrow) and 1500 px (wide) windows confirmed the inline-code segmentation fixes the bug fully whether or not `default_width(Some(600))` is in place. Removing the cap is a separate UX decision worth its own discussion.

## Architecture

### New helper

| Function | Purpose |
|----------|---------|
| `inline_code_wrap_segments()` | Splits a long inline-code token into ≤56-char chunks; returns `vec![text]` unchanged for short tokens. |

### Rendering flow

`Event::Code` now iterates over the helper's segments, calling `event_text` per segment and `ui.end_row()` between them (and after the final segment) so the row-wrap layout gets a hard break instead of trying to wrap a single oversized label.

## Testing Notes

Three integration tests in `crates/egui_commonmark/egui_commonmark/tests/wrapping.rs`:

- Short inline code stays on one row.
- Path-like inline code wraps across multiple rows.
- Unbreakable long runs wrap by char-count.

Row-height threshold is queried from the egui context rather than hard-coded, so changes to body line-height don't silently break the tests.

## Future Improvements

- [ ] Consider an opt-in toggle (View menu) to disable the 600 px typography cap for users who prefer full-width rendering on wide displays. Separate from this fix.
- [ ] Custom inline-code layout that handles wrapping natively without segment-and-end_row, if egui's row-wrap layout gains finer-grained control over individual widget overflow.
