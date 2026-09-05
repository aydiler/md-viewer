#!/usr/bin/env bash
# Extract one version's section from CHANGELOG.md for the GitHub Release body.
#
# The release body used to come from `git-cliff --latest`, which parses
# conventional commits. This history does not conform, so v0.1.17's release page
# carried eleven lines — two bullets — while its CHANGELOG entry was a page of
# prose. The Release page is the only changelog most users ever see: snap and
# AUR users never open the repository.
#
# Falls back to git-cliff when the version has no section, so a tag that skips
# the changelog still produces something rather than an empty release.
set -euo pipefail
VERSION="${1:?usage: release-notes.sh <version-without-v>}"
CHANGELOG="${2:-CHANGELOG.md}"

if [ -f "$CHANGELOG" ] && grep -q "^## \[${VERSION}\]" "$CHANGELOG"; then
    awk -v v="^## \\\\[${VERSION}\\\\]" '
        $0 ~ v      { inside = 1; next }
        inside && /^## \[/ { exit }
        inside      { print }
    ' "$CHANGELOG" | sed -e '/./,$!d' | awk 'BEGIN{RS="";ORS="\n\n"}1'
else
    echo "::warning::no CHANGELOG section for ${VERSION}; falling back to git-cliff" >&2
    git-cliff --latest --strip header
fi
