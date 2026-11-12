# Build the crates.io package in the release dry run

**Status:** ✅ Complete
**Branch:** `ci/crates-package-dry-run`
**Date:** 2026-11-12

## Summary

The release dry run (devlog 075) builds Linux, macOS, Windows and the snap, but
not the package `cargo publish` uploads. That package is the only build that
resolves epaint from crates.io instead of the vendored copy, and it is where
0.3.1 broke (#215). Nothing compiled it before a tag.

The new `crates-package-check` job runs only on `workflow_dispatch` and does

```bash
python3 scripts/prepare-crates-manifest.py
cargo publish --dry-run --allow-dirty -p md-viewer \
  --config 'patch.crates-io.egui_commonmark_extended.path="crates/egui_commonmark/egui_commonmark"' \
  --config 'patch.crates-io.egui_commonmark_backend_extended.path="crates/egui_commonmark/egui_commonmark_backend"'
```

## Architecture

- **`scripts/prepare-crates-manifest.py`** holds the two `Cargo.toml` rewrites
  that `publish-crates` used to do inline: strip the local-only MCP dependency
  (same anchored regexes) and turn `default = ["color-emoji"]` into
  `default = []`, failing unless exactly one line matched. `publish-crates` and
  the dry-run job both call it, so the dry run verifies the manifest the tag
  publishes.
- **The fork is patched, epaint is not.** `[patch.crates-io]` in the manifest
  is ignored by `cargo publish`, but a `--config` patch is honoured by its
  verify build. Only the two egui_commonmark fork crates are patched:
  - On a tag, `publish-crates` publishes the fork from this same checkout
    before md-viewer, so the local fork is what crates.io will hold.
  - `validate` (`check-fork-publishable.sh`) separately fails if a fork version
    already on crates.io differs from the local source.
  - Without the patch the check builds against whatever fork version is
    already on crates.io, which is stale whenever the fork changed — and fails
    outright when md-viewer needs an API the published fork lacks.
  - epaint stays the registry's: that is the #215 condition.
- **No `needs`.** The job uses nothing from `validate`, and it should still
  report when `validate` fails for another reason (a stale fork, for one).

## Verification

Local, nightly cargo, each direction observed with the exit code captured
directly (no pipe):

| run | expected | result |
|---|---|---|
| transform + fork patch | pass | rc=0; fork compiled from local paths, epaint 0.33.3 from the registry |
| fork patch, **no** color-emoji transform | fail with the #215 error | rc=101, `error[E0609]: no field emoji_only on type FontData` at `src/system_fonts.rs:783` |
| transform, **no** fork patch | (predicted: fail) | **rc=0** — downloaded the published `egui_commonmark_extended 0.28.4` and compiled against it |

The third row disproved my prediction. md-viewer calls nothing that #220 added
to the fork, so it compiles against the stale published fork too. The patch is
therefore not what makes the check pass today; it is what makes the check build
the fork code that will actually be published. Without it the check verifies
against the wrong fork: silently green while md-viewer and the fork agree,
spuriously red on the first release where md-viewer uses a new fork API.

## Key Discoveries

- `cargo publish` ignores the *manifest's* `[patch]`, but its verify build does
  apply a config-level `[patch]` (`--config`). That is what lets a dry run
  build a package whose fork dependency is not published yet.
- `egui_commonmark_extended@0.28.4` is stale after #220 (`check-fork-publishable.sh`
  flags `latex_delimiters.rs` and `pulldown.rs`). 0.3.2 must bump the fork to
  0.28.5, or `validate` fails the release.

## Future Improvements

- Run the same check in PR CI, at least when `Cargo.toml`, `src/` or
  `crates/epaint` change, so a #215-class error surfaces at review time instead
  of at the pre-release dry run.
