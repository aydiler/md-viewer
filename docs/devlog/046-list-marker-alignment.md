# Fix: List marker vertical alignment

**Status:** ✅ Complete
**Branch:** `fix/list-marker-alignment`
**Date:** 2026-07-15
**Lines Changed:** ~+35 / -12 in the vendored renderer (`elements.rs`, `lib.rs`, `generator.rs`)

## Summary

List markers (the `•` bullet, the hollow `◦` nested bullet, and the `N.` number)
sat visibly **above** the optical centre of their list-item text — the marker
looked high, the text looked low. This affected every bullet and ordered list.

## Root Cause

The item text carries the 1.5× accessibility line-height (WCAG 2.1 SC 1.4.12,
applied via `TextFormat.line_height`), so its galley row is 1.5× the raw font
height. The list row is laid out with `Layout::left_to_right(Align::BOTTOM)`, so
it is as tall as that 1.5× text and everything bottom-aligns to the row.

`bullet_point` / `bullet_point_hollow` / `number_point` (in
`egui_commonmark_backend/src/elements.rs`) sized their marker box to only the
**raw** body-font height (`height_body`) and drew the marker at that small box's
centre. Bottom-aligned, a raw-height box centres its marker well above where the
text — sitting near the bottom of its taller line box — actually renders.

## Fix

Pass the resolved body line-height into the three marker functions and:

1. Size the marker box to that `row_height` (`row_height.max(raw)`), so it
   bottom-aligns identically to the text.
2. Place the marker at the text's optical centre — `rect.bottom() - raw/2.0`
   via a shared `marker_center` helper — instead of the box centre.
3. Keep the dot radius / number font tied to the **raw** glyph height, so marker
   size is unchanged; only its vertical position moves.

`List::start_item` (in `egui_commonmark/src/lib.rs`) computes the line-height
from `options.typography.resolve_line_height(body_h)` (falling back to the raw
height when typography is off) and passes it through.

The compile-time macro path (`egui_commonmark_macros/src/generator.rs`) has no
typography config, so it passes `ui.text_style_height(&egui::TextStyle::Body)`
(the raw height) — marker layout there is unchanged from before.

## Architecture

### Modified Functions

| Function | Change |
|----------|--------|
| `bullet_point(ui, row_height)` | new `row_height` param; box sized to it; marker at `marker_center` |
| `bullet_point_hollow(ui, row_height)` | same |
| `number_point(ui, number, row_height)` | same; number drawn at `marker_center().y`, right-aligned |
| `marker_center(rect, raw)` | new helper: optical centre = `rect.bottom() - raw/2.0` |
| `List::start_item` | resolves line-height from typography and threads it in |

## Testing Notes

- Verified visually on Xvfb with filled bullets, hollow nested bullets, ordered
  numbers, and nested ordered numbers — all markers now centre on the text.
- Confirmed font-independent: correct with both egui's bundled Ubuntu-Light body
  font and a Noto Sans body.
- `cargo test --manifest-path crates/egui_commonmark/Cargo.toml -p egui_commonmark_extended --test wrapping` — 12 passed.

## Future Improvements

- `marker_center`'s `rect.bottom() - raw/2.0` assumes egui lays the boosted
  line-height galley with the glyph near the bottom of the line box. If a future
  egui changes that leading distribution, revisit this offset.
