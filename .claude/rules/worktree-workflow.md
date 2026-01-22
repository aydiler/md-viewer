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

**Step 1: Create worktree**
```bash
git -C ~/markdown-viewer/.bare worktree add \
    ~/markdown-viewer/worktrees/<name> -b feature/<name>
```

**Step 2: Create devlog stub**
```bash
# Find next number
ls docs/devlog/[0-9]*.md | sort | tail -1

# Copy template (replace NNN with next number)
cp docs/devlog/TEMPLATE.md docs/devlog/NNN-<name>.md
```

Devlog MUST exist before any commits. Infrastructure and "phase work" still require devlogs.

**Step 3: Verify fresh base (CRITICAL)**
```bash
# Ensure you're based on latest main
git fetch origin
git log --oneline HEAD..origin/main  # Should be empty!

# If NOT empty, your branch is stale. Rebase first:
git rebase origin/main
```

**Why this matters:** Working from stale code causes regressions. Recent fixes in main won't be in your branch, and your changes may silently overwrite them when merged.

Claude Code automatically finds `.claude/rules/` by searching parent directories (via the root symlink).

## Managing Worktrees

```bash
git -C ~/markdown-viewer/.bare worktree list              # See all worktrees
git -C ~/markdown-viewer/.bare worktree remove \
    ~/markdown-viewer/worktrees/<name>                    # Remove after merge
```

## Slash Command

Use `/feature <description>` to automatically create a worktree, implement a feature, and report when done.

## Before Writing Code

After creating a worktree, before writing ANY code:

1. **Read files you'll modify** - Use Read tool, don't rely on memory
2. **Check recent commits** - `git log --oneline -10`
3. **Check LESSONS.md** - Search for relevant gotchas
4. **Preserve existing patterns** - If code uses `.scroll_source()`, keep it

See `context-awareness.md` and `refactoring-rules.md` for full guidelines.

## Branch Protection

A Claude Code hook prevents editing files on `main` branch. Create a feature worktree for all new work.
