# Fix: keep Full Width content clear of the floating scrollbar

**Status:** ✅ Complete
**Branch:** `fix/full-width-scrollbar-gutter`
**Date:** 2026-11-21
**Lines Changed:** renderer `parsers/pulldown.rs` (`scrollbar_gutter`, `floating_scrollbar_reach`, `show_scrollable`, `show_measuring`, `measure_from_frontier`, both table sites); `tests/scrollbar_gutter.rs`

## Summary

In Full Width mode, code-block backgrounds, table frames and the ends of long
lines reached the pane's right edge, under the floating scrollbar (noted in
devlog 106's Future Improvements). egui paints a floating bar on top of the
scroll area's content and gives it no layout space. With no `default_width`,
the renderer's column was the whole scroll area.

The renderer now keeps a gutter as wide as the bar's reach on the right of
the column, but only in its own scroll area (`show_scrollable`) and only for
a floating bar.

## Architecture

- `scrollbar_gutter: f32` on `CommonMarkViewerInternal`:
  - default 0;
  - `show_scrollable` sets it from `floating_scrollbar_reach(&ui.spacing().scroll)`,
    which is `bar_inner_margin + bar_width + bar_outer_margin` for a floating
    bar and 0 for a solid one, since egui already lays content out beside a
    solid bar;
  - the progressive measurer copies it.
- `show_measuring` caps the column at `max_rect().right() - gutter`. That width
  becomes `ContentGeometry`, which viewport slices and progressive measurement
  reuse, so every pass agrees (devlogs 055, 103).
- The column margin is the old centred margin, clamped so the column never
  reaches into the gutter. Full Width keeps its left edge, and a reading column
  with room on both sides stays where it was.
- Tables: `table_bound` is capped at the column (`max_rect().right() -
  cursor().left()`) when a gutter is reserved. The caller's bound is the whole
  pane, so without the cap the frame was laid out under the bar and clipped.
  Plain `show()` callers are unaffected.
- The copy button keeps #264's own inset. It now sits where it did on main,
  relative to its block, a few points further in than strictly needed.

## Key Discoveries

All three were found by measuring, after the first version passed its own test:

1. **Centering moved the narrowed column right.** The column is centred in the
   viewport, so subtracting 14 px from its width added 7 px on each side. The
   test reported content at 521 instead of 528, still past the line at 514.
2. **A second gutter on tables.** Inside the column ui, `max_rect().right()` is
   already the narrowed column (a probe printed `max_rect ... 514.0` with
   `gutter=14`). Subtracting the gutter again left tables ending at 500 instead
   of 514. The screenshot showed it as a table narrower than the code block
   above it.
3. **An unconditional table cap changed plain `show()`.**
   `html_table_uses_height_aware_column_widths` (`wrapping.rs`) went red: in a
   panel narrower than `table_max_width`, tables had always been allowed to
   reach past it. The cap now applies only with a gutter.

The full suite caught number 3 only because `cargo test` stops at the first
failing target. The re-run used `--no-fail-fast` so the remaining targets ran
too.

## Tests (`tests/scrollbar_gutter.rs`, through `show_scrollable`)

- `full_width_content_stays_clear_of_the_floating_scrollbar`:
  - no painted content rect starts left of the gutter line and ends past it;
  - the code block and the markdown table both reach the line;
  - the widest block ends within 40 px of it.
- `a_solid_scrollbar_gets_no_extra_gutter`: with a solid bar, content ends at
  the scroll area's content edge.
- `the_reading_column_is_unchanged`: a 400 px reading column starts at the
  same x as on main.

Verification runs:

| run | result |
|---|---|
| main's renderer (control) | first test red, "content reaches 528, under the floating bar that starts at 514"; the other two green |
| mutant: gutter for solid bars too | `a_solid_scrollbar_gets_no_extra_gutter` red (508 vs 518). An earlier 30 px tolerance did not catch this, so it was tightened to 1 px |
| mutant: double gutter on tables | red, "only blocks of heights [46] reach the gutter line" |

Suites:

- renderer with CI features and with defaults, every target (`--no-fail-fast`);
- root tests: 98 passed;
- clippy: no new warnings; fmt: clean.

## E2E (Xvfb, release builds of main and the branch)

- Fixture: Full Width, explorer and outline hidden, pointer over the document
  so the bar is shown.
- On main, the code block and the table run under the bar's strip. On the
  branch, both end before it, with the same table-to-block offset as on main.
- `scroll-regression.sh` PASS (bottom at frame 63 of 150);
  `visual-regression.sh` PASS.

## Future Improvements

- The copy button's own `bar_reach` inset (#264) is now redundant inside
  `show_scrollable`. Dropping it there would move the button a few points
  closer to the block edge. Done in devlog 112.
