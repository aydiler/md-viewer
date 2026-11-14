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

## Bidirectional text (#108)

The epaint part of emilk/egui#8822 (head `6dbd0934`, by ahmedhosnypro;
cumulative, it contains #8818), applied unchanged with
`git apply --include='crates/epaint/src/*'`, plus its `unicode-bidi` 0.3.18
dependency in `Cargo.toml`:

- runs are split by bidi level as well as font face, each shaped in its own
  direction, and rows are reordered visually (UAX #9 L1/L2);
- `LayoutJob::direction` (`TextDirection::Auto` by default) sets the
  paragraph base direction, and `halign_min_is_start` aligns RTL rows to
  the right of the widest row;
- `Row::x_ranges`, `char_at` and `x_offset` are direction-aware, which is
  what makes stock egui 0.37's selection and copy correct on mixed lines.

Not applied: the PR's egui-side changes (`label.rs`, text selection
painting, caret movement). egui is not vendored. Stock egui 0.37 compiles
against the patched epaint, and selection plus copy across a mixed line
measured correct on Xvfb.

Drop this section's changes when an egui release ships bidi: replace
`crates/epaint/src` with the released epaint, keep only the presentation
patch above, and run `mixed_direction_line_is_in_visual_order`
(`src/system_fonts.rs`).
