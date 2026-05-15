# Publishing Guide

Releases are mostly automated. Tag a commit with `vX.Y.Z` and push the tag — `.github/workflows/release.yml` builds prebuilt binaries for Linux, macOS Intel, macOS arm64, and Windows; publishes to crates.io, the Snap Store, and the AUR; and creates a GitHub Release with checksums.

This document covers the **one-time setup** for each channel and the **per-release checklist** at the bottom.

## GitHub Repository

1. **Add topics** (Settings → About → Topics):
   - `markdown`, `markdown-viewer`, `egui`, `rust`, `linux`, `desktop-app`
   - `syntax-highlighting`, `file-explorer`, `tabs`, `wayland`, `x11`

2. **Add description**:
   > Fast, lightweight markdown viewer for Linux with tabs, file explorer, and live reload

3. **Enable features**:
   - Releases
   - Discussions (optional)

## Crates.io

**Crates.io publishing is intentionally NOT automated** in `release.yml`. `cargo publish` rejects this repo because `Cargo.toml` consumes `egui_commonmark_extended` with a custom `math` feature (added in the vendored fork at `crates/egui_commonmark/`). The upstream `egui_commonmark_extended` on crates.io does not have that feature, so dependency resolution against the registry fails:

```
package `md-viewer` depends on `egui_commonmark_extended` with feature `math`
but `egui_commonmark_extended` does not have that feature
```

To re-enable crates.io publishing, you would need to either:
- Publish the vendored fork to crates.io under a new name (e.g., `egui_commonmark_extended_aydiler`) and update `Cargo.toml` to depend on the renamed crate. Ongoing fork maintenance becomes a separate publishing pipeline.
- Or, upstream the `math` feature into `egui_commonmark_extended` and drop the local patch.

Manual publish (if you do address one of the above):

```bash
cargo login              # one-time
cargo publish            # from repo root
```

## AUR (Arch User Repository)

The AUR `md-viewer-git` package is now published **automatically by CI** on every `v*` tag (see the `publish-aur` job in `.github/workflows/release.yml`).

### One-time setup to enable automation

1. **Create an SSH keypair** dedicated to AUR pushes (no passphrase):
   ```bash
   ssh-keygen -t ed25519 -f aur-key -C "aur-md-viewer" -N ""
   ```
2. **Register the public key** on your AUR account → SSH Public Key:
   https://aur.archlinux.org/account/aydiler/edit
   (paste the contents of `aur-key.pub`).
3. **Add the private key** as a GitHub repo secret named `AUR_SSH_PRIVATE_KEY`
   (Settings → Secrets and variables → Actions → New repository secret).
   Value: entire contents of the `aur-key` file (including BEGIN/END lines).
4. **Delete the local keypair**: `rm aur-key aur-key.pub`.

After this, every `git push origin v0.X.Y` triggers `publish-aur`, which:
- Bumps the literal `pkgver=` in `aur/PKGBUILD` to the tag version.
- Regenerates `.SRCINFO` via an `archlinux/archlinux:base-devel` container.
- Commits and pushes to `ssh://aur@aur.archlinux.org/md-viewer-git.git`.

The job is **gated on the secret existing** (`if: secrets.AUR_SSH_PRIVATE_KEY != ''`), so the rest of the release pipeline still runs if you skip this setup.

### Manual fallback

```bash
git clone ssh://aur@aur.archlinux.org/md-viewer-git.git
cd md-viewer-git
cp /path/to/repo/aur/PKGBUILD .
sed -i "s/^pkgver=.*/pkgver=$NEW_VERSION/" PKGBUILD
makepkg --printsrcinfo > .SRCINFO
git add PKGBUILD .SRCINFO && git commit -m "Update to $NEW_VERSION" && git push
```

Test locally: `makepkg -si`.

## Snap Store

1. **Create Snapcraft account**: https://snapcraft.io/account

2. **Login**:
```bash
snapcraft login
```

3. **Build the snap**:
```bash
cd /path/to/repo
snapcraft
```

4. **Register the name** (one-time):
```bash
snapcraft register md-viewer
```

5. **Upload and release**:
```bash
snapcraft upload --release=stable md-viewer_0.1.0_amd64.snap
```

## Flatpak / Flathub

