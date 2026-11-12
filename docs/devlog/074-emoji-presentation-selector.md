# Emoji presentation selector (VS16) picks the color face

**Status:** ✅ Complete
**Branch:** `fix/emoji-presentation-selector`
**Date:** 2026-11-12

## Summary

`❤️` (and the `:heart:` shortcode, which expands to it) rendered as a white
text heart. `❤️` is U+2764 followed by U+FE0F (VARIATION SELECTOR-16, "emoji
presentation"). U+2764 is not a default emoji, so the color face, restricted to
default-emoji characters since #204 (`emoji_only`), refused it, and a text face
drew it. epaint lays text out one `char` at a time with no lookahead, so the
selector never influenced the choice of face.

## Fix (vendored epaint only)

- `text_layout.rs::layout_section`: peek at the next char. When it is U+FE0F,
  look the current char up with `Font::glyph_info_emoji_presentation`.
- `CachedFamily::glyph_info_emoji_presentation` (`fonts.rs`): asks only the
  `emoji_only` faces, so the color face wins over text faces that come earlier
  in the fallback chain and also contain ❤. If no emoji face has the glyph, the
  normal lookup runs.
- `FontImpl::glyph_info_emoji_presentation` (`font.rs`): lets an `emoji_only`
  face claim any character it has a glyph for.

Both levels use their **own** caches. The normal per-face and per-family caches
are keyed by `char` alone and are checked before the `emoji_only` gate, so
writing a VS16 result into them would turn every later bare `❤` color as well.
The test lays out bare, then VS16, then bare again to catch exactly that.

U+FE0F itself already had zero advance, because a face in the chain maps it to
an empty glyph. The test pins that, so no change was needed.

The crates.io build (`color-emoji` off) has no `emoji_only` face, so nothing
changes there.

## Verification

- `vs16_requests_the_color_face_for_text_default_characters`
  (`src/system_fonts.rs`, `#[ignore]`, needs system NotoColorEmoji): red before
  the fix (`vs16=[('❤', false, …)]`), green after (`('❤', true, 19.9)`, bare
  heart still `false` both before and after).
- Xvfb screenshot, `main` vs branch: `❤️` and `:heart:` turn red; bare `❤`,
  🚀 and ✔ ▶ ⚠ are unchanged.
- `cargo test` / `clippy -D warnings` with and without default features.

## Future Improvements

- U+FE0E (text presentation) could force a default emoji back to the text face.
- ZWJ sequences (👨‍👩‍👧) still render as separate glyphs; that needs shaping,
  not a lookahead.
- The vendored epaint's own tests cannot run in this repo (`cargo test -p
  epaint` refuses: not a workspace member, needs dev-dependencies), so the
  coverage lives in md-viewer's tests.
