# Gate color emoji so the crates.io build compiles

**Status:** ✅ Complete
**Branch:** `fix/crates-color-emoji`
**Date:** 2026-11-12

## Summary

The color-emoji support from 0.3.0 sets `FontData::emoji_only`, a field that
exists only in the vendored epaint (`crates/epaint`, wired in through
`[patch.crates-io]`). `cargo publish` ignores patches, so the md-viewer package
on crates.io is compiled against stock epaint 0.33.3 and does not build. This
blocked the 0.3.1 release.

The color-emoji code is now behind a `color-emoji` cargo feature:

- On by default in the repository, so every source build (snap, both AUR
  packages, the GitHub Release binaries, CI, local dev) is unchanged.
- The `publish-crates` job rewrites `default = ["color-emoji"]` to
  `default = []` before publishing, the same pattern as the existing MCP-strip
  step. It fails if that line is missing, so a renamed feature cannot silently
  ship a package that does not compile.
- Without the feature, emoji fall back to the bundled monochrome Noto Emoji
  face, which is registered unconditionally.

## Verification

See the PR description for the measured matrix: `cargo publish --dry-run`
fails on `main` and passes with the transform applied; default and
`--no-default-features` builds, clippy and tests are green.

## Key Discoveries

- Nothing but `cargo publish --dry-run` builds what crates.io users get. CI, the
  snap and AUR all compile the git tree with the patch applied, so the breakage
  was invisible everywhere else.
- `scripts/check-fork-publishable.sh` guards only the egui_commonmark fork,
  which is published under its own name. epaint is patched in place under its
  upstream name and cannot be published, so anything it adds has to be
  feature-gated instead.

## Future Improvements

- Run `cargo publish --dry-run -p md-viewer` with the transform applied as a
  step inside `publish-crates` before the fork crates go out, so a future
  patch-only API fails before anything is uploaded. It has to stay in that job,
  not in `validate` (see LESSONS, "A fail-fast check belongs in its own job").
