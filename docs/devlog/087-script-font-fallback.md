# Script font fallback: Thai tofu, Hebrew in monospace

**Status:** Complete
**Branch:** `fix/script-font-fallback`
**Date:** 2026-11-13

## Summary

Found while measuring #108 on 0.3.4. Thai text rendered as tofu boxes with
Noto Sans Thai installed, and Hebrew prose was set in Liberation Mono. Neither
is a shaping problem; both come from how the per-script fallback face is chosen.

## Root cause

`install_regular_fonts` asked fontique for the script's fallback families and
took the first face covering the sample. On this system fontique 0.11.1's
fontconfig backend returns **one** family per script:

| script | fontique answer | result |
|---|---|---|
| Thai | `Misc Fixed` (PCF bitmap) | not loadable → no Thai face installed → tofu |
| Hebrew | `Liberation Mono` | covers Hebrew → monospace prose |
| Arabic | `Noto Kufi Arabic` | fine |
| Devanagari | `Noto Sans Devanagari` | fine |

(Probe: `collection.fallback_families((Script, locale))` for each script, with
`en-US`, `th` and no locale; all gave the same single family.)

## Fix

`select_script_fallback` tries, in order:

1. the platform list, proportional faces only (`ttf_parser` `is_monospaced`);
2. families whose name contains the script's English name as a whole word
   (`ScriptFallback::name_hint`), ranked Sans → unmarked → Serif → Mono, then
   shorter names first, so `Noto Sans Thai` beats `Noto Sans Thai Looped`;
3. the generic sans-serif families;
4. the platform list including monospace faces (the old behaviour, as a last
   resort).

`select_from_families` gained a `_where` variant with a face filter; the
original delegates with an accept-all filter, so other callers are unchanged.

## Verification

- `installed_fonts_cover_reported_scripts` (ignored, needs fonts) was red on the
  unchanged code ("regular chain lacks" the Thai sample) and is green now; it
  also samples Arabic and Hebrew.
- New `script_fallbacks_are_proportional_faces` (ignored, needs fonts):
  red with the old selection ("SystemThaiFallback was not installed") and with
  a mutant that lets the platform tier accept monospace ("SystemHebrewFallback
  is monospace: Liberation Mono"), green with the fix.
- New pure test `script_named_families_rank_sans_and_base_family_first`.
- Xvfb screenshot of the #108 sample document: Thai renders, Hebrew is
  proportional (Noto Sans Hebrew). Arabic joining and RTL order are unchanged,
  as expected (#108).
- Font selection time, debug builds, warm: 127/144 ms on main vs 139/152 ms
  with the fix (two more faces load).

## Key Discoveries

- A mutant that changes a helper the test's own assertion also uses proves
  nothing: making `face_is_proportional` always true left the Hebrew assertion
  green because the assertion itself called it. The valid mutant changed the
  selection only.

## Future Improvements

- Han still resolves to WenQuanYi Zen Hei rather than Noto Sans CJK; it is
  proportional and covers the sample, so the platform answer stands.
