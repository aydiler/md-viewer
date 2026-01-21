# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with this repository.

## Project Structure

This is a **bare repository** setup. The git database lives in `.bare/` and all working copies are in `worktrees/`.

```
~/markdown-viewer/
├── .bare/                      # Bare git repository (don't edit directly)
├── .claude -> worktrees/main/.claude  # Symlink to tracked config
├── worktrees/
│   └── main/
│       ├── .claude/            # Claude Code config (tracked in git)
│       │   ├── settings.json
│       │   ├── hooks/          # Branch protection hook
│       │   └── rules/          # Auto-loaded instructions
│       ├── CLAUDE.md           # This file
│       └── docs/               # Evolving docs (versioned with code)
│           ├── ARCHITECTURE.md
│           ├── KEYBOARD_SHORTCUTS.md
│           ├── TARGET_METRICS.md
│           ├── DEVELOPMENT_PLAN.md
│           ├── LESSONS.md
│           └── devlog/
└── CLAUDE.md -> worktrees/main/CLAUDE.md  # Symlink
```

## Documentation

**Auto-loaded** (from `.claude/rules/`):
- `build-commands.md` - cargo build, run, clippy, make install
- `devlog-workflow.md` - How to document feature implementations
- `worktree-workflow.md` - How to create and manage worktrees
- `system-dependencies.md` - Arch Linux packages

**Imported** (via `@path`):
- @docs/ARCHITECTURE.md - Core components, libraries, rendering flow
- @docs/KEYBOARD_SHORTCUTS.md - All keyboard shortcuts
- @docs/TARGET_METRICS.md - Performance targets and planned features
- @docs/LESSONS.md - **Check before debugging** - reusable fixes and gotchas

## Quick Reference

```bash
# Build
cargo build
cargo run -- file.md -w    # Run with file watching

# Worktrees
git -C ~/markdown-viewer/.bare worktree list

git -C ~/markdown-viewer/.bare worktree add \
    ~/markdown-viewer/worktrees/<name> -b feature/<name>

git -C ~/markdown-viewer/.bare worktree remove \
    ~/markdown-viewer/worktrees/<name>
```

## Branch Protection

A Claude Code hook prevents editing files on `main` branch. Create a feature worktree for all new work.
