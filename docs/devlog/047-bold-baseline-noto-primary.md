# Fix: Bold baseline — make Noto Sans the primary body face

**Status:** ✅ Complete
**Branch:** `fix/39-bold-complete`
**Date:** 2026-07-15
**Lines Changed:** ~+10 / -2 in `src/main.rs` (`setup_fonts`)

## Summary

Completes the #39 bold fix. PR #42 added an opt-in strong-font path that renders
`**bold**` in **Noto Sans Bold** (`STRONG_FONT_FAMILY`), which makes bold
visible. But regular body text still rendered in egui's bundled **Ubuntu-Light**
— Noto Sans was only registered as a *fallback* (appended after the default), so
Latin body text never reached it. The two faces have different baseline/ascent
metrics, so every bold span sat a couple pixels low and looked like a font
switch mid-line.

## Fix

In `setup_fonts`, promote **Noto Sans Regular** from a fallback to the *primary*
proportional font — `insert(0, …)` instead of `push(…)` for the `NotoSans`
entry. Regular and bold text now both come from the Noto Sans family with
matching metrics, so bold aligns on the same baseline. CJK / Arabic / other
scripts stay as appended fallbacks (unchanged).

```rust
if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
    if *font_name == "NotoSans" {
        family.insert(0, font_name.to_string()); // primary body face
    } else {
        family.push(font_name.to_string());      // fallback
    }
}
```

## Tradeoff (intentional)

This shifts the app's entire body typeface from Ubuntu-Light to Noto Sans —
regular text changes appearance too, not just bold. That is a deliberate choice
for consistent bold rendering and was reviewed live before landing.

## Testing Notes

- Verified on Xvfb: bold text aligns on the same baseline as surrounding text and
  is one consistent typeface.
- No regression on emoji (#38), list code blocks (#44), or CJK (#40).
- Combined with PR #42's opt-in `use_strong_font_family(true)` (also on this
  branch), `**bold**` renders correctly.

## Related

- Builds on PR #42 (`aki1ro/fix/39-bold-rendering`), merged into this branch.
- See `docs/LESSONS.md` → "Strong markdown needs a registered bold family".
