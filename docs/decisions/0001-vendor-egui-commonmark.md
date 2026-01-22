# ADR-0001: Vendor egui_commonmark Instead of Git Dependency

**Status:** Accepted
**Date:** 2026-01-22
**Deciders:** Ahmet

## Context

The markdown-viewer app needed custom typography support (configurable line height, font sizes) that egui_commonmark doesn't expose. We needed to modify the library to wire up egui's existing `TextFormat.line_height` capability.

## Decision Drivers

- Need to modify internal rendering code
- Want easy debugging and iteration
- Must work reliably across all development machines
- Want clear visibility into our changes vs upstream

## Considered Options

### Option 1: Fork on GitHub + git dependency

Create a GitHub fork and reference it in Cargo.toml:
```toml
egui_commonmark = { git = "https://github.com/user/egui_commonmark", branch = "typography" }
```

**Pros:**
- Standard Rust practice for forks
- Can easily PR changes upstream
- Tracks upstream commits

**Cons:**
- Network dependency for builds
- Branch management overhead
- Harder to see local changes at a glance
- Rebasing upstream changes is manual

### Option 2: Vendor in `crates/` directory

Copy the full crate into `crates/egui_commonmark/` and reference as path dependency:
```toml
egui_commonmark = { path = "crates/egui_commonmark/egui_commonmark" }
```

**Pros:**
- No network dependency
- Immediate debugging with local changes
- Clear diff against upstream (git blame works)
- Simpler builds, works offline
- All code visible in project

**Cons:**
- Manual sync with upstream updates
- Larger repository size
- Must track which changes are ours

### Option 3: Patch in Cargo.toml

Use `[patch]` section to override the crates.io version:
```toml
[patch.crates-io]
egui_commonmark = { path = "crates/egui_commonmark" }
```

**Pros:**
- Other dependencies still see the crate normally
- Can switch back to crates.io easily

**Cons:**
- Same vendoring overhead as Option 2
- More confusing dependency resolution

## Decision

Vendor in `crates/egui_commonmark/` as a path dependency (Option 2).

For a solo developer project with deep modifications, vendoring provides the best iteration speed and debugging experience. The network independence is valuable for development on unreliable connections.

## Consequences

### Positive

- Fast local iteration on typography features
- Offline builds always work
- Easy to trace our modifications via git history
- Full control over update timing

### Negative

- Must manually check upstream for important fixes
- Repository is ~500KB larger
- Must document which files we've modified

## Related

- `crates/egui_commonmark/` - Vendored crate location
- `docs/devlog/002-line-height-investigation.md` - Why we needed typography changes
- `docs/devlog/003-evidence-based-typography.md` - WCAG research driving requirements
- `docs/LESSONS.md` - Section on "Line height not exposed by default"
