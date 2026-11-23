# 116: AUR md-viewer-bin sources with versioned names (#282)

**Branch:** `fix/aur-bin-versioned-sources`
**Date:** 2026-11-23
**Status:** Complete

## Summary

Issue #282: building `md-viewer-bin` 0.3.11-1 failed with
`THIRD_PARTY_NOTICES ... FAILED`. The PKGBUILD's checksum was correct
(`780a707f…`, the file at `v0.3.11`). The source line saved the file as plain
`THIRD_PARTY_NOTICES`, and makepkg reuses a source file already in the build
directory instead of downloading it. AUR helpers keep that directory between
upgrades. `THIRD_PARTY_NOTICES` changed in 0.3.9 (#255): v0.3.6–v0.3.8 hash to
`c999057b…`, v0.3.9–v0.3.11 to `780a707f…`. A cache from 0.3.8 or earlier
therefore fails every later build.

## Reproduction

A clean clone of `https://aur.archlinux.org/md-viewer-bin.git` (0.3.11-1),
plus `git show v0.3.8:THIRD_PARTY_NOTICES > THIRD_PARTY_NOTICES` in the clone
as the stale cache:

| directory | `makepkg --verifysource` |
|---|---|
| stale cache | no `Downloading THIRD_PARTY_NOTICES` line, then `THIRD_PARTY_NOTICES ... FAILED`, rc 1 |
| clean clone (control) | all five `Passed`, rc 0 |

## Fix

- `aur-bin/PKGBUILD`: every aux source's local name carries `${pkgver}`
  (`md-viewer-${pkgver}.desktop`, `md-viewer-${pkgver}.png`,
  `LICENSE-${pkgver}`, `THIRD_PARTY_NOTICES-${pkgver}`). `package()` installs
  them under the same paths as before. The tarball's name already carried the
  version.
- The template's header was still at 0.1.7 with a `SKIP` sum; it now holds
  0.3.11 and real sums, so it matches what the AUR gets.
- `release.yml` (`publish-aur-bin`) needed no logic change: its Python rewrite
  replaces `pkgver=` and the five-element `sha256sums` array, and the source
  order is unchanged. Run on the new template with `VERSION=0.3.12` and dummy
  sums, it changed exactly those six lines; `makepkg --printsrcinfo` on the
  result lists `…-0.3.12` names.
- `PUBLISHING.md`: the `-bin` section said four sums and three aux files
  (there are five sources); it now explains the versioned names.

## Verification

- The new PKGBUILD in the stale-cache directory: `makepkg -f` downloads all
  four versioned files, all five pass, the package builds.
- Its `usr/` tree is identical (`diff -r`) to a package built from the old
  PKGBUILD without a cache, and each aux file is byte-identical to the file at
  the `v0.3.11` tag. The binary differs from the tarball's by 8 bytes in both
  packages: makepkg's default `strip`.
- Published to the AUR as 0.3.11-2 (`pkgrel` bump; the repo template keeps
  `pkgrel=1`, which CI uses for each new version).

## Key Discoveries

- A fresh build always downloads, so CI, a clean local build and a first
  install all pass. Only an upgrade with a populated cache fails, which is why
  this shipped in three releases (0.3.9–0.3.11) before a report. The
  `.desktop` file, icon and `LICENSE` had the same fixed names and were only
  safe because they had not changed.

## Future Improvements

- The release tarball has bundled `LICENSE` and `THIRD_PARTY_NOTICES` since
  0.1.14, so the PKGBUILD could install them from the extracted tarball and
  drop two downloads and two checksums. That changes the release job's sums
  list (five to three), which only runs on a tag, so it is left for a change
  that can be checked in a release.
