# Feature: Markdown Font Presets (Default / GitHub / VS Code)

**Status:** ✅ Complete
**Branch:** `feature/font-presets`
**Date:** 2026-08-23
**Lines Changed:** +~230 / -~20 in `src/system_fonts.rs`, `src/main.rs`

## Summary

Adds a **View → Fonts** menu with three presets that switch which markdown
viewer's font choices lead the app's font chains:

- **Default** – the app's current look (best installed sans + bundled mono).
- **GitHub** – emulates github.com rendered markdown (Primer `.markdown-body`).
- **VS Code** – emulates the VS Code markdown preview defaults.

The choice persists across sessions and hot-swaps at runtime via
`egui::Context::set_fonts`.

## Features

- [x] `FontPreset` enum (`Current`, `Github`, `Vscode`) with serde persistence
- [x] Preset body families lead the primary sans chain (bold/strong follows
      automatically because `MarkdownStrong` resolves from the primary family)
- [x] Preset code families lead the monospace chain (all inline/block code)
- [x] View menu section with ✓ marks, hover explanations, MCP registration
- [x] Graceful fallback: missing preset faces degrade to the existing chain
- [x] Per-preset line heights (GitHub 1.5/1.45, VS Code 1.6/1.36); base size
      is no longer a preset property
- [x] Shared text-size classes applying to every preset: Small 14 / Normal 16
      (default) / Large 18 / Extra large 20 / Huge 24, selectable in
      View → Fonts ▸ with ✓ marks; persisted via
      `PersistedState.text_size_class`
- [x] Unit tests for stack contents and resolution order
- [x] Zoom hygiene: multiplicative steps (×1.25) shared by keyboard, menu,
      and Ctrl+wheel/pinch via one `zoomed()` helper; redundant per-frame
      `set_zoom_factor` writes skipped via `last_applied_zoom_level`

## Research: exact fonts used by the emulated viewers

### GitHub (verified against live github.githubassets.com CSS + Primer source)

