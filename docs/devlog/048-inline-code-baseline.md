# Fix: Inline code baseline/size alignment (#46)

**Status:** ✅ Complete
**Branch:** `fix/font-fallback`
**Date:** 2026-07-15
**Lines Changed:** +7 / -1 in `egui_commonmark_backend/src/misc.rs`

## Summary

Issue #46: inline `` `code` `` rendered **smaller than and raised above** the
surrounding body text — it did not sit on the shared baseline.

## Root cause

`Style::to_richtext_internal` styled inline code with egui's `RichText::code()`,
which sets the **Monospace text style**. That text style carries its own size —
`FontId::monospace(14.0)` in this app (`src/main.rs`) — while body text is
`FontId::proportional(16.0)`. So inline code came out at **14px** against **16px**
body text. Both fragments also carry the 1.5× accessibility line-height, and egui
positions the smaller glyph within that shared line box such that it sits raised,
not baseline-aligned.

## Fix

Pin inline code to the current context size after `.code()`:

```rust
if self.code {
    rich_text = rich_text.code().size(selected_font_size);
}
```

`selected_font_size` is the body size (16px) for normal text, and the heading
size when inline code appears inside a heading. `RichText`'s explicit `.size()`
overrides the Monospace text style's size while keeping the monospace **family**
and the code background, so code now matches the adjacent text size and baseline.

## Scope

- Only **inline** code (`Event::Code`) goes through this path. Fenced **code
  blocks** render via the separate syntect `LayoutJob` path (`elements.rs` /
  `misc.rs` code-block rendering) and are unaffected — verified pixel-identical
  (ImageMagick AE = 0) before/after on a code-block repro.

## Testing

- Xvfb before/after: all inline code spans (plain, mid-sentence, inside a
  bold/italic line) now align on the body baseline and match body size.
- Code-block region pixel-diff = 0 (no regression).

## Not fixed here: #45 (`sh` code block "different font")

Investigated together. #45 is **not a font bug** — in a ```` ```sh ```` block,
syntect colors the first word of each line as a shell *command* (dim) and the
rest as *arguments* (bright). The letterforms are identical monospace; the user
saw the color/brightness difference. A plain ```` ``` ```` fence (no language)
renders uniformly. Left as a separate discussion on the issue; changing it would
mean overriding syntect theme colors for all code blocks.
