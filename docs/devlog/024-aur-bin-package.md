# Feature: `md-viewer-bin` AUR package

**Status:** 🚧 In Progress
**Branch:** `feature/aur-bin`
**Date:** 2026-05-16

## Summary

Adds a prebuilt-binary AUR package (`md-viewer-bin`) alongside the existing source-build `md-viewer-git`. Users who want pacman-managed updates without compiling the Rust toolchain (~2–3 min build) can now install via `yay -S md-viewer-bin` and pull the GitHub Releases tarball directly.

## Features

- [x] `aur-bin/PKGBUILD` — prebuilt-binary recipe
- [x] CI: `publish-aur-bin` job in `release.yml` mirroring `publish-aur`
- [x] `PUBLISHING.md` section documenting the new package
- [ ] Local `makepkg` smoke test (see Verification)
- [ ] First release tag triggers the new job

## Key Discoveries

### Release tarball is binary-only

`md-viewer-VERSION-linux-x86_64.tar.gz` ships just the `md-viewer` ELF — no `.desktop`, no icon, no `LICENSE`. So the PKGBUILD pulls those three files from raw GitHub at the tagged commit:

```bash
source=(
    "...releases/download/v${pkgver}/md-viewer-${pkgver}-linux-x86_64.tar.gz"
    "md-viewer.desktop::https://raw.githubusercontent.com/aydiler/md-viewer/v${pkgver}/data/md-viewer.desktop"
    "io.github.aydiler.md-viewer.png::https://raw.githubusercontent.com/aydiler/md-viewer/v${pkgver}/data/io.github.aydiler.md-viewer.png"
    "LICENSE::https://raw.githubusercontent.com/aydiler/md-viewer/v${pkgver}/LICENSE"
)
```

Trade-off vs bundling them into the release tarball: PKGBUILD has 4 sources + 4 sha256s instead of 1, but `release.yml`, `scripts/install.sh`, and the published tarball layout stay untouched.

### `ldd` doesn't validate the dep set

Running `ldd target/release/md-viewer` shows only `libc`, `libm`, `libgcc_s`, `ld-linux` linked directly. Everything else — `libxcb`, `libxkbcommon`, `openssl`, `gtk3`, `fontconfig`, `dbus` — is `dlopen`'d at runtime (winit / rfd / zbus do this for portability). So the source-build PKGBUILD's `depends=` list isn't validatable via `ldd` against the prebuilt binary; the safe choice is to match it verbatim. `namcap` on the final `.pkg.tar.zst` is the right validator.

### CI rewrites `pkgver` + all four sha256s

Mirrors the existing `publish-aur` job. New wrinkle: `pkgver()` doesn't exist on `-bin` packages (no git source), so the literal `pkgver=` line and the multi-element `sha256sums=( ... )` array both get rewritten in CI. `sed` on a multi-line array is fragile, so the rewrite uses a small inline Python script.

The four sha256s come from:
- Tarball: the published `<asset>.sha256` file from the GitHub release.
- Three aux files: computed in CI via `curl -fsSL <raw-url> | sha256sum`.

### Step-level secret gating + chown restore

Both gotchas from `publish-aur` (`LESSONS.md` → "GitHub Actions blocks `secrets.*` AND `env.*` in job-level `if:`" and "Docker bind-mount `chown` breaks host runner ownership") apply identically to the new job. Pattern is copied verbatim.

## Architecture

### New files

- `aur-bin/PKGBUILD` — pkg recipe (committed with real v0.1.7 hashes so the file is installable as-is, not just a CI template).
- `docs/devlog/024-aur-bin-package.md` — this file.

### Modified files

- `.github/workflows/release.yml` — appends `publish-aur-bin` job after `publish-aur`.
- `PUBLISHING.md` — new "AUR (`-bin` variant)" subsection.

### Conflicts

`provides=('md-viewer')` + `conflicts=('md-viewer' 'md-viewer-git')` ensures only one variant is installed at a time. The matching `conflicts=('md-viewer')` in `aur/PKGBUILD` (for `md-viewer-git`) does *not* mention `md-viewer-bin` — that one-way conflict is intentional: the bin package knows about the git package, but the existing git package shouldn't need an edit just to land this PR. Pacman resolves conflicts symmetrically anyway.

## Verification

1. **Local build smoke test** (no AUR push):
   ```bash
   docker run --rm -v "$PWD/aur-bin:/pkg" -w /pkg archlinux/archlinux:base-devel bash -c '
       useradd -m b && chown -R b /pkg &&
       sudo -u b bash -c "cd /pkg && makepkg --printsrcinfo > .SRCINFO && makepkg -f --noconfirm --nodeps"
   '
   tar -tf md-viewer-bin-0.1.7-1-x86_64.pkg.tar.zst | grep -E '(bin/md-viewer|applications|pixmaps|licenses)'
   ```
   Expect: four target paths present in the package.

2. **Runtime check** — `sudo pacman -U md-viewer-bin-*.pkg.tar.zst`, then `md-viewer README.md`. `sudo pacman -R md-viewer-bin` to clean up.

3. **namcap** — `namcap md-viewer-bin-0.1.7-1-x86_64.pkg.tar.zst` to surface missing-deps warnings (real validator, since `ldd` is insufficient).

4. **First release tag** — pushing `v0.1.8` (or later) should make `publish-aur-bin` create `ssh://aur@aur.archlinux.org/md-viewer-bin.git` on first push.

## Future Improvements

- Add `md-viewer-bin` to the `aur/PKGBUILD` (`-git`) `conflicts=` array for symmetry. Holds off until the next `-git` PKGBUILD touch to avoid an unrelated edit on this branch.
- Consider bundling `.desktop` / icon / LICENSE into the release tarball if we ever ship a 2nd binary (e.g. aarch64-linux) — at that point the PKGBUILD source-count balloon makes the per-tarball-aux-file approach uncomfortable.
