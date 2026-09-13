# Feature: One Fonts menu point + picker list that always works

**Status:** ✅ Complete
**Branch:** `feature/fonts-one-menu`
**Date:** 2026-09-12
**Lines Changed:** see `git diff main --stat` (src/main.rs, src/system_fonts.rs)

## Summary

The View menu had two font entries — "Font: <family>…" (opening the family
picker dialog) and "Fonts: <preset>" (a submenu with the markdown presets and
text-size classes) — and most families listed in the picker did nothing when
selected. Both entries are now a single "Fonts…" item opening one dialog that
holds the preset row, the text-size row, and the family picker, and the family
list is filtered so every name in it actually changes the document font.

## Features

- [x] Single "Fonts…" entry in the View menu (replaces "Font: …" + "Fonts:" submenu)
- [x] One "Fonts" dialog: Preset row, Text Size row, searchable family list
- [x] Family list offers one canonical name per family, body-capable only
- [x] Family scan runs on a background thread (~0.5 s), dialog shows a scanning note
- [x] All dialog changes flow through the existing `last_applied_font_config` diff gate

## Key Discoveries

### Fontique's `family_names()` is not a picker list

`Collection::family_names()` reports every FC_FAMILY string fontconfig knows,
including localized names and weight-instance names ("Noto Sans Black",
"Noto Sans Light", …) which are *aliases* of the base family, plus
script-specific families ("Noto Sans Devanagari", emoji, symbol faces). On a
stock Arch system: **742 names, 317 distinct families**, and ~481 names cannot
cover basic Latin at all. `install_regular_fonts` requires "Aa" coverage, so
picking any of those logged a warning and silently fell back to the
auto-detected default — the "most fonts don't change anything" report. Weight
aliases that did resolve picked the base family's regular face — also no
visible change.

**Fix:** `scan_pickable_font_families()` keeps a name only if
(a) it is the family's canonical name (`family_name(family_id(name)) == name`,
collapsing alias groups to one entry) and (b) `select_from_families` can pick a
normal-style face covering "Aa" — the same gate the installer applies, so every
listed name is guaranteed to take effect.

### The scan must stay off the UI thread

Probing Latin coverage loads every candidate face; ~420 ms cold on this system.
The explorer's `mpsc` + `try_recv` poll pattern fits: spawn
`font-family-scan` in `new()`, poll in `update()`, fill
`available_font_families`/`_lower` once, `request_repaint()` so an already-open
dialog picks the list up, drop the channel. Until it lands the dialog shows
"System Default" plus a "Scanning installed fonts…" note.

### Why presets looked broken, too

A family picked earlier outranks the preset's body stack (by design), so with
e.g. "Adwaita Sans" persisted, switching Default→GitHub barely changes
anything — and Adwaita Sans *is* the auto-detected default here, so the family
pick also looked like a no-op. The dialog now prints a hint under the preset
row whenever a specific family is picked ("A picked family leads the body
chain; presets still set the code font and line heights."). See LESSONS:
"egui silently skips set_fonts when definitions compare equal" for the other
half of historical preset no-op reports.

## Architecture

### Modified state (`MarkdownApp`)

```rust
available_font_families: Vec<String>,        // now filled by the scan, empty at startup
available_font_families_lower: Vec<String>,  // unchanged purpose
pending_font_family_scan: Option<Receiver<Vec<String>>>, // in-flight scan
```

`setup_fonts` no longer returns the family-name list (it built the raw, unusable
picker list); it returns `()`.

### New functions

| Function | Purpose |
|----------|---------|
| `system_fonts::scan_pickable_font_families()` | Own Collection + SourceCache; returns canonical, body-capable family names |
| `system_fonts::pickable_family_names()` | Testable inner filter (canonical-name dedup + "Aa" gate) |

### UI

View menu: one `Fonts…` button (MCP id `Menu: View → Fonts`) → `render_font_settings`
renders the "Fonts" window: `Font Dialog: Preset: <label>` and
`Font Dialog: Size: <label>` selectable rows, `Font Dialog: Search`,
`Font Dialog: System Default`, then the filtered family rows. Handlers only
mutate state; the `update()` diff gate rebuilds chains.

## Testing Notes

- `cargo test`: 76 pass (5 ignored need installed fonts).
- `cargo test -- --ignored`: all 5 pass, including new
  `pickable_family_list_is_deduplicated_and_body_capable` (asserts sorted,
  one-name-per-family, canonical, body-capable, and strictly smaller than the
  raw name count).
- Xvfb smoke run: startup clean, View menu shows exactly one "Fonts…", dialog
  renders all three sections, GitHub preset click flips instantly, Arial pick
  visibly re-renders the whole UI in Liberation Sans. Search field focuses
  (xdotool couldn't type into Xvfb; the filter code is unchanged from the old
  dialog).

## Future Improvements

- [ ] Re-run the scan when fontconfig files change (new fonts installed mid-session)
- [ ] Per-family preview in the picker rows (render the name in its own face)
- [ ] Show a count ("N families match") next to the search field
