# Emoji text-presentation selector (U+FE0E)

**Status:** Complete
**Branch:** `fix/emoji-text-presentation`
**Date:** 2026-11-13

## Summary

Devlog 074 made U+FE0F (VS16) pick the color-emoji face. Its counterpart,
U+FE0E (VS15, "text presentation"), was ignored: `🚀︎` still rendered in color.
Layout now peeks for U+FE0E as well and asks only the faces that are not
`emoji_only`, so a default emoji followed by FE0E comes from the bundled
monochrome Noto Emoji face (or any text face that has it).

## Change (vendored epaint)

- `text_layout.rs::layout_section`: the lookahead is a `match` on the next
  char: FE0F → `glyph_info_emoji_presentation`, FE0E →
  `glyph_info_text_presentation`, anything else → `glyph_info`.
- `CachedFamily::glyph_info_text_presentation` (`fonts.rs`) walks the chain
  skipping `emoji_only` faces, with its own `vs15_glyph_info_cache`. Same
  reason as VS16: the family cache is keyed by `char` alone, so sharing it
  would make every later bare 🚀 monochrome. Per-face caches are safe to reuse
  here, because a non-`emoji_only` face answers identically either way.
- `Font::glyph_info_text_presentation` (`font.rs`) falls back to normal lookup
  when no text face has the glyph.

## ZWJ sequences: measured, no change

`👨‍👩‍👧` renders as its component emoji. U+200D already has zero advance (a
face in the chain maps it to an empty glyph, as with FE0F) and the neighbours
stay color. Joining a sequence into one glyph needs a shaper; out of scope.

## Verification

`fe0e_requests_a_text_face_and_zwj_takes_no_space` (`src/system_fonts.rs`,
`#[ignore]`, needs system NotoColorEmoji), laying out bare 🚀, 🚀+FE0E, bare
🚀 again, and 👨‍👩:

| build | 🚀+FE0E | bare 🚀 after | ZWJ advance |
|---|---|---|---|
| before | color (red) | color | 0.0 |
| after | monochrome, 16.45 wide | color | 0.0 |

The VS16 test still passes. `cargo test` with and without default features,
`clippy -D warnings`. Xvfb screenshots before/after: only the text-style
rocket changes.
