# Feature: Distribution Improvements

**Status:** 🚧 In Progress
**Branch:** `feature/distribution`
**Date:** 2026-05-15
**Lines Changed:** TBD

## Summary

Broadens md-viewer's distribution beyond Linux-x86_64-and-build-from-source. Adds:

1. Cross-platform prebuilt binaries (Linux, macOS Intel, macOS Apple Silicon, Windows) via a release matrix in `.github/workflows/release.yml`.
2. A `scripts/install.sh` one-liner installer so Linux/macOS users skip the 2–3 minute Cargo compile and just grab the prebuilt tarball.
3. SHA256 checksums for every release artifact, so the installer can verify downloads and downstream packagers can pin checksums.
4. Automated AUR `md-viewer-git` PKGBUILD + .SRCINFO push from CI (gated on `AUR_SSH_PRIVATE_KEY` secret).
5. Flatpak/Flathub manifest + AppStream MetaInfo + placeholder icon, ready for a Flathub PR once a designed icon replaces the placeholder.

Motivated by the audit at `~/.claude/plans/check-how-installing-of-polymorphic-falcon.md`: today only Snap users auto-update, Cargo/AUR/source paths all compile locally, and there's no macOS/Windows path at all.

## Features

- [ ] Fix stale `pkgver=0.1.0` in `aur/PKGBUILD`
- [ ] Cross-platform build matrix in `release.yml` (ubuntu-latest, macos-13, macos-14, windows-latest)
- [ ] Generate `.sha256` alongside each artifact
- [ ] `scripts/install.sh` — POSIX sh, detects platform, downloads latest tagged release, verifies checksum, installs to `~/.local/bin`
- [ ] `publish-aur` CI job (gated `if: secrets.AUR_SSH_PRIVATE_KEY != ''`)
- [ ] `flatpak/io.github.aydiler.md-viewer.yaml` manifest
- [ ] `flatpak/cargo-sources.json` (generated via `flatpak-cargo-generator.py`)
- [ ] `data/io.github.aydiler.md-viewer.metainfo.xml` (AppStream)
- [ ] `data/io.github.aydiler.md-viewer.png` (placeholder, 256x256)
- [ ] README install section reordering + macOS Gatekeeper note
- [ ] `PUBLISHING.md` AUR-setup + Flathub-submission + icon-replacement checklists

## Key Discoveries

### AUR md-viewer-git already exists

Reconnaissance via the AUR RPC showed the package is already live under maintainer `aydiler` at version `0.1.2.r0.g2b51270-1`. The PKGBUILD's `pkgver()` function correctly derives the version from `git describe`, so the literal `pkgver=0.1.0` in the file has been harmlessly stale. The CI automation only needs to push updates of the PKGBUILD itself (e.g., new deps) plus the regenerated `.SRCINFO`.

### `ci.yml` already has a version-sync job

The earlier plan called for adding a CI step that fails if `Cargo.toml` and `snap/snapcraft.yaml` versions disagree. That job already exists. Skipped.

### Icon is a placeholder

No real icon existed anywhere — the `.desktop` file references `Icon=text-markdown` (a generic system icon). Flathub requires a real custom icon. Generated a placeholder PNG so the Flatpak manifest is complete and the build path works end-to-end. **The placeholder must be replaced with a designed icon before the Flathub PR.**

### Cross-platform `sed` portability

The existing MCP-strip step uses GNU `sed -i` syntax. macOS BSD `sed` requires `sed -i ''`. Standardizing on a tiny `python3 -c '...'` script avoids the difference, since `python3` is preinstalled on all GitHub runners (Linux, macOS, Windows).

### `makepkg` refuses to run as root

The AUR `publish-aur` job uses an `archlinux/archlinux:base-devel` container to generate `.SRCINFO`. `makepkg` refuses to run as root, so the container must create an unprivileged user and run `makepkg --printsrcinfo` under it.

### Flatpak needs offline Cargo sources

Flathub builds in a sandbox without network access. The Rust source crate set must be pre-resolved into `cargo-sources.json` via `flatpak-cargo-generator.py` from the `flatpak-builder-tools` repo. Regenerate whenever `Cargo.lock` changes.

## Architecture

No code changes to `src/`. This is purely a release/packaging change.

### New files

| Path | Purpose |
|------|---------|
| `scripts/install.sh` | One-line installer (Linux/macOS) |
| `flatpak/io.github.aydiler.md-viewer.yaml` | Flatpak manifest |
| `flatpak/cargo-sources.json` | Pre-resolved Cargo sources (generated) |
| `data/io.github.aydiler.md-viewer.metainfo.xml` | AppStream metainfo |
| `data/io.github.aydiler.md-viewer.png` | Placeholder app icon |

### Modified files

| Path | Change |
|------|--------|
| `.github/workflows/release.yml` | Matrix build, SHA256, publish-aur job |
| `aur/PKGBUILD` | Bump literal `pkgver` line |
| `README.md` | Reorder install section, add macOS note |
| `PUBLISHING.md` | AUR setup, Flathub checklist, icon swap reminder |

## Testing Notes

- Local `cargo build --release` should still succeed (no Cargo.toml changes).
- `bash scripts/install.sh --help` should print usage; running with `INSTALL_DIR=/tmp/mv-test` should drop a working binary into that path.
- `flatpak-builder --user --install --force-clean build-dir flatpak/io.github.aydiler.md-viewer.yaml` should build successfully; `flatpak run io.github.aydiler.md-viewer README.md` should open the app (placeholder icon visible).
- Real end-to-end test: tag `v0.1.3-rc1` on this branch, watch `release.yml` produce 4 binaries × (tarball/zip + .sha256), confirm `publish-aur` skips cleanly without the secret set.

## Future Improvements

- [ ] Apple Developer ID + notarization to remove macOS Gatekeeper quarantine friction.
- [ ] Windows code signing (paid certificate).
- [ ] Linux ARM (`aarch64`) prebuilt binary, either via a self-hosted runner or `cross`.
- [ ] In-app update check (the audit's item #1) — startup ping to GitHub Releases API with a 24h cache, banner if newer.
- [ ] Auto-bump `<releases>` in MetaInfo XML from `git-cliff` output during the release workflow.
- [ ] CI check that `flatpak/cargo-sources.json` matches `Cargo.lock` (regenerate-on-mismatch failure).
