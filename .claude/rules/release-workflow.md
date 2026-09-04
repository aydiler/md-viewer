# Release Workflow

## The CHANGELOG is hand-written — never regenerate it

**Do not run `git-cliff -o CHANGELOG.md`.** It parses conventional commits, this
repository's history does not conform, and the generated file is sparse enough
that entire versions disappear (v0.1.4, v0.1.6 and v0.1.7 vanished on the
v0.1.8 attempt). Running it with `-o` overwrites the existing hand-written
prose with that degraded version.

If you want a starting point, `git-cliff --tag vX.Y.Z` prints to stdout and can
be read for inspiration. The entry itself is written by hand, in the style of
the existing ones: what changed, why, and the PR reference.

Recovery if it was run anyway: `git checkout CHANGELOG.md`.

See `docs/LESSONS.md`, "CHANGELOG.md is hand-curated".

## Before tagging

1. **Everything intended for the release is merged to `main`,** and `main` is
   in sync with `origin/main`.

2. **Bump the version in `Cargo.toml` and `Cargo.lock`.**

   Edit the lockfile *surgically* — scope the change to the `md-viewer` block:

   ```
   name = "md-viewer"
   version = "0.1.7"      ->      version = "0.1.8"
   ```

   Never `sed` it. `sed -i 's/^version = "0.1.7"$/.../' Cargo.lock` rewrote five
   lines on the v0.1.8 attempt — four unrelated crates happened to sit at the
   same version and were silently given invalid ones. `cargo update -p md-viewer
   --precise X.Y.Z` from a clean tree is the unambiguous alternative.

   Always `git diff Cargo.lock` afterwards. Anything beyond the one line under
   `[[package]] name = "md-viewer"` is a mistake.

3. **If the vendored renderer changed, bump its workspace version too**
   (`crates/egui_commonmark/Cargo.toml`, `[workspace.package] version`). All
   three fork crates share it.

4. **Prepend a `## [X.Y.Z] - YYYY-MM-DD` section to `CHANGELOG.md`,** by hand.

5. **Commit, then tag:** `git tag vX.Y.Z && git push origin vX.Y.Z`.

## What the tag sets off

Pushing the tag runs `.github/workflows/release.yml`, which publishes to
**four** places. It is not reversible by deleting the tag.

- **crates.io** — the `publish-crates` job runs `scripts/publish-crates.sh`,
  which publishes in dependency order with a 45 s sparse-index pause between
  steps: `egui_commonmark_backend_extended` → `..._macros_extended` →
  `..._extended` → `md-viewer`. Re-tagging is safe: "already uploaded" counts
  as success.

  The fork crates therefore do **not** need publishing by hand first. A local
  `cargo package -p md-viewer` *will* fail while the fork's new version is not
  yet on crates.io — that is expected and not a release blocker, because the
  pipeline publishes them earlier in the same run.

- **Snap Store** — built in CI with LXD. Never `snapcraft --destructive-mode`
  from this machine: its glibc is newer than the declared `base:`, and the
  resulting snap fails to start on the target distro (issue #3). If CI fails,
  fix CI.

- **AUR**, twice — `md-viewer` (source) and `md-viewer-bin`.

- **GitHub Release** with the platform tarballs.

## Verification worth doing before tagging

- `cargo test` at the root, and the renderer crates through their own manifest:

  ```bash
  RENDERER_FEATURES=better_syntax_highlighting,svg,svg_text,load-images,fetch,mermaid,math,macros
  cargo test --manifest-path crates/egui_commonmark/Cargo.toml \
      -p egui_commonmark_extended --features "$RENDERER_FEATURES"
  ```

  The two are separate workspaces — a plain `cargo test` at the root covers
  only half the code, silently. (Discussed in issue #122.)

- `scripts/scroll-regression.sh` and `scripts/visual-regression.sh`. Neither
  runs in CI — they need Xvfb, xdotool and ImageMagick — and between them they
  have caught several rendering defects that all three CI jobs passed.
