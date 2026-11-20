# Mermaid diagrams follow the dark theme

**Status:** ✅ Complete
**Branch:** `fix/mermaid-dark-theme`
**Date:** 2026-11-20
**Issue:** #257

## Summary

In dark mode every Mermaid diagram was a white card with dark text and pastel
boxes, unlike GitHub, where diagrams follow the page theme.

Diagrams are still rendered by merman with mermaid's light palette, then
recoloured in the background thread after rasterization: HSL lightness is
inverted with hue and saturation kept, and black is lifted to the code-block
background. White cards take the code-block colour, dark text turns light,
pastel fills turn dark with the same hue. The theme and the background colour
are part of a diagram's cache key, so toggling the theme renders it again.
Light mode is unchanged.

## Key Discoveries

- **merman's `dark` theme is not enough.** It can be selected through
  `HeadlessRenderer::with_site_config` (and partly through an
  `%%{init: {"theme": "dark"}}%%` directive), but
  `apply_dark_theme_defaults` (`merman-core/src/theme.rs`) seeds only a few
  variables (`background`, `primaryColor`, `textColor`, …). The renderers read
  `mainBkg`, `nodeBorder`, `lineColor`, `clusterBkg`, note colours and more
  through `theme_color(.., fallback)` with light fallbacks, and some diagram
  CSS is hardcoded. A flowchart rendered with `theme: dark` came out with
  light text (`#ccc`) on the light node fill (`#ECECFF`): worse than before.
- **Inverting HSL lightness is one shift for all three channels.** For fixed
  hue and saturation, `c' = c + 1 − (max + min)` keeps chroma (`max − min`)
  and hue, so the mapping needs no colour-space round trip
  (`mermaid_dark_pixel`).

## Architecture

- `mermaid_dark_pixel(rgb, background)` in
  `egui_commonmark_backend/src/misc.rs`.
- `rasterize_mermaid_svg(svg, dark_background: Option<[u8; 3]>)` applies it to
  every non-transparent pixel when set.
- `CodeBlock::render_mermaid` passes `extreme_bg_color` (the code-block
  background) in dark mode and hashes it with the diagram source.

## Testing

- `white_becomes_the_background_and_dark_text_becomes_light`,
  `colours_keep_their_hue_and_light_fills_turn_dark` (mermaid's own
  `#9370DB`, `#ECECFF`, `#ffffde`), `rasterizing_recolours_only_in_dark_mode`.
- Mutant: ignoring the background in the rasterizer turns the last test red.
- Xvfb screenshots of a flowchart with a subgraph, a sequence diagram with a
  note, a pie chart and a class diagram, in both themes.

## Future Improvements

- If merman gains a complete dark theme, rendering with it would give
  mermaid's own dark palette instead of a derived one.
