# Fix: Emoji fonts did not load outside the developer's machine

**Status:** ✅ Complete
**Branch:** `fix/emoji-font-loading`
**Date:** 2026-11-11
**Files:** `src/system_fonts.rs`, `THIRD_PARTY_NOTICES`, `assets/fonts/OFL.txt`

## Summary

Both bugs were found while getting CI green (#212). They came in with #204 and
were never released.

1. **The bundled monochrome emoji font was read from the build machine's path.**
   `EMOJI_FONT_PATH = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/…")`
   was passed to `std::fs::read` at runtime. `env!` is evaluated at compile
   time, so the path is wherever the source tree sat during the build: a CI
   runner directory for the snap, the tarball and `md-viewer-bin`, and a
   cargo registry directory for `cargo install`. On every user's machine the
   read failed. The only symptom was a `warn!`, so the emoji fallback silently
   went missing.
2. **Color emoji only worked on Arch.** `COLOR_EMOJI_FONT_PATHS` listed Arch's
   two locations. Debian and Ubuntu (`fonts-noto-color-emoji`) install to
   `/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf`.

## Fix

- `include_bytes!("../assets/fonts/NotoEmoji-Regular.ttf")`, loaded with
  `FontData::from_static`. The runtime read and its error branch are gone.
  The binary grows by about 2 MB. `cargo package --list` already included
  the asset, so crates.io builds compile.
- Added the Debian/Ubuntu path. Fedora's `Noto-COLRv1.ttf` is deliberately
  not listed, because the swash patch rasterizes CBDT bitmaps and not COLRv1.
- Embedding makes the binary a font distributor, so `THIRD_PARTY_NOTICES` now
  carries the Noto Emoji copyright and the full OFL 1.1 text. That file ships
  in the tarballs, the snap and the AUR package; `assets/fonts/OFL.txt` alone
  would not. The text is from `google/fonts/ofl/notoemoji/OFL.txt`.

## Tests

| test | runs in CI | guards |
|---|---|---|
| `bundled_emoji_face_is_always_registered_last` | yes | bug 1: face present, last in both chains |
| `bundled_emoji_face_alone_renders_doc_emoji` | yes | bug 1: glyph coverage + non-blank raster |
| `emoji_fallback_face_resolves_for_doc_emoji` | ignored (system fonts) | color path |
| `color_emoji_face_never_claims_ui_symbols` | ignored (system fonts) | `emoji_only` restriction |

**Observed red:** with the loader mutated back to a runtime read of a missing
path (what users' machines saw), both new tests fail. The old code would have
passed them *in this checkout*, because the build path exists here. That is
exactly why the bug survived local testing.

## Key Discoveries

- `env!`/`concat!` in a runtime file path is a compile-time constant. It reads
  correctly on the machine that built the binary and nowhere else. Tests
  written on the build machine cannot catch it unless the read target is made
  to disappear.
- The two system-font tests had never run in CI. The `check` job stopped at
  `cargo fmt` for every push after #204, which hid both the test portability
  problem and the product bug behind a formatting failure.

## Future Improvements

- Color emoji inside the snap: host fonts aren't at `/usr/share/fonts` under
  strict confinement. They would have to be looked up via fontconfig or the
  snap's host-fonts mount.
- Fedora COLRv1 support would need a COLR renderer in the epaint patch.
