#!/usr/bin/env bash
#
# Guard against shipping a stale crates.io build of md-viewer.
#
# md-viewer vendors a fork of egui_commonmark under crates/egui_commonmark/ and
# uses it locally via [patch.crates-io] in the root Cargo.toml. Every *source*
# build (snap, AUR, the GitHub release binaries, local dev) therefore uses the
# local fork regardless of version — but `cargo publish` ignores the patch and
# resolves the fork from crates.io at the pinned version. crates.io is immutable,
# so if the fork's source changes but its version is NOT bumped, the release
# silently skips re-publishing the fork and `cargo install md-viewer` builds
# against the OLD fork code.
#
# This script fails the release when the local fork source differs from what is
# already published on crates.io at the current fork version.
#
# To fix a failure: bump the workspace version in
#   crates/egui_commonmark/Cargo.toml
# and the matching pin in the root Cargo.toml
#   egui_commonmark_extended = { version = "X.Y.Z", ... }
# then re-run. (The publish-crates job will then upload the new fork version
# instead of skipping it.)
#
set -euo pipefail

UA="md-viewer-fork-publish-check (+https://github.com/aydiler/md-viewer)"
FORK_DIR="crates/egui_commonmark"
VERSION="$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' "$FORK_DIR/Cargo.toml")"
echo "Vendored fork workspace version: $VERSION"

# published crate name : local crate directory
CRATES=(
  "egui_commonmark_backend_extended:$FORK_DIR/egui_commonmark_backend"
  "egui_commonmark_extended:$FORK_DIR/egui_commonmark"
  "egui_commonmark_macros_extended:$FORK_DIR/egui_commonmark_macros"
)

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
stale=0

for entry in "${CRATES[@]}"; do
  crate="${entry%%:*}"
  dir="${entry#*:}"

  # Is this version published? If the download 404s, it's a fresh version → fine.
  if ! curl -fsSL -A "$UA" \
        "https://crates.io/api/v1/crates/$crate/$VERSION/download" \
        -o "$tmp/c.crate" 2>/dev/null; then
    echo "  $crate@$VERSION: not on crates.io yet — will publish fresh. OK"
    continue
  fi

  rm -rf "$tmp/x"
  mkdir -p "$tmp/x"
  tar xzf "$tmp/c.crate" -C "$tmp/x"
  pub="$tmp/x/$crate-$VERSION"

  # Compare every Rust source file. cargo packages .rs verbatim, so a content
  # difference means crates.io would serve different code than this checkout.
  diffs=""
  while IFS= read -r rel; do
    if [ ! -f "$dir/$rel" ] || ! diff -q "$pub/$rel" "$dir/$rel" >/dev/null 2>&1; then
      diffs="$diffs $rel"
    fi
  done < <(cd "$pub" && find . -name '*.rs' | sed 's|^\./||' | sort)

  # Newly-added source files under src/ that the published crate doesn't have
  # (a new module without a version bump would make `cargo install` fail to
  # compile or build stale). examples/ and tests/ are not packaged, so ignore
  # local-only files outside src/.
  if [ -d "$dir/src" ]; then
    while IFS= read -r rel; do
      [ -f "$pub/$rel" ] || diffs="$diffs $rel(new)"
    done < <(cd "$dir" && find src -name '*.rs' | sed 's|^\./||' | sort)
  fi

  if [ -n "$diffs" ]; then
    echo "  $crate@$VERSION: STALE — local differs from published:$diffs"
    stale=1
  else
    echo "  $crate@$VERSION: matches published. OK"
  fi
done

if [ "$stale" -eq 1 ]; then
  echo "::error::Vendored egui_commonmark fork changed but version $VERSION is already on crates.io. Bump crates/egui_commonmark/Cargo.toml's version and the root Cargo.toml pin, or 'cargo install md-viewer' will ship stale fork code (source builds — snap/AUR/binaries — are unaffected)."
  exit 1
fi

echo "Vendored fork is publishable — no stale crates.io mismatch."
