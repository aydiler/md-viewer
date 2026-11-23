# 114: Startup with no system fonts

**Branch:** `fix/fontique-no-fonts`
**Date:** 2026-11-22
**Status:** Complete

## Summary

When fontconfig exposed no fonts at all, md-viewer died before its first frame:

```
panicked at fontique-0.11.1/src/backend/fontconfig.rs:685:61:
called `Result::unwrap()` on an `Err` value: NoMatch
```

`Collection::new` builds fontique's generic-family map by calling
`FcFontSort` for each generic name and unwrapping the result. With an empty
system font set fontconfig answers `NoMatch`. Both `FontLoader::new` (startup
and every font change) and `scan_pickable_font_families` (the Fonts dialog)
build a collection, so there was no path past it. Found while checking #255
with an empty fontconfig configuration (2026-11-20).

md-viewer itself already handled the case: with no system faces it installs
egui's bundled faces plus its own (`NotoEmoji`, `UiSymbols`) and logs a
warning. The crash was entirely inside fontique.

## Fix

fontique 0.12.0 replaced the unwrap with `let Ok(..) else { continue }`, with a
comment naming this case. Bumped fontique 0.11.1 → 0.13.0. The public API change
is additive only (`family_ids`, `first_available_font`, `FamilyNameMap::ids`,
and a lifetime on `Query::matches_with`, which md-viewer does not call), so no
source change was needed. The lockfile picks up read-fonts 0.46 (a new copy
beside 0.35 and 0.41), and font-types 0.12.7, parlance 0.1.1 and objc2 0.6.5,
each a minimum that fontique 0.13 or read-fonts 0.46 requires.

A `catch_unwind` workaround was not an option: release builds use
`panic = "abort"`.

## Test

`system_fonts::tests::startup_fonts_survive_a_fontconfig_with_no_fonts`.
fontconfig reads `FONTCONFIG_FILE` once per process, so the test re-runs its
own binary with `--exact` and an empty `fonts.conf`. The child builds the full
font set and runs the picker scan, asserts the bundled faces are in the
proportional chain, and prints a marker. The parent requires success **and** the
marker, so a child that matched no test cannot pass.

| fontique | result |
|---|---|
| 0.11.1 (mutant) | red: the child panics at `fontconfig.rs:685:61`, `NoMatch` |
| 0.13.0 | green |

## Verification

- Xvfb, `FONTCONFIG_FILE` pointing at an empty config:
  - main: exits with status 101 and the panic above;
  - branch: the window renders the document in egui's bundled faces;
  - the branch also opens View → Fonts (only "System Default" listed) and
    switches the preset to GitHub without a crash.
  - Non-Latin text shows as boxes, which is expected with no fonts to fall back on.
- Xvfb with the normal configuration: both builds select the same 2 base and
  13 full faces, and their document panes are pixel-identical.
- Release binary: 50 510 168 → 50 512 472 bytes (+2.3 KB).
- `cargo test`: 101 passed, 10 ignored. The test inventory against main differs
  only by the new test. clippy and fmt are clean, and `cargo publish --dry-run`
  succeeds.
