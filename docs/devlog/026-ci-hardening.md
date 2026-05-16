# Feature: CI Pipeline Hardening

**Status:** 🚧 In Progress
**Branch:** `feature/ci-hardening`
**Date:** 2026-05-16
**Lines Changed:** TBD

## Summary

Two recent release runs failed (v0.1.8: aur-bin race; v0.1.9: cargo publish
dirty-check). Both root causes were patched, but the audit surfaced more
fragility. This change improves robustness across release.yml and ci.yml:

- T1.1: Drop `--token` flag from `cargo publish` (deprecation + secret-in-argv leak)
- T1.2: New `validate` job — fail-fast if tag ↔ Cargo.toml ↔ snap versions disagree
- T1.3: Fix MCP-strip regex so it ignores commented `# mcp = ...` lines (was the
  proximate cause of the v0.1.9 cargo-publish dirty-check failure)
- T2.1: `timeout-minutes` on every job (no more 6-hour hangs)
- T2.2: `publish-snap` secret gate (graceful skip if SNAPCRAFT_STORE_CREDENTIALS missing)
- T2.3: `create-release` no longer waits on `publish-snap` (GH release ~12 min faster)

## Features

- [ ] T1.1 — drop --token flag
- [ ] T1.2 — validate job (tag ↔ Cargo.toml ↔ snap)
- [ ] T1.3 — anchor MCP-strip regex
- [ ] T2.1 — timeout-minutes on every job
- [ ] T2.2 — snap secret gate
- [ ] T2.3 — decouple create-release from publish-snap
- [ ] LESSONS.md entry for MCP-strip regex

## Key Discoveries

### `--token` flag leaks to argv AND is deprecated since cargo 1.75

`cargo publish --token "$X"` puts the token in argv → visible in `ps`, CI step
logs (cargo doesn't redact), shell history if reused. Cargo emits a deprecation
warning suggesting env var. The env var IS already set by the workflow step
(`env: CARGO_REGISTRY_TOKEN: ...`); cargo picks it up automatically. Dropping
`--token` is strict improvement.

### Plain `str.replace` matches inside comments

The old MCP-strip:
```python
t = t.replace('mcp = ["dep:egui-mcp-bridge"]', 'mcp = []')
```
also rewrites `# mcp = ["dep:egui-mcp-bridge"]` → `# mcp = []` because the
target string is a substring of the commented line. Caused v0.1.9 publish
failure (dirty-check). Anchor the regex at line start to skip comments:
```python
t = re.sub(r'(?m)^mcp\s*=\s*\["dep:egui-mcp-bridge"\]', 'mcp = []', t)
```

### GitHub jobs default to 6-hour timeout

A hung Xvfb-style snap LXD VM or stalled action would burn 6 hours of compute
before erroring. Per-job `timeout-minutes` surfaces hangs quickly. Caps sized
~2× observed runtime.

## Architecture

### CI workflow structure after changes

```
push v* tag
   ↓
┌─ validate ──────┐  (new; ~30s; fails fast on version mismatch)
│   tag vs Cargo  │
│   vs snap       │
└─────────────────┘
   ↓ needs: validate
build (matrix)        ~12 min, timeout 30
   ↓ needs: [validate, build]
├─ publish-crates    timeout 20  (gated on CARGO_REGISTRY_TOKEN)
├─ publish-snap      timeout 30  (gated on SNAPCRAFT_STORE_CREDENTIALS — new gate)
├─ publish-aur       timeout 10  (gated on AUR_SSH_PRIVATE_KEY)
├─ publish-aur-bin   timeout 10  (gated on AUR_SSH_PRIVATE_KEY)
└─ create-release    timeout 5   (now needs: [validate, build] — no longer waits for snap)
```

### Files changed

| File | Tier | Change |
|------|------|--------|
| `scripts/publish-crates.sh` | T1.1 | drop `--token` flag |
| `.github/workflows/release.yml` | T1.2 | new validate job + needs: cascade |
| `.github/workflows/release.yml` | T1.3 | anchor MCP-strip regex (2 copies) |
| `.github/workflows/release.yml` | T2.1 | timeout-minutes per job |
| `.github/workflows/release.yml` | T2.2 | snap secret gate (step-level) |
| `.github/workflows/release.yml` | T2.3 | create-release `needs:` change |
| `.github/workflows/ci.yml` | T2.1 | timeout-minutes per job |
| `docs/LESSONS.md` | — | new entry for MCP-strip regex |

## Testing Notes

Local verification (before pushing):
- Run new MCP-strip Python locally on current Cargo.toml → expect 0 diff (comment line preserved).
- Uncomment the dep manually, re-run → expect both lines stripped.
- `cargo check` to confirm no regressions.

End-to-end verification (after merging, before next release tag): no easy local
exercise of the validate job since it triggers on tag push only. Trust the
awk/sed extraction (lifted verbatim from already-working ci.yml:version-sync).
First real release after this PR (v0.1.10) is the actual smoke test.

## Future Improvements

- [ ] Pin actions to commit SHAs once Dependabot is set up
- [ ] Bump Node 20 actions to Node 24 by 2026-09 (currently issues warnings)
- [ ] Release health summary in GH release body (aggregate channel status)
