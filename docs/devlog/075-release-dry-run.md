# Release workflow dry run (`workflow_dispatch`)

**Status:** ✅ Complete (pending the first dispatch run after merge)
**Branch:** `ci/release-dry-run`
**Date:** 2026-11-12

## Summary

`release.yml` only ran on a `v*` tag push, so nothing exercised it before a
release. Two consecutive problems were found only after tagging:

- #215: md-viewer did not compile as a crates.io package (a field that only
  exists in the vendored epaint).
- #217: GitHub retired the `macos-14` runner image. The v0.3.1 macOS build was
  cancelled before getting a runner, and every publish job (`needs: build`) was
  skipped. The tag then had to be moved.

`gh workflow run release.yml --ref main` now runs the same pipeline as a dry run:

| job | tag push | dry run |
|---|---|---|
| validate | tag == Cargo == snapcraft, fork publishable | Cargo == snapcraft, fork publishable |
| build (linux, macOS, windows) | runs | runs |
| publish-snap | builds and uploads | **builds only**; upload and metadata steps skipped |
| check-crates-token, publish-crates, publish-aur, publish-aur-bin, create-release | run | skipped |

## Architecture

- `validate` derives the version once and exposes it as the job output `version`.
  On a dry run there is no tag, so the Cargo.toml version is used and still
  compared against snapcraft.yaml.
- `build` reads `VERSION` from that output (job-level `env`) instead of parsing
  `GITHUB_REF` in three places. On a branch ref those would have produced
  `refs/heads/main` as the version.
- Gating uses job-level `if: github.event_name == 'push'`. Job-level `if:` may
  use the `github` context (it may not use `secrets`/`env`; see LESSONS).
- The AUR jobs still parse `GITHUB_REF`, which is fine because they only run on
  push.

## Verification

YAML parse + job/step `if:` structure checked locally (no `actionlint` on this
machine). The real check is the first dispatch run after merge: every build leg
and the snap build must succeed and every publish job must show *skipped*; then
crates.io and the Releases page must be unchanged.

## Future Improvements

- Run `cargo publish --dry-run` (with the color-emoji transform) inside the dry
  run once the fork crates are already at the target version on crates.io.
  Before that, the dry run would fail on the unpublished fork.
- A scheduled weekly dry run would catch runner-image retirement between releases.
