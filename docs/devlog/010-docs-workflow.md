# Feature: Documentation Workflow Improvements

**Status:** ✅ Complete
**Branch:** `feature/docs-workflow`
**Date:** 2026-01-22
**Lines Changed:** +350 / -0 in `docs/decisions/`, `.claude/commands/`, `cliff.toml`

## Summary

Added three documentation infrastructure components based on analysis of the "definitive solo-developer documentation stack" guide:

1. **Architecture Decision Records (ADRs)** - MADR-format decision documentation
2. **git-cliff changelog automation** - Conventional commit changelog generation
3. **Claude Code slash commands** - Workflow automation

## Features

- [x] `cliff.toml` for automatic changelog generation
- [x] `docs/decisions/` directory with MADR template
- [x] ADR-0000: Record architecture decisions (meta-ADR)
- [x] ADR-0001: Vendor egui_commonmark
- [x] ADR-0002: Bare repo worktree workflow
- [x] ADR-0003: Custom tab system
- [x] `/new-devlog` slash command
- [x] `/session-start` slash command
- [x] `/changelog` slash command

## Key Discoveries

### ADRs vs Devlogs - Clear Distinction

After analyzing the existing documentation, the key insight is:

| Document Type | Purpose | When to Use |
|---------------|---------|-------------|
| **Devlog** | What was implemented, API discoveries | Every feature/fix |
| **ADR** | Why one approach was chosen over alternatives | Major decisions with options |
| **LESSONS.md** | Tactical fixes, gotchas, patterns | Reusable knowledge |

Devlogs already capture "what" excellently. ADRs fill the gap of capturing "why this approach vs alternatives."

### What Wasn't Added (and Why)

From the guide, these were evaluated but not added:

- **JOURNAL.md** - Devlogs serve this purpose better for feature-centric work
- **MCP Mermaid** - ARCHITECTURE.md works well as text; diagrams add complexity without proportional benefit
- **Repomix** - ~2000 line main.rs + good docs means context is already accessible

## Architecture

### New Directory Structure

```
docs/decisions/
├── adr-template.md
├── 0000-record-architecture-decisions.md
├── 0001-vendor-egui-commonmark.md
├── 0002-bare-repo-worktree-workflow.md
└── 0003-custom-tab-system.md

.claude/commands/
├── new-devlog.md
├── session-start.md
└── changelog.md

cliff.toml  (root)
```

### Slash Command Purposes

| Command | Purpose |
|---------|---------|
| `/new-devlog <name>` | Create numbered devlog from template |
| `/session-start` | Verify fresh base, show recent changes |
| `/changelog` | Generate CHANGELOG.md via git-cliff |

## Testing Notes

- Verified cliff.toml syntax is valid TOML
- Slash commands follow `.claude/commands/` format with proper frontmatter
- ADRs follow MADR 4.0 structure

## Future Improvements

- [ ] Add more ADRs as significant decisions arise
- [ ] Consider `/new-adr` slash command if ADR creation becomes frequent
- [ ] Periodically review if upstream egui_commonmark changes warrant sync (ADR-0001)
