# Feature: 057 — Styled Layouter for the Live Editor (B+)

**Status:** ✅ Complete
**Branch:** `feature/editing`
**Lines Changed:** ~+520 in `crates/egui_commonmark/{egui_commonmark_backend, egui_commonmark}/src`, `src/main.rs`

## Summary

The active block's TextEdit now paints through a markdown-aware layouter:
headings render at heading scale in the strong face; `**bold**`, `*em*`,
`` `code` `` and links are styled with their delimiters hidden as blank
glyphs; structural markers (`# `, `- `, `> `) vanish. The caret's line
reveals raw syntax (the Obsidian Live Preview signature). All of it inside a
stock `TextEdit` — no custom text engine.

## How it works

1. **Equal-char-count substitution.** The layouter builds a job whose text
   has exactly the buffer's char count; markup chars become spaces. egui maps
   cursors by char index onto the returned galley, so cursor, selection and
   IME stay exact (same mechanism as password masking).
2. **Inline parsing per frame.** pulldown-cmark runs over the small block
   buffer; events drive per-char decoration tags (strong/em/code/link),
   container leftovers (delimiters, link URLs) become blanks.
3. **Caret-line reveal.** After each paint the primary cursor char index is
   stashed under `<editor id>/caret_line_char`; next frame the layouter skips
   all blanking on that line.
4. **Block kind.** `EditRegionConfig.kind` (heading level / paragraph / quote
   / list / code) drives prefix rules and heading scale; the app derives it
   from the block's first line.

## Gotchas discovered

- **FontFamily must be real, not debug-formatted strings** — first version
  stored `format!("{family:?}")` which produced unregistered named families
  and panicked `FontFamily::"Proportional" is not bound`.
- **pulldown's Code event span includes its backticks** — blank first/last
  chars of the span, don't look outside it.
- **Clipboard comes from the galley (egui#5885)** — copying selected text in
  an edited block yields hidden markers as blanks. Documented tradeoff;
  never skip bytes to avoid worse breakage.

## Verification

- styler unit tests ×7 (unicode length preservation, reveal-line, ordered
  list numbers, link url hiding, section coverage)
- app tests 57 ✓ · fork lib 24+37 ✓ · e2e typing green through the new path
- clippy clean on touched code

## Next

PSE-2/C: persistent styled editors (no swap), galley-caret activation.
