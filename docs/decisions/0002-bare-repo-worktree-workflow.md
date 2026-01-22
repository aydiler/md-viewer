# ADR-0002: Use Bare Repository with Git Worktrees

**Status:** Accepted
**Date:** 2026-01-22
**Deciders:** Ahmet

## Context

The project frequently has multiple features in progress simultaneously. Switching branches with uncommitted changes is error-prone, and stashing/unstashing disrupts flow. We needed a workflow that allows parallel work on multiple features.

## Decision Drivers

- Multiple features often in-flight (drag-scroll, minimap, outline, etc.)
- Want to preserve work-in-progress without committing broken code
- Claude Code sessions benefit from isolated working directories
- Need to prevent accidental commits to main branch

## Considered Options

### Option 1: Standard branching with stash

Use normal git branches, stash changes when switching:
```bash
git stash
git checkout feature-x
git stash pop
```

**Pros:**
- Standard git workflow everyone knows
- Single working directory
- No extra disk space

**Cons:**
- Stash conflicts are common
- Can't work on two features simultaneously
- Easy to forget stashed changes
- Context switching overhead

### Option 2: Multiple clones

Clone the repo multiple times:
```bash
~/markdown-viewer-main/
~/markdown-viewer-feature-x/
~/markdown-viewer-feature-y/
```

**Pros:**
- Complete isolation
- No git complexity

**Cons:**
- Wastes disk space (duplicate .git)
- Changes don't share history easily
- Fetching updates in each clone

### Option 3: Bare repo + worktrees

Bare repository with worktrees for each branch:
```bash
~/markdown-viewer/
├── .bare/           # Git database
└── worktrees/
    ├── main/
    └── feature-x/
```

**Pros:**
- Single git database, multiple working directories
- Each worktree is fully isolated
- Can work on multiple features simultaneously
- Natural home for Claude Code hooks/rules
- Disk efficient (shared objects)

**Cons:**
- Non-standard setup, learning curve
- Worktree commands are verbose
- Must remember to clean up old worktrees

## Decision

Use bare repository at `.bare/` with all work happening in `worktrees/` (Option 3).

Combined with a Claude Code hook that blocks edits on `main`, this enforces a clean workflow: all changes happen on feature branches with full isolation.

## Consequences

### Positive

- Parallel feature development without stashing
- Claude Code sessions get dedicated working directories
- Branch protection hook prevents main branch accidents
- Clean mental model: one directory = one feature

### Negative

- Contributors must learn worktree commands
- Need documented workflow (see `.claude/rules/worktree-workflow.md`)
- Old worktrees accumulate if not cleaned up

## Related

- `.claude/rules/worktree-workflow.md` - How to create/remove worktrees
- `.claude/hooks/branch-protect.sh` - Prevents main branch edits
- `CLAUDE.md` - Documents the directory structure
