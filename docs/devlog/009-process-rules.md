# 009 - Process Rules for Regression Prevention

**Status:** Completed
**Branch:** `feature/process-rules`
**Date:** 2026-01-22

## Problem

During the tab system refactoring (commit `dab927a`), several features were unintentionally lost:
- `scroll_source` restrictions (drag-to-scroll re-enabled)
- Large file line count indicator
- Multi-window support (intentionally removed, but undocumented)

Root cause: The refactor was developed from stale code rather than incrementally modifying HEAD. Even though git recorded proper parentage, the actual code was written independently, causing recent fixes to be silently overwritten.

## Solution

Implement Tier 1 process rules - documentation and workflow changes that prevent this class of error:

1. **refactoring-rules.md** - Enforce incremental changes, ban large rewrites
2. **context-awareness.md** - Ensure Claude reads current state before modifying
3. **worktree-workflow.md update** - Require rebase check before starting work

## Files Changed

- `.claude/rules/refactoring-rules.md` (new)
- `.claude/rules/context-awareness.md` (new)
- `.claude/rules/worktree-workflow.md` (updated)

## Key Rules Introduced

### Refactoring Rules
- Use Edit tool, not Write tool for existing files
- One logical change per commit
- Separate refactoring from feature work
- Red flag: >100 lines changed in single commit

### Context Awareness
- Read recent commits before starting work
- Read files before modifying them
- Check LESSONS.md for gotchas
- Preserve existing patterns

### Workflow Updates
- Verify branch is based on latest main before starting
- Explicit rebase check command

## Lessons Learned

### Process failures require process solutions
Technical safeguards (tests, hooks) help, but the root cause was a workflow problem. The developer (human + AI) worked from old code instead of current HEAD.

### "All features preserved" claims need verification
The original commit message was wrong. Future commits making such claims should include evidence (test results, behavior checklist).

### Incremental > Rewrite
A 50% file rewrite hides regressions. Many small commits make each change reviewable.

## Future Improvements

- Tier 2: Add automated diff size warning hook
- Tier 3: Add behavioral regression tests
- Tier 4: Create configuration registry for explicit behavior tracking
