# Context Awareness Rules

These rules ensure Claude has accurate context before modifying code, preventing regressions from working with stale or incomplete information.

## Before Implementing ANY Feature

### 1. Read Recent Commits

Before starting work, understand what changed recently:

```bash
# See recent commit messages
git log --oneline -10

# See actual code changes in recent commits
git log -p -3 -- src/main.rs
```

**Why:** Recent commits may contain fixes or features that your changes must preserve.

### 2. Read Files Before Modifying

ALWAYS use the Read tool on files you plan to modify:

- Note configuration patterns (e.g., `.scroll_source()`, `.auto_shrink()`)
- Note struct fields and their purposes
- Note any comments explaining non-obvious code

**Never rely on memory or assumptions about file contents.**

### 3. Check LESSONS.md

Before debugging or implementing in an area:

```bash
grep -i "keyword" docs/LESSONS.md
```

LESSONS.md contains hard-won fixes and gotchas. Ignoring it wastes time re-discovering solved problems.

### 4. Preserve Patterns You See

If existing code uses a pattern, new code must follow it:

| If You See | New Code Must |
|------------|---------------|
| `.scroll_source(...)` on ScrollArea | Include scroll_source config |
| `.auto_shrink([false, false])` | Maintain same shrink behavior |
| Error handling with `if let Some(...)` | Use same error pattern |
| Comments explaining "why" | Preserve or update comments |

**Removing patterns without understanding them causes regressions.**

## When Starting a New Feature Worktree

### Verify Fresh Base

Before writing any code:

```bash
# Check your branch is based on latest main
git fetch origin
git log --oneline HEAD..origin/main

# If output is NOT empty, rebase first:
git rebase origin/main
```

### Read Architecture Docs

Skim these before major changes:

- `docs/ARCHITECTURE.md` - understand component structure
- `docs/LESSONS.md` - know the gotchas
- Recent devlogs in `docs/devlog/` - understand recent changes

## Red Flags - You're Missing Context

### "I'll just rewrite this part"
Stop. Read the existing code first. Understand WHY it's structured that way.

### "This code seems unnecessary"
It probably isn't. Check git blame to see when/why it was added:
```bash
git blame -L 100,120 src/main.rs
git show <commit-hash>  # Read the commit message
```

### "I remember the code does X"
Don't trust memory. Use Read tool to verify current state.

### Copy-pasting from earlier in conversation
Conversation context may be stale. Re-read the actual file.

## Checklist Before Modifying Code

- [ ] Read the file(s) I'm about to modify
- [ ] Checked `git log -5` for recent changes
- [ ] Searched LESSONS.md for relevant gotchas
- [ ] Identified patterns in existing code to preserve
- [ ] Verified my branch is based on latest main
