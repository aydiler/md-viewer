# Feature: Devlog Workflow Improvement

**Status:** ✅ Complete
**Branch:** `feature/devlog-workflow`
**Date:** 2026-01-22
**Files Changed:** `.claude/rules/worktree-workflow.md`, `.claude/rules/devlog-workflow.md`

## Summary

Improve the worktree workflow to prevent missing devlogs by making devlog creation a mandatory step when creating a feature worktree. This addresses the gap where multi-window support (Phase A) was implemented without documentation.

## Features

- [x] Update worktree-workflow.md to include devlog creation step
- [x] Add "infrastructure is a feature" policy

## Key Discoveries

### Root cause of missing devlogs

Analysis of the multi-window feature gap revealed:
1. Feature was mentally classified as "Phase A prep work" not a standalone feature
2. Late-night development momentum (01:22 AM commit)
3. Worktree named `tab-system` suggested larger scope, so intermediate work felt exempt
4. No enforcement mechanism — devlog workflow was advisory only

### Prevention strategy

Auto-creating the devlog stub at worktree creation time changes the psychology:
- Filling in existing file = completion
- Creating new file = extra work

This is Option 1 from the analysis. Soft enforcement chosen over hard-blocking hooks to avoid friction during rapid iteration.

## Architecture

No code changes. Documentation updates only:

| File | Change |
|------|--------|
| `.claude/rules/worktree-workflow.md` | Added Step 2: create devlog stub after worktree |
| `.claude/rules/devlog-workflow.md` | Added "What Requires a Devlog" section with phase work policy |

## Future Improvements

- [ ] Post-commit reminder hook if soft measures prove insufficient
- [ ] Hard-blocking pre-commit hook as last resort
- [ ] Retroactive devlog for multi-window feature (separate PR)
