# Fix: Code block copy button painted as a missing glyph

**Status:** ✅ Complete
**Branch:** `fix/code-copy-icon`
**Date:** 2026-11-15

## Summary

Since the egui 0.37 upgrade (v0.3.6) the copy button in the top-right corner of
every code block showed an empty box. Found while retaking the README
screenshots. Its label was U+1F5D0 (🗐), which came from `emoji-icon-font.ttf`.
egui 0.37 no longer puts that font in `FontDefinitions::default()`; it only
ships it behind the `monochrome_emoji_fonts` feature and keeps a five-glyph
`egui-icons.ttf` subset by default. No font md-viewer installs has U+1F5D0.

The label is now U+1F4CB U+FE0E (📋 in text presentation), the glyph egui's own
color picker uses for copy. The bundled Noto Emoji face covers both code
points, so the icon renders with no system fonts. U+FE0E keeps it monochrome
through the vendored epaint's presentation-aware fallback, like the old icon.

## Features

- [x] `COPY_ICON` / `COPIED_ICON` constants in `egui_commonmark_backend::elements`,
  re-exported from `egui_commonmark_extended`.
- [x] Regression test `code_block_copy_icons_render_without_system_fonts`
  (`src/system_fonts.rs`): every char of both labels has a glyph in egui's
  default fonts plus the bundled emoji face, with any host colour emoji face
  removed.

## Key Discoveries

### egui 0.37 dropped `emoji-icon-font` from the default fonts

`epaint_default_fonts-0.37.0/fonts/egui-icons.txt`: the default set now keeps
only the Android, Windows, GitHub, git and Apple logos. Any other symbol that
used to come from `emoji-icon-font` silently becomes a missing-glyph box. The
other symbols md-viewer paints (◀ ▶ ⊞ ⊟ ↻ ✔ …) are covered by other faces; a
glyph check over all of them is a possible follow-up.

### Verification

The test was run green with the fix, then red with the label set back to
U+1F5D0 (see the PR for the output).
