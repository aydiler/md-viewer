#!/usr/bin/env bash
set -euo pipefail

# Publish vendored fork crates (in dep order) then md-viewer to crates.io.
#
# Idempotent: if a version is already on the registry, cargo emits "already
# uploaded" — we treat that as success so re-tagging the same release doesn't
# fail the job.
#
# Invoked from .github/workflows/release.yml `publish-crates` job. Requires
# CARGO_REGISTRY_TOKEN in the environment.

if [ -z "${CARGO_REGISTRY_TOKEN:-}" ]; then
    echo "error: CARGO_REGISTRY_TOKEN not set" >&2
    exit 1
fi

publish_dir() {
    local dir="$1"
    local label
    if [ "$dir" = "." ]; then
        label="md-viewer"
    else
        label=$(basename "$dir")
    fi

    echo ""
    echo "::group::Publishing ${label}"

    local logfile
    logfile=$(mktemp)
    local rc=0
    (cd "$dir" && cargo publish --token "$CARGO_REGISTRY_TOKEN" 2>&1) | tee "$logfile" || rc=$?

    if [ $rc -eq 0 ]; then
        echo "  Published ${label} OK"
        # Sparse-index catch-up — give dependents enough time to resolve the new version.
        echo "  Sleeping 45s for crates.io sparse-index propagation..."
        sleep 45
    elif grep -qE "already (uploaded|exists on crates.io)" "$logfile"; then
        echo "  ${label}: version already on crates.io; skipping (idempotent)."
    else
        echo "::error::cargo publish failed for ${label}"
        rm -f "$logfile"
        echo "::endgroup::"
        exit 1
    fi

    rm -f "$logfile"
    echo "::endgroup::"
}

publish_dir crates/egui_commonmark/egui_commonmark_backend
publish_dir crates/egui_commonmark/egui_commonmark_macros
publish_dir crates/egui_commonmark/egui_commonmark
publish_dir .
