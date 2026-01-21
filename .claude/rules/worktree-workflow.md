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

```bash
git -C ~/markdown-viewer/.bare worktree add \
    ~/markdown-viewer/worktrees/<name> -b feature/<name>
```

Claude Code automatically finds `.claude/rules/` by searching parent directories (via the root symlink).

## Managing Worktrees

```bash
git -C ~/markdown-viewer/.bare worktree list              # See all worktrees
git -C ~/markdown-viewer/.bare worktree remove \
    ~/markdown-viewer/worktrees/<name>                    # Remove after merge
```

## Slash Command

Use `/feature <description>` to automatically create a worktree, implement a feature, and report when done.

## Branch Protection

A Claude Code hook prevents editing files on `main` branch. Create a feature worktree for all new work.
