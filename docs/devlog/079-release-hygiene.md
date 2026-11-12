# Release hygiene: metainfo releases, snap listing fields, crates.io check on PRs

**Status:** Complete
**Branch:** `chore/release-hygiene`
**Date:** 2026-11-12

## Summary

Three small gaps found while shipping 0.3.2:

1. **`data/io.github.aydiler.md-viewer.metainfo.xml` stopped at 0.2.0.** Five
   releases (0.2.1 to 0.3.2) were missing from the AppStream release history
   that Flathub and software centers show. Nothing checked it.
2. **The snap build warned about six missing listing fields**: `title`,
   `contact`, `license`, `issues`, `source-code`, `website`.
3. **The crates.io package check only ran in the release dry run.** A
   crates.io-only build error (#215) would still reach `main` unnoticed until
   someone dispatched the dry run.

## Change

- metainfo: `<release>` entries for 0.2.1, 0.2.2, 0.3.0, 0.3.1, 0.3.2, written
  from `CHANGELOG.md` (user-facing items only). The 0.1.4–0.1.17 gap is left
  as is.
- `ci.yml` `version-sync`: also compares the newest metainfo `<release
  version>` against `Cargo.toml`, so the next release cannot skip it. Step 4 of
  `.claude/rules/release-workflow.md` says so.
- `snap/snapcraft.yaml`: the six fields. `license: MIT` matches `Cargo.toml`;
  the URLs point at the GitHub repository and its issues.
- `ci.yml` new job `crates-package`: the same steps as release.yml's
  `crates-package-check` (prepare the manifest, then `cargo publish --dry-run`
  with the egui_commonmark fork patched in through `--config`). The release
  job stays, because the dry run still uses it.

## Verification

- `appstreamcli validate --no-net` on the metainfo: successful.
- The metainfo check, run as a shell function: passes on this branch
  (0.3.2 = 0.3.2), fails on a copy with the newest release set to 0.0.0, and
  fails on `main`'s metainfo (0.2.0), so it would have caught this drift.
- `snapcraft.yaml` and `ci.yml` parse as YAML; the new job's steps differ from
  the release job's only in the cache key.
- PR CI runs the new job.

## Future Improvements

- The snap build is not part of PR CI, so the new snapcraft fields are first
  built by the next release dry run.
