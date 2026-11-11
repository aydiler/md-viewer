# Responsive tables and Full Width by default (#211)

**Status:** ✅ Complete
**Branch:** `feat/responsive-tables-and-defaults` (contributed by @nsrshishir; maintainer fixes on top)
**Date:** 2026-11-11

## Summary

Contributor PR #211 reverses the sticky-width table policy from #201:

- Tables stretch proportionally to fill the pane when their natural width is
  smaller (`fit_column_widths`).
- Columns re-fit whenever the pane width changes by more than 1 px: window
  resize, Full Width toggle, sidebar drag (`bound_changed` → `reset()`).
- Full Width (`full_width_content`) is now the default when nothing is
  persisted.
- New **Ctrl+Shift+W** shortcut toggles Full Width.

The maintainer compared it side by side with `main` and preferred #211.

## Fixes added before merge

1. **Table left short after the sidebar snaps back.** Dragging the right
   sidebar past its minimum and letting go narrows the pane in one frame.
   The columns re-fitted to the new width, but the separator-drag caps
   (`column_caps`, `.at_most(..)`) were computed from the previous, wider
   frame's widths and clipped them. The table stayed short until the next
   resize. Caps are now skipped on frames whose pane width changed. Both
   the Markdown and HTML table paths get the fix.
   - Regression test `table_refits_fully_after_the_sidebar_snaps_back`
     (`tests/pane_clip.rs`) drives `show_scrollable` through widen-then-snap
     and widen-then-gradual-return sequences. Observed red before the fix
     (466.0 vs a fresh 470.2) and green after (470.2 / 470.2 / 470.2).
2. **Ctrl+Shift+W also closed the tab.** The Ctrl+W handler did not exclude
   Shift, so the new shortcut fired both actions. It now requires
   `!modifiers.shift`.
3. The menu entry shows the shortcut via `.shortcut_text(..)`, like its
   neighbours, instead of baking it into the label.
4. Merged `main` into the branch (no force-push) so the PR picks up #204,
   #212, #213 and #214.

## Key Discoveries

- A reset that replaces one layout-derived value must also drop every other
  value derived from the old layout on that same frame. Here the widths were
  reset but the caps were not.
- The bug is invisible in small or gradual width changes (the stale cap
  differs by about the per-frame delta) and only shows on a large
  single-frame jump, which is exactly what a sidebar snap produces.

## Future Improvements

- Reflowing on every sidebar-drag frame discards a column the user resized
  by hand. If that turns out to matter, re-fit only on window resize and the
  Full Width toggle, or keep user-resized columns sticky.
