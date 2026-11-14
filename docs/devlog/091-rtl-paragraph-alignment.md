# Feature: Right-aligned RTL paragraphs and headings (#108)

**Status:** ✅ Complete for paragraphs, headings and blockquotes; lists and tables stay left-aligned
**Branch:** `feature/rtl-align`
**Date:** 2026-11-14

## Summary

After devlog 090, Arabic and Hebrew lines read in the right order but every
paragraph still started at the left edge of the reading column. Now a
paragraph or heading whose first strong character is right to left (UAX #9
P2/P3, the same rule as HTML's `dir="auto"`) is drawn as one galley with each
row flush with the column's right edge. English documents take exactly the
old code path.

## Features

- [x] RTL paragraphs and headings sit at the right edge, every wrapped row too.
- [x] Also inside blockquotes (the bar stays on the left).
- [x] Bold, italics, inline code and search highlights keep their look.
- [x] Links keep their look, follow on click, show the pointing hand and the
  destination on hover.
- [x] Selection and copy work: it is still a selectable `Label`.
- [ ] RTL list items, table cells, footnote and definition-list bodies: stay
  left-aligned (markers and cell layout sit on the left by construction).
- [ ] RTL paragraphs that contain an inline image, inline math, inline HTML,
  a footnote reference or a code token long enough to be cut into rows: stay
  on the old per-label path, so left-aligned.

## Key Discoveries

### Why one galley
The renderer emits one `Label` per text piece into a wrapping left-to-right
row. egui flows a label on from the previous one (first row indented, later
rows from the left edge) only for `Direction::LeftToRight`
(`egui-0.37 widgets/label.rs:192`); in a right-to-left layout each piece
becomes its own box. So an RTL paragraph is collected like a heading already
was, and drawn once at `TagEnd::Paragraph` with `halign = Align::RIGHT`,
inside a `top_down(Align::Max)` scope spanning the column.

### Direction is decided once per parse, by source offset
`rtl_block_starts` walks the cached event stream when it is parsed and stores
the source start offset of every RTL paragraph/heading in `CommonMarkCache`.
`event()` looks the offset up on `Start(Paragraph | Heading)`. Keying by
offset avoids threading event indices into `start_tag`, and the bootstrap and
slice passes see the same set because they share the event stream.

### `append_to` merges equal neighbouring sections
The first version styled link sections after appending them. In the app the
link came out plain: `RichText::append_to` extends the previous section when
the formats are equal, so the link's text joined the preceding section and
the restyling loop saw an empty slice. Links are now styled (`color`,
`underline`) on the `RichText` before appending, which also keeps them in
sections of their own.

### Link hit-testing needs the bidi-aware epaint
A link inside the galley is not a widget; a click is mapped to a character
with `Galley::cursor_from_pos` and checked against the link's character
range. With crates.io epaint 0.37 that mapping is wrong inside RTL runs
(glyphs are in visual order, the cursor index assumes logical order): a click
on the link's first glyph returned the index of the paragraph's last
character. With #8822's direction-aware `Row::char_at` in `crates/epaint` it
is right. So the two click tests live in the root workspace
(`tests/rtl_links.rs`), which builds with the patched epaint; the renderer's
own tests cover geometry, styling and detection, which do not depend on it.
`Glyph::section_index` would have avoided this, but it is `pub(crate)`.

### `rustfmt <file>` formats the modules it declares
Running `rustfmt` on the new test file also reformatted `tests/common/mod.rs`
through its `mod common;`. The renderer workspace is not rustfmt-clean and CI
does not check it, so unrelated reformatting was reverted.

## Architecture

- `egui_commonmark_backend/src/misc.rs`: `CommonMarkCache::{set_rtl_block_starts, is_rtl_block}`;
  `follow_link` (shared by `Link::end` and the RTL path).
- `egui_commonmark/src/parsers/pulldown.rs`:
  - `rtl_block_starts`, `block_is_rtl` (detection and eligibility);
  - `RtlBlock` and `render_rtl_block` (layout, link hit-testing);
  - fields `rtl_paragraph`, `current_heading_rtl`; one branch each in
    `emit_text`, `finish_link`, soft/hard breaks, the inline-code autolink,
    `TagEnd::Paragraph` and `TagEnd::Heading`.
- New dependency of `egui_commonmark_extended`: `unicode-bidi` 0.3.18
  (`hardcoded-data`), already in the app's lockfile via `crates/epaint`.

## Testing

- `tests/rtl_alignment.rs` (renderer, both passes): paragraph and heading at
  the right edge, every wrapped row at the right edge, LTR and English-first
  paragraphs unchanged, image paragraph keeps the old path, search highlight
  and link styling survive.
- Unit tests for `rtl_block_starts`: first strong character, inline code,
  headings/quotes vs lists/tables/footnotes, widgets.
- `tests/rtl_links.rs` (root): click on the link follows it, click on plain
  text does not.
- Mutations, each run on the target holding the tests: detection off (6 red),
  direction ignored (LTR test red), image allowed (image test red), hit test
  matching any link (plain-click red), hit test never matching (link-click red).
- Xvfb: Arabic heading, wrapped Arabic paragraph with bold and a wrapped link,
  Hebrew line, Arabic and English blockquotes, mixed line, list, English
  control. Partial drag-select of the Hebrew line copied exactly the
  highlighted `שלום עולם, זה טק`. Clicking the Arabic link opened `other.md`.

## Future Improvements

- RTL list items with the marker on the right.
- RTL table cells.
- Search scroll-to for a match inside an RTL paragraph lands on the paragraph
  top, not the match's row.
- "Copy Link Address" on right-click is not offered for links inside an RTL
  paragraph (the label's own context menu is the text one).
