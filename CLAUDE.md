# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with this repository.

## Tech Stack

| Component | Technology |
|-----------|------------|
| Language | Rust |
| GUI Framework | egui 0.33 + eframe (glow backend) |
| Markdown | egui_commonmark 0.22 (vendored with custom typography) |
| File Watching | notify 6.1 + notify-debouncer-mini |
| File Dialogs | rfd |
| CLI | clap |
| Allocator | mimalloc |

**Binary:** ~8.7MB (includes syntax highlighting, image support, Wayland+X11)

## Before You Code

**STOP and do these checks first:**

1. **Check branch:** `git branch --show-current` (must not be `main`)
2. **Read files you'll modify** (use Read tool, don't rely on memory)
3. **Search LESSONS.md:** `grep -i "keyword" docs/LESSONS.md`
4. **Check recent changes:** `git log --oneline -5`

**For feature work, follow the egui immediate-mode pattern:**
```
State → Logic → UI → Async
```
See `docs/EGUI_WORKFLOW.md` for the complete guide.

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
- `egui-patterns.md` - **Critical** - Never parse in UI code, State→Logic→UI→Async
- `build-commands.md` - cargo build, run, clippy, make install
- `devlog-workflow.md` - How to document feature implementations
- `worktree-workflow.md` - How to create and manage worktrees
- `system-dependencies.md` - Arch Linux packages
- `refactoring-rules.md` - **Read before refactoring** - prevent regressions
- `context-awareness.md` - **Read before coding** - ensure fresh context

**Imported** (via `@path`):
- @docs/EGUI_WORKFLOW.md - **Read before coding** - State→Logic→UI→Async pattern
- @docs/ARCHITECTURE.md - Core components, libraries, rendering flow
- @docs/KEYBOARD_SHORTCUTS.md - All keyboard shortcuts
- @docs/TARGET_METRICS.md - Performance targets and planned features
- @docs/LESSONS.md - **Check before debugging** - reusable fixes and gotchas

## Quick Reference

```bash
# Build
cargo build
cargo run -- file.md       # File watching enabled by default

# Worktrees
git -C ~/markdown-viewer/.bare worktree list

git -C ~/markdown-viewer/.bare worktree add \
    ~/markdown-viewer/worktrees/<name> -b feature/<name> main

git -C ~/markdown-viewer/.bare worktree remove \
    ~/markdown-viewer/worktrees/<name>
```

## Branch Protection

A Claude Code hook prevents editing files on `main` branch. Create a feature worktree for all new work.
