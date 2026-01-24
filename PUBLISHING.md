# Publishing Guide

Steps to publish md-viewer to various package registries.

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

```bash
# Login (one-time)
cargo login

# Publish (run from repo root)
cargo publish
```

**Note**: The vendored `egui_commonmark` fork uses a local path. For crates.io publishing, you may need to either:
- Publish your fork to crates.io first
- Or patch the dependency in Cargo.toml to use a git URL

## AUR (Arch User Repository)

1. **Create AUR account**: https://aur.archlinux.org/register

2. **Set up SSH key**: https://aur.archlinux.org/account/YOUR_USERNAME/edit (SSH keys tab)

3. **Clone and push**:
```bash
git clone ssh://aur@aur.archlinux.org/md-viewer-git.git
cd md-viewer-git
cp /path/to/aur/PKGBUILD .
makepkg --printsrcinfo > .SRCINFO
git add PKGBUILD .SRCINFO
git commit -m "Initial upload: md-viewer-git 0.1.0"
git push
```

4. **Test the package**:
```bash
makepkg -si
```

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

## Release Checklist

- [ ] Update version in `Cargo.toml`
- [ ] Generate changelog: `git-cliff -o CHANGELOG.md`
- [ ] Commit version bump
- [ ] Create git tag: `git tag v0.1.0`
- [ ] Push tag: `git push origin v0.1.0`
- [ ] Publish to crates.io: `cargo publish`
- [ ] Update AUR PKGBUILD and push
- [ ] Build and upload Snap
- [ ] Create GitHub Release with changelog
