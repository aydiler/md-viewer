# The interface's own symbols no longer depend on installed fonts

**Status:** ✅ Complete
**Branch:** `fix/ui-symbol-glyphs`
**Date:** 2026-11-20
**Issue:** #255

## Summary

The Refresh (↻), Expand all (⊞) and Collapse all (⊟) buttons in the explorer
and View → Dark Mode (☾) showed missing-glyph boxes on the reporter's Arch
system, both from AUR source and `-bin`.

The cause was the egui 0.37 upgrade (devlog 089): egui dropped
`emoji-icon-font` from its default fonts, and that font was what drew ⊞ ⊟ ↻.
Since then those glyphs, and ☾ ✓ ✕ which no bundled font ever had, came from
whatever system fonts happened to be installed. On a machine with DejaVu Sans
or Noto Sans Symbols everything looked fine, which is why nobody here saw it.

md-viewer now embeds a 4.7 KB subset of DejaVu Sans with the 29 symbols its own
interface uses and close relatives, appended last to both font families. An
installed font still draws any character it has; the subset only fills gaps.

## Measurements

`ui_symbols_render_without_system_fonts` builds the font definitions with no
system fonts and checks every symbol-block character (U+2190–U+2BFF and ×) in
the non-test part of `src/main.rs`. Before the fix it failed with 11 missing in
the proportional family:

```
← ↑ → ↓ ↻ ⊞ ⊟ ● ☾ ✓ ✕
```

The four the reporter saw, plus the find bar's arrows and close button (the
error bar's too), the View menu's check marks and the LIVE dot. ◀ ▶ ☀ ⚠ were
already covered by the bundled NotoEmoji face.

## Key Discoveries

- **Which faces are guaranteed is narrower than it looks.** egui 0.37's
  proportional family is only Ubuntu-Light (plus egui-icons); Hack covers ⊞ ⊟
  ↻ ← ↑ → ↓ but sits only in the monospace family, so buttons and menus never
  reach it.
- **No single Noto face covers the set.** Noto Sans Math lacks ✕ and ☀, Noto
  Sans Symbols 2 lacks the arrows and ⊞ ⊟; DejaVu Sans has all of them.
- **The test scans the source, so a new symbol in the UI is checked
  automatically.** It looks at symbol blocks only, so the CJK punctuation in
  the heading-slug code (char literals, never displayed) is not swept in.

## Architecture

- `assets/fonts/DejaVuSans-UiSymbols.ttf` (the exact `pyftsubset` command is in
  the doc comment of `UI_SYMBOL_FONT_BYTES`), license in
  `assets/fonts/DejaVu-LICENSE.txt` and `THIRD_PARTY_NOTICES`.
- `install_bundled_fallbacks` (`src/system_fonts.rs`) installs the emoji faces
  and then "UiSymbols" at the end of the proportional and monospace chains.
  `FontLoader::definitions` calls it for both the base and the full font set,
  and the test goes through the same function.

## Testing

- `ui_symbols_render_without_system_fonts`: red before the font (11 missing),
  green after.
- The asset is in `cargo package --list`, so the crates.io build embeds it.

## Future Improvements

- Painting the explorer icons as shapes would remove the font dependency
  altogether, at the cost of hand-drawn icons.
