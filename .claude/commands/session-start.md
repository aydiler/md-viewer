---
description: Start a development session with fresh context
allowed-tools: Bash, Read
---

Prepare for a development session by gathering context and verifying branch freshness.

**Steps:**

1. Show current branch and worktree:
```bash
git branch --show-current
pwd
```

2. Check if branch is up-to-date with main:
```bash
git fetch origin 2>/dev/null
git log --oneline HEAD..origin/main 2>/dev/null | head -5
```
If output is not empty, warn that the branch may be stale.

3. Show recent commits on this branch:
```bash
git log --oneline -5
```

4. Check for any work-in-progress:
```bash
git status --short
```

5. List any relevant gotchas from LESSONS.md by searching for keywords related to what the user mentioned they want to work on.

6. Summarize:
   - Current branch and status
   - Whether rebase is recommended
   - Any uncommitted changes
   - Relevant lessons to keep in mind