Effective `.markdown-body` stack (Primer primitives token
`--fontStack-sansSerif`; Mona Sans was prepended 2026-03 in
primer/primitives#1332):

```css
"Mona Sans VF", -apple-system, BlinkMacSystemFont, "Segoe UI",
"Noto Sans Backtick Fix", "Noto Sans", Helvetica, Arial, sans-serif,
"Apple Color Emoji", "Segoe UI Emoji"
/* font-size:16px; line-height:1.5 */
```

Code (`--fontStack-monospace`, applied to `code/tt/samp/pre/kbd`):
`ui-monospace, SFMono-Regular, SF Mono, Menlo, Consolas, Liberation Mono,
monospace` (85% size for code/pre).

GitHub ships **no webfont** for markdown; "Noto Sans Backtick Fix" is a
`local()`-only `@font-face` shim covering only U+60, so it is skipped in our
emulation. "Mona Sans VF" renders only where installed locally
(github/mona-sans).

### VS Code markdown preview

Verified from microsoft/vscode source AND the locally installed v1.127 files
under `/usr/lib/code/extensions/markdown-language-features/media/markdown.css`:

```css
html, body { font-family: var(--markdown-font-family,
  -apple-system, BlinkMacSystemFont, "Segoe WPC", "Segoe UI", system-ui,
  "Ubuntu", "Droid Sans", sans-serif);
  font-size: var(--markdown-font-size, 14px); }
code { font-family: var(--vscode-editor-font-family,
  "SF Mono", Monaco, Menlo, Consolas, "Ubuntu Mono", "Liberation Mono",
  "DejaVu Sans Mono", "Courier New", monospace); }
```

Inside VS Code the variable is always injected: Linux `editor.fontFamily`
default is `'Droid Sans Mono', monospace` (`fontInfo.ts`), so that is the
effective code stack we emulate.

## Key Discoveries

### Discovery 1: fontdb does not apply fontconfig aliases

`fontdb` matches literal family names only. Browsers on this machine resolve
`"Segoe UI"` to **Adwaita Sans** (fontconfig substitution) and generic
`monospace` to **Noto Sans Mono**, but our resolver would miss both. The preset
lists therefore transcribe CSS-generic keywords into their common Linux
resolutions:

- `"Adwaita Sans"` sits directly after `"Segoe UI"` (and system-ui proxies:
  Adwaita Sans → Cantarell).
- GitHub's leading `ui-monospace` becomes Noto Sans Mono / DejaVu Sans Mono
  (they lead the list, mirroring resolution order on Linux browsers).
- `-apple-system` / `BlinkMacSystemFont` are macOS-only keywords → skipped.

### Discovery 2: one insertion point per chain covers everything

- Body: the primary `SystemSans` spec just resolves against the preset's list;
  CJK/script fallbacks and the strong/bold family keep working unchanged
  because they hang off the resolved primary family name.
- Code: inserting `PresetMono` at index 0 of `FontFamily::Monospace` switches
  every code span (renderer uses `RichText::code()` → Monospace style).
- Strong inline code intentionally keeps the mono face (#39 lesson).

### Discovery 3: runtime font switching needs no cache

`setup_fonts(ctx, preset)` rebuilds `FontDefinitions` from a fresh `fontdb`
scan (~tens of ms) — fine for an occasional menu toggle, avoids caching font
bytes per preset. `ctx.set_fonts` re-tessellates automatically.

### Discovery 4: set_fonts skips identical definitions — presets need metrics

`Context::set_fonts` diffs against installed definitions (TTF-data equality)
and returns early on match. On stock Linux, GitHub and VS Code stacks resolve
to the *same* faces (Adwaita Sans + Noto Sans Mono), so GitHub ↔ VS Code
switching was visually a no-op even though state updated. Fix: presets now
also carry the viewers' typography metrics — base body size (16px vs 14px,
applied to `TextStyle::Body`/`Heading`/`Monospace`; the renderer derives every
document size from Body) and renderer line heights (GitHub 1.5/1.45, VS Code
1.6/1.36). See LESSONS.md.

## Architecture

### New/Modified Types

```rust
// src/system_fonts.rs
pub(crate) enum FontPreset { #[default] Current, Github, Vscode }
//   ALL, label(), description(), body_families(), mono_families()
const GITHUB_BODY_FAMILIES / GITHUB_MONO_FAMILIES
const VSCODE_BODY_FAMILIES / VSCODE_MONO_FAMILIES
```

`PersistedState.font_preset: Option<FontPreset>` and
`MarkdownApp.font_preset: FontPreset` persist the choice.

### Modified Functions

| Function | Change |
|----------|--------|
| `setup_fonts(ctx, preset)` | New param; installs preset faces then `set_fonts` |
| `install_regular_fonts(..., preset_body_families)` | Primary spec resolves preset list first; falls back to app defaults if none installed |
| `install_preset_mono_font(db, fonts, preset)` | New; front-inserts the first installed preset code face |

### UI

View menu, between Full Width and Zoom: `Fonts: Default | GitHub | VS Code`
with ✓ mark, hover description, and MCP ids
`Menu: View → Font Preset: <label>` for E2E testing.

## Testing Notes

Unit tests cover: stack ordering vs the real CSS stacks (Mona Sans head,
system-ui before Ubuntu), Linux monospace resolutions leading, GitHub
preferring its stack over the app default, fallback when no preset face is
installed, first-installed-face-wins selection, and a RON persistence
roundtrip through an in-memory `eframe::Storage` mock (incl. legacy state
without the field).

Live verification on Arch/GNOME (`RUST_LOG=info`): Default keeps Noto Sans +
bundled mono; GitHub resolves body → **Adwaita Sans** (Segoe UI fontconfig
substitution) + code → **Noto Sans Mono**; VS Code resolves identically on
this box (faithful — both stacks converge on stock Linux; they diverge where
Mona Sans/Arial/Ubuntu/Droid Sans are installed). The `--ignored` test
`presets_reorder_the_applied_font_chains` asserts the GitHub preset installs
`PresetMono` at the monospace chain head while Current does not.

Known emulation limits: fontconfig aliasing can differ per distro, so lists
encode the common cases. Family-level differences between GitHub and VS Code
only appear where their named faces (Mona Sans, Arial, Ubuntu, Droid Sans
Mono…) are installed; on stock Linux the presets are distinguished by their
metrics instead.

## Future Improvements

- [ ] Custom font family picker on top of the presets
