# Feature: Document Font Selection

**Status:** ✅ Complete
**Branch:** `feature/font-selection`
**Date:** 2026-08-31
**Lines Changed:** +226 / -29 across `src/main.rs` (+134/-24) and `src/system_fonts.rs` (+121/-5)

## Summary

Lets the user pick which installed system font renders the markdown body/heading
text (the `FontFamily::Proportional` chain), instead of always using the
auto-detected system sans-serif. Selection persists across sessions. The
monospace/code font is untouched.

## Features

- [x] `system_fonts::setup_fonts` accepts an optional preferred family name and
      falls back to today's auto-detected sans-serif chain when unset/not found
- [x] `setup_fonts` returns the sorted list of installed family names so the UI
      doesn't need a second `fontique::Collection` scan
- [x] `View → Font…` opens a searchable picker window (`egui::Window`)
- [x] Selection persisted in `PersistedState.selected_font_family`
- [x] Font reload only runs when the selection actually changes (mirrors the
      existing `last_applied_dark_mode` pattern), not per-frame

## Key Discoveries

### fontique name lookup is case-insensitive and safe to call every time

`Collection::family_id(&mut self, name: &str) -> Option<FamilyId>` matches
against a lowercased key (`fontique-0.11.1/src/family_name.rs`), so any name
returned from `family_names()` round-trips through `family_id()` without
needing exact-case bookkeeping.

### Bold companion resolution needed no changes

`install_strong_font_family` already derives the bold face from whichever
family was installed as `primary` (looked up by `regular.selected.family_id`),
regardless of whether that family came from the auto-detect path or a
user preference. Wiring the preferred family into the *same* `"SystemSans"`
primary slot in `install_regular_fonts` meant the existing strong-font-family
and script/generic fallback logic needed zero changes.

## Architecture

### New/Modified Structs

```rust
// PersistedState
selected_font_family: Option<String>, // None = system default (auto-detected)

// MarkdownApp
selected_font_family: Option<String>,
available_font_families: Vec<String>,   // populated once at startup from setup_fonts()'s return value
last_applied_font_family: Option<String>,
show_font_dialog: bool,
font_filter: String,                    // transient UI-only search text, not persisted
```

### New Functions

| Function | Purpose |
|----------|---------|
| `system_fonts::setup_fonts(ctx, preferred_family: Option<&str>) -> Vec<String>` | Same as before, plus optional named-family preference; now returns the sorted family name list |
| `MarkdownApp::render_font_settings(&mut self, ctx)` | Renders the `View → Font…` picker window |

## Testing Notes

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`
  (56 passed) all clean.
- Added two new `system_fonts` tests behind `#[ignore = "requires installed
  system fonts"]` (matching the existing sibling test's convention, since
  CI's `ubuntu-latest` image isn't guaranteed to have real fonts installed):
  `unknown_preferred_family_falls_back_to_auto_detect` and
  `known_preferred_family_becomes_primary` (the latter is self-consistent —
  it discovers whatever the auto-detected default sans is, then re-requests
  it by name, so it doesn't hardcode a font that might not exist on every
  machine). Ran locally with `cargo test -- --ignored`: both pass, along with
  the pre-existing ignored coverage test.
- Manual end-to-end verification via Xvfb + xdotool + ImageMagick `import`
  (this sandbox had no toolchain or GUI test tools at all going in; installed
  rustup, Fedora build deps, and the Xvfb/xdotool/ImageMagick stack already
  used by `scripts/visual-regression.sh`, then drove the real binary):
  - Opened `View → Font…`, confirmed the picker lists real installed families
    (Adwaita Sans, Cantarell, Droid Sans, etc.) with "System Default" marked
    selected.
  - Typed "Caladea" in the search box — list filtered live to the one match.
  - Clicked it — **entire UI** (menu bar, sidebar, tabs, document body,
    heading, and `**bold**` text) switched to the serif face immediately,
    confirming both the regular and bold-companion face resolution worked.
  - Closed and relaunched the binary (same `XDG_DATA_HOME`/`XDG_CONFIG_HOME`)
    — Caladea was still applied on the very first frame, and the View menu
    correctly read "Font: Caladea…".
- Known, expected scope: the font choice governs `FontFamily::Proportional`,
  which is what nearly all egui widgets use by default — so it restyles the
  whole app's UI text, not just the markdown pane. This matches how the
  existing dark/light and zoom settings behave (global, not per-document) and
  was not flagged as a problem during manual testing.

## Future Improvements

- [ ] Separate monospace/code-font picker (explicitly out of scope here per
      user request, which was about "a font" for the document, not code)
- [ ] Live preview sample text in the picker window

## Optimization Pass (2026-09-01)

User confirmed the feature worked, then asked for a pass to optimize it.
Two real (not micro-) issues found by re-reading the diff against
`docs/EGUI_WORKFLOW.md`'s "never allocate in the render loop" rule and the
existing `render_outline`/`show_rows` precedent in this same file:

1. **Per-frame allocation while the picker is open.** The filter loop ran
   `name.to_ascii_lowercase()` for every installed font family (100–300+ on a
   typical system) on every repaint — and repaints happen on a timer, not
   just on click/keystroke, because `update()` already schedules periodic
   `request_repaint_after` while file-watching is on. So this cost was paid
   repeatedly for as long as the dialog stayed open, not just once.
   **Fix:** precompute `available_font_families_lower: Vec<String>` once at
   startup (same one-time-lowercase idiom `find_matches` already uses per
   `docs/LESSONS.md`'s `to_ascii_lowercase` note), and only build a
   `Vec<usize>` of matching indices when the search box is non-empty — the
   common empty-filter case now allocates nothing per frame.
2. **All matching rows got a widget every frame regardless of scroll
   position.** Switched `ScrollArea::show` to `ScrollArea::show_rows`,
   mirroring `render_outline`'s existing virtualization (see
   `docs/LESSONS.md`, "show_rows for the outline drops O(headers) per frame
   to O(visible)"). "System Default" is row 0, unconditionally pinned and
   exempt from filtering (unchanged behavior).

Also dropped an unnecessary `persisted.selected_font_family.clone()` in
`MarkdownApp::new()` in favor of a plain move — `persisted`'s other fields
are already partial-moved out field-by-field elsewhere in the same
constructor (`open_tabs`, `explorer_root`, etc.), so this just matches
existing style instead of paying for a clone nothing needed.

Re-verified after the change: `cargo fmt --check` / `cargo clippy --all-targets
-- -D warnings` / `cargo test` (56 passed, 3 ignored) all clean, and a fresh
Xvfb pass confirmed the unfiltered list, the "sans" search filter, and
clicking a filtered result (Liberation Sans) all still work identically to
before — the optimization changed only how the data gets to the screen, not
what appears.