A Flatpak manifest lives at `flatpak/io.github.aydiler.md-viewer.yaml`. Submitting to Flathub is **manual** (one-time PR to the Flathub org).

### Pre-submission requirements

- **Replace the placeholder icon** at `data/io.github.aydiler.md-viewer.png` with a designed 256×256 PNG (or scalable SVG). Flathub reviewers will reject submissions whose icon is a generic placeholder.
- **Bump `tag:` and `commit:` in the manifest** to the latest release before opening the PR. The `x-checker-data` block will auto-bump them on subsequent releases *after* the first submission is accepted.
- **Regenerate Cargo sources** when `Cargo.lock` changes its transitive deps (Flathub builds offline):
  ```bash
  # one-time tool setup
  git clone https://github.com/flatpak/flatpak-builder-tools.git /tmp/fbt
  uv run --with aiohttp --with PyYAML --with tomlkit \
      /tmp/fbt/cargo/flatpak-cargo-generator.py Cargo.lock \
      -o flatpak/cargo-sources.json
  ```
  Commit the updated `flatpak/cargo-sources.json`. Pure version bumps (without dep changes) don't require regeneration.
- **Run the Flathub linter and fix all error-level findings**:
  ```bash
  # one-time tool install (Arch Linux):
  sudo pacman -S flatpak-builder
  flatpak install --user flathub org.flatpak.Builder

  # then:
  flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest \
      flatpak/io.github.aydiler.md-viewer.yaml
  ```
  Warnings are non-fatal but worth addressing. **One known lint error is intentional**: `finish-args-home-ro-filesystem-access` flags the `--filesystem=home:ro` permission. The viewer needs this to read markdown files at arbitrary paths under `$HOME` for CLI invocation (`md-viewer ~/anywhere/foo.md`) and live reload via the notify watcher. Flathub permits read-only home access with reviewer justification — include a note in the PR description explaining the use case. If reviewers reject it, fall back to `--filesystem=xdg-documents` only and document the CLI/live-reload limitation in the README.

### Local smoke test before submitting

```bash
flatpak install --user flathub \
    org.freedesktop.Platform//24.08 \
    org.freedesktop.Sdk//24.08 \
    org.freedesktop.Sdk.Extension.rust-stable//24.08

flatpak-builder --user --install --force-clean build-dir \
    flatpak/io.github.aydiler.md-viewer.yaml

flatpak run io.github.aydiler.md-viewer README.md
```

### Submission steps

1. Fork https://github.com/flathub/flathub on GitHub.
2. Create a branch `io.github.aydiler.md-viewer` in your fork.
3. Add `io.github.aydiler.md-viewer.yaml` (copied from this repo's `flatpak/`) plus the `cargo-sources.json` to the branch root.
4. Open a PR against `flathub/flathub:new-pr`. Flathub's bot will validate the build.
5. Iterate with reviewers (typical asks: icon variants, screenshot improvements, sandbox tightening).
6. After merge, Flathub builds and serves updates on every `v*` tag if you wire `x-checker-data` into the manifest.

## Icon

The repo currently ships a **placeholder** icon at `data/io.github.aydiler.md-viewer.png` (256×256 "md" on dark blue, generated by ImageMagick). Replace this with a designed icon before opening the Flathub PR. The icon is also referenced by the Flatpak manifest and AppStream metainfo.

## Release Checklist

- [ ] Update version in `Cargo.toml` (`version = "X.Y.Z"`)
- [ ] Update version in `snap/snapcraft.yaml` (`version: 'X.Y.Z'`) — `version-sync` CI check enforces parity
- [ ] Append a `<release>` entry to `data/io.github.aydiler.md-viewer.metainfo.xml`
- [ ] Generate changelog: `git-cliff -o CHANGELOG.md`
- [ ] Commit: `git commit -am "Release X.Y.Z"`
- [ ] Tag: `git tag vX.Y.Z && git push origin main vX.Y.Z`
- [ ] Watch `release.yml`. Expected outputs:
  - 4 prebuilt binaries (Linux x86_64, macOS x86_64, macOS arm64, Windows x86_64) + matching `.sha256`
  - GitHub Release with `RELEASE_NOTES.md` body
  - crates.io publish (skipped if version already there)
  - Snap Store stable channel publish
  - AUR push (gated on `AUR_SSH_PRIVATE_KEY`)
