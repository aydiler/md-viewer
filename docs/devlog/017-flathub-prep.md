# Feature: Flathub Submission Prep

**Status:** 🚧 In Progress
**Branch:** `feature/flathub-prep`
**Date:** 2026-05-15
**Lines Changed:** TBD

## Summary

Brings the Flatpak manifest from "exists" to "ready for Flathub PR review" by addressing items flagged in the post-v0.1.3 audit against Flathub's published rules (https://docs.flathub.org/docs/for-app-authors/requirements + linter rules). The placeholder icon and the Flathub PR itself stay manual user actions; everything else automatable is done here.

The original manifest in devlog 016 was a first-pass scaffold — runtime was 23.08 (newer cycle exists), sandbox permissions were too broad for a viewer (`--filesystem=home`), and network access was missing (breaks remote-image rendering documented in README).

## Features

- [x] Bump `runtime-version` from 23.08 → 24.08 (latest stable freedesktop cycle on Flathub at time of writing)
- [x] Replace `--filesystem=home` with `--filesystem=host:ro` (read-only, covers CLI args + live reload + ad-hoc files)
- [x] Add `--share=network` so HTTP image URLs in markdown render in the sandbox
- [x] Bump `sources.git.tag` to `v0.1.3` and add `commit:` SHA pin (Flathub linter recommendation)
- [x] Add `x-checker-data` block keyed off Git tags for automatic version detection on Flathub
- [x] Update `PUBLISHING.md` Flathub section with lint command, icon-replacement reminder, and tag/commit bump procedure
- [ ] Replace placeholder icon at `data/io.github.aydiler.md-viewer.png` — **user task**, can't be automated
- [ ] Run `flatpak-builder-lint` locally before submission — **requires `sudo pacman -S flatpak-builder`**, user must opt in
- [ ] Open Flathub PR — **user task** (fork flathub/flathub, push manifest + cargo-sources.json, open PR against `new-pr` branch)

## Key Discoveries

### `host:ro` vs `home` is a different shape of broadness, not strictly narrower

`--filesystem=home` is rw access to `$HOME`. `--filesystem=host:ro` is read-only access to the *entire* host filesystem. For a markdown viewer:

- Read-only fits the use case (no writes to user files)
- Covers CLI invocation `md-viewer ~/notes/foo.md` (path could be anywhere)
- Covers live reload (file watcher needs read access to the watched path)
- Covers `xdg-open` style file pickers that may return paths outside `xdg-documents`

Trade-off: `host:ro` exposes `/etc`, `/usr`, etc. to read access — broader *scope* but no write *capability*. Flathub reviewers may push back; defense is "live reload requires open read access to the file path the user picks, and viewers don't mutate user files." Precedent: several markdown editors on Flathub use `--filesystem=host` or `host:ro`.

### `cargo-sources.json` was NOT regenerated for v0.1.3

The version bump 0.1.2 → 0.1.3 only changed `md-viewer`'s own version in `Cargo.lock`. The transitive dependency tree is identical, and `cargo-sources.json` lists only transitive crates (md-viewer itself isn't in there — Flathub fetches it via the git source). No regeneration needed.

If Cargo.lock changes transitive deps in a future release, run:

```bash
git clone https://github.com/flatpak/flatpak-builder-tools.git /tmp/fbt
uv run --with aiohttp --with PyYAML --with tomlkit \
    /tmp/fbt/cargo/flatpak-cargo-generator.py Cargo.lock \
    -o flatpak/cargo-sources.json
```

### `x-checker-data` only helps *after* the Flathub PR is accepted

The `x-checker-data` block tells Flathub's update bot to watch for new git tags matching `^v([\d.]+)$`. It does nothing during initial submission — Flathub maintainers still review the first PR manually. After acceptance, future tag pushes auto-create update PRs to the Flathub repo.

## Architecture

### Modified files

| Path | Change |
|------|--------|
| `flatpak/io.github.aydiler.md-viewer.yaml` | Runtime bump, finish-args tightened, source pinned to v0.1.3 + SHA, x-checker-data added |
| `PUBLISHING.md` | Three new bullets in the Flathub submission section |

### No code changes

This is a packaging/metadata change only. No `src/` modifications. No Cargo.toml changes.

## Testing Notes

**Required (done):**
- `python3 -c "import yaml; yaml.safe_load(open('flatpak/io.github.aydiler.md-viewer.yaml'))"` — parses
- `appstreamcli validate data/io.github.aydiler.md-viewer.metainfo.xml` — passes (unchanged)

**Optional (requires user to install flatpak-builder via pacman):**
- `flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest flatpak/io.github.aydiler.md-viewer.yaml` — should be clean (no error-level findings)
- `flatpak-builder --user --install --force-clean build-dir flatpak/io.github.aydiler.md-viewer.yaml` — should build the app
- `flatpak run io.github.aydiler.md-viewer README.md` — should open with the placeholder icon visible in the taskbar

## Future Improvements

- [ ] Replace the placeholder icon with a designed PNG/SVG
- [ ] Once the Flathub PR is accepted, the `x-checker-data` block will auto-bump tag references on each new release — verify it actually fires on the next tag push
- [ ] Consider tightening `--filesystem=host:ro` to `--filesystem=xdg-documents:ro --filesystem=xdg-download:ro` if Flathub reviewers push back — would break CLI invocation from arbitrary paths and live-reload of files outside those directories
- [ ] Drop the `cargo-sources.json` regeneration step from the maintainer flow once we have a CI check that compares the file's hash against a re-generation from Cargo.lock
