# Worktree Workflow

This repo uses a bare repository setup at `~/markdown-viewer/.bare`. All worktrees live inside `~/markdown-viewer/worktrees/`.

## Directory Structure

```
~/markdown-viewer/
├── .bare/                      # Bare git repository
├── .claude -> worktrees/main/.claude  # Symlink to tracked config
└── worktrees/
    ├── main/
    │   └── .claude/            # Claude Code config (tracked in git)
    └── <feature-name>/         # Feature worktrees
```

## Creating a Feature Worktree

When asked to implement a feature that requires a new branch, **automatically create a worktree from main**:

```bash
# Create worktree from current main (ALWAYS specify main as start point)
git -C ~/markdown-viewer/.bare worktree add \
    ~/markdown-viewer/worktrees/<name> -b feature/<name> main

# Change to the new worktree
cd ~/markdown-viewer/worktrees/<name>

# Create devlog (see devlog-workflow.md for automation)
LAST=$(ls docs/devlog/[0-9]*.md 2>/dev/null | sort | tail -1 | grep -oP '\d{3}' | head -1)
NEXT=$(printf "%03d" $((10#$LAST + 1)))
cp docs/devlog/TEMPLATE.md docs/devlog/${NEXT}-<name>.md

# Verify main is up to date with remote before branching
git fetch origin 2>/dev/null
git log --oneline main..origin/main
```

If the last command shows commits, main is behind remote. Update main first:
```bash
git -C ~/markdown-viewer/.bare fetch origin main:main
```

Devlog MUST exist before any commits. Infrastructure and "phase work" still require devlogs.

Claude Code automatically finds `.claude/rules/` by searching parent directories (via the root symlink).

## Managing Worktrees

```bash
git -C ~/markdown-viewer/.bare worktree list              # See all worktrees
git -C ~/markdown-viewer/.bare worktree remove \
    ~/markdown-viewer/worktrees/<name>                    # Remove after merge
```

## Before Writing Code

These checks happen automatically (see `context-awareness.md`), but ensure:

1. **Read files you'll modify** - Use Read tool, don't rely on memory
2. **Check recent commits** - `git log --oneline -10`
3. **Check LESSONS.md** - Search for relevant gotchas
4. **Preserve existing patterns** - If code uses `.scroll_source()`, keep it

See `context-awareness.md` and `refactoring-rules.md` for full guidelines.

## Branch Protection

A Claude Code hook prevents editing files on `main` branch. Create a feature worktree for all new work.
