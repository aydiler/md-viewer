# Vendored epaint 0.37.0 — md-viewer patch

Stock epaint 0.37.0 (commit "Vendor stock epaint 0.37.0 (unmodified)") plus
one change, marked `md-viewer patch` in the source:

- `src/text/family.rs`: `Family::resolve_cluster` honors emoji / text
  presentation (UTS #51) through the existing `has_emoji_presentation`.
  Emoji-presentation clusters (default emoji, U+FE0F, skin tones) prefer a
  face with color glyphs; all others, including U+FE0E, prefer a face without.
  Each falls back to any face that has the char. Separate caches per
  presentation.
- `src/text/font_face.rs`: `FontFace::has_color_glyphs()` accessor.

Stock 0.37 picks a face by the cluster's first char only, so `❤️` rendered
like `❤`, and a color face early in the chain would claim `✔ ▶ ⚠`.

`cargo publish` ignores `[patch.crates-io]`: the crates.io build of md-viewer
uses stock epaint 0.37 and still gets color emoji (`color_fonts`), without the
presentation handling.
