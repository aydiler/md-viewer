# Feature: Restore crates.io auto-publish

**Status:** 🚧 In Progress
**Branch:** `feature/crates-publish`
**Date:** 2026-05-16
**Lines Changed:** TBD

## Summary

`publish-crates` job was removed from `.github/workflows/release.yml` in PR #11
because `cargo publish` for md-viewer failed:

```
package `md-viewer` depends on `egui_commonmark_extended` with feature `math`
but `egui_commonmark_extended` does not have that feature
```

Re-enable crates.io publish by:

1. Bump vendored fork workspace 0.22.2 → 0.23.0 (`math` feature added since the
   last fork publish).
2. Update root `Cargo.toml` dep to `0.23.0`.
3. Add `publish-crates` job to `release.yml` mirroring the step-level
   secret-gating pattern from `publish-aur`.
4. Add `scripts/publish-crates.sh` with idempotent publish loop in dep order +
   sparse-index settle delay.

## Features

- [ ] Bump fork workspace + inter-deps to 0.23.0
- [ ] Update root Cargo.toml `egui_commonmark_extended` dep to 0.23.0
- [ ] Add `scripts/publish-crates.sh`
- [ ] Add `publish-crates` job to `.github/workflows/release.yml`
- [ ] Rewrite Crates.io section of `PUBLISHING.md`
- [ ] Rewrite "cargo publish rejects vendored forks" lesson in `LESSONS.md`
- [ ] Local pre-flight (cargo check + dry-runs)

## Key Discoveries

### Fork crates already on crates.io — under renamed identifiers

The three fork crates (`egui_commonmark_extended`,
`egui_commonmark_backend_extended`, `egui_commonmark_macros_extended`) were
published at v0.22.2 on 2026-03-04 by `aydiler` (same day md-viewer v0.1.2
shipped). The renamed `_extended` identifiers mean no upstream conflict.

The blocker isn't "publish under a new name" (LESSONS.md / memory's
recommendation). It's "republish with feature parity": v0.22.2 on the registry
lacks `math` (the feature was added to the local fork *after* that publish), so
`cargo publish` for md-viewer fails feature-resolution.

Verified via crates.io API:

```
$ curl -s https://crates.io/api/v1/crates/egui_commonmark_extended/0.22.2 \
   | jq '.version.features.math'
null
```

### `[patch.crates-io]` is safe to keep

`cargo publish` ignores `[patch.crates-io]` during its verify step (resolves
deps against the registry directly). Once the registry version matches what
the root Cargo.toml asks for, the patch becomes neutral — useful for local dev
between fork bumps, harmless for publish.

### Sparse-index propagation needs a settle delay

After `cargo publish` for crate A, dependents publishing immediately may fail
with "not in index". Add `sleep 45` between publishes. If still flaky under
crates.io load, bump to 90s.

## Architecture

### New file: `scripts/publish-crates.sh`

Iterates over the publish dep order (backend → macros → extended → md-viewer).
Catches "already uploaded" from cargo stderr → treats as success (idempotent
on re-tags). Otherwise propagates failure.

### CI: new `publish-crates` job

Mirrors the secret-gating pattern at `release.yml:137-145` (LESSONS.md →
"GitHub Actions blocks `secrets.*` AND `env.*` in job-level `if:`"). When
`CARGO_REGISTRY_TOKEN` is unset, all steps no-op and the job stays green with
a `::notice::`. Same Linux apt deps as the build job — required for md-viewer's
verify step that links against eframe/rfd/etc.

Reuses the build job's "Remove local-only MCP dependency" Python transform —
strips the `path = ".../egui-mcp-bridge"` line that can't exist on crates.io.

## Testing Notes

Local pre-flight before tagging:

- `cargo check --all-features` (still uses patch — verifies local builds)
- `cargo publish --dry-run` on each fork crate in turn (verifies metadata +
  version-newness on registry)
- `cargo publish --dry-run` on md-viewer **will fail locally** before the
  fork-at-0.23.0 is published — expected, the failure message should say
  "version 0.23.0 not found" (NOT "feature missing") if feature parity is right

## Future Improvements

- [ ] Bump sleep to 90s if 45s proves flaky under load
- [ ] Eventually: upstream `math` feature to `lampsitter/egui_commonmark` so
      we can drop the fork entirely. Other deviations would also need
      upstreaming (line height, header positions, search highlights, table
      builder, wheel routing) — large effort, deferred.
