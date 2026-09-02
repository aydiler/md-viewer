# Feature: Smooth Text Rendering Toggle

**Status:** ✅ Complete
**Branch:** `feature/font-selection` (folded in alongside the font picker; originally developed on `feature/smooth-text-rendering`)
**Date:** 2026-09-02
**Lines Changed:** +~35 in `src/main.rs`, +2 in `README.md`

## Summary

User reported font rendering looked jagged/aliased. Adds a **View → Smooth
Text Rendering** toggle that disables egui's `round_text_to_pixels`
tessellation option. Off by default (preserves current behavior); persists
across restarts.

## Key Discoveries

### `round_text_to_pixels` is the actual knob for "jagged" text, not `feathering`

`epaint::TessellationOptions::feathering` explicitly documents "This setting
does not affect text" — it only smooths shape edges (rects, lines, circles).
The relevant option is `round_text_to_pixels` (default `true`): it snaps each
glyph's position to the physical pixel grid. That's normally what you want
for crisp text, but this app's zoom levels (`Ctrl+Scroll`, `Ctrl+/-`, 0.5–3.0
continuous) routinely produce a fractional effective `pixels_per_point`
(native scale × zoom). Under a fractional ratio, per-glyph rounding makes
letter spacing land inconsistently between characters, which reads as
"jagged" rather than crisp. Disabling the snap lets glyphs render at their
exact sub-pixel position, using epaint's normal alpha-coverage
anti-aliasing for the edges instead.

### `ctx.tessellation_options_mut()` is not covered by the startup memory-clear

`MarkdownApp::new()` clears `ctx.memory_mut(|mem| mem.data = ...)` at startup
to drop stale persisted egui widget state (see `docs/ARCHITECTURE.md`).
Tessellation options live under `ctx.memory().options`, a separate field —
unaffected by that clear, so no special handling was needed here.

### No last-applied change-gate needed

Every other cross-frame setting in this file (`dark_mode`, `highlight_color`,
`link_color`) uses a `last_applied_*` field to avoid rebuilding `Visuals`
every frame. `ctx.tessellation_options_mut()` is a single field write into
egui's own memory struct — cheaper than the comparison itself would be — so
it's applied unconditionally every frame, right next to the existing
unconditional `ctx.set_zoom_factor(self.zoom_level)` call, which follows the
same "just set it, it's free" precedent already in this codebase.

## Architecture

### New/Modified Fields

```rust
// PersistedState
smooth_text_rendering: Option<bool>, // None/Some(false) = pixel-snapped (today's default)

// MarkdownApp
smooth_text_rendering: bool,
```

No new functions — this is a straight persisted-bool-toggle following the
existing `full_width_content` pattern (menu button with a `✓` prefix when on,
no separate settings window).

## Testing Notes

- `cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo test`
  (56 passed, 1 ignored) all clean.
- Not verified visually in this sandbox (no GUI test tooling installed at the
  time of writing); logic mirrors the already-shipped `full_width_content`
  toggle exactly, so the persistence/menu wiring risk is low. Recommend a
  manual before/after screenshot comparison at a fractional zoom level (e.g.
  125%) to confirm the visual improvement matches the reported complaint.

## Future Improvements

- [ ] If glyphs still look thin/uneven after this, look at `FontTweak::scale`
      per-family, or exposing `feathering_size_in_pixels` (shape-only, not
      text, but worth ruling out for borders/backgrounds around text).
