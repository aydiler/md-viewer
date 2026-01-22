# Refactoring Rules

These rules prevent regressions during refactoring by enforcing incremental, reviewable changes.

## Core Principle: Never Rewrite, Always Modify

When refactoring existing code:

1. **Use Edit tool, not Write tool** - Modify existing code incrementally, never replace entire files
2. **One logical change per commit** - Don't mix refactoring with features
3. **Preserve existing behavior** - Run the app after each change to verify nothing broke

## Refactoring vs Features Must Be Separate

If you need to refactor before adding a feature:

1. Create refactor branch, make ONLY structural changes
2. Verify all existing behavior still works
3. Merge refactor to main
4. Create feature branch from updated main
5. Add feature on clean foundation

**Why:** Mixing refactoring and features in one PR makes it impossible to distinguish intentional changes from accidental regressions.

## Red Flags - Stop and Reconsider

### >100 Lines Changed in One File
If a single commit changes >100 lines in one file:
- STOP and reconsider the approach
- Break into smaller, incremental commits
- Document why each change is needed
- Review what's being DELETED - is it intentional?

### Using Write Tool on Existing Files
The Write tool replaces entire file contents. For existing files:
- ALWAYS use Edit tool instead
- Edit preserves unchanged code automatically
- Write can silently drop code you didn't include

### "Simplifying" by Removal
If your refactor removes code to "simplify":
- Verify each removal is intentional
- Check if removed code implemented a feature or fix
- Document why the code is no longer needed

## Checklist Before Committing Refactor

- [ ] Used Edit tool (not Write) for all existing files
- [ ] Each commit has single logical change
- [ ] Ran the app and verified basic functionality
- [ ] Reviewed deleted lines - all intentional?
- [ ] Commit message accurately describes changes
- [ ] No claims like "all features preserved" without verification

## Examples

### Good: Incremental Refactor
```
Commit 1: Extract Tab struct from MarkdownApp fields
Commit 2: Move tab rendering to Tab::render method
Commit 3: Add tabs Vec to replace single content field
Commit 4: Update file loading to create Tab instances
Commit 5: Remove old single-file fields (now unused)
```

### Bad: Big Bang Rewrite
```
Commit 1: Replace entire file with new tab implementation
          (+726 lines, -704 lines, "all features preserved")
```

The bad example hides which specific changes were made, making regression invisible.
