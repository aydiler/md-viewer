# Feature: File Explorer Live Reload

**Status:** ✅ Complete
**Branch:** `feature/explorer-live-reload`
**Date:** 2026-01-24
**Lines Changed:** +25 / -10 in `src/main.rs`

## Summary

Extends the existing file watcher to also monitor the explorer root directory recursively. When files are created, deleted, or renamed within the explorer directory, the file tree automatically refreshes.

## Features

- [x] Watch explorer root directory recursively
- [x] Refresh file tree on directory changes
- [x] Flash effect shows for updated files/directories
- [x] Maintains existing tab content live reload

## Key Discoveries

### RecursiveMode::Recursive vs NonRecursive

The notify crate supports two watch modes:
- `NonRecursive`: Watch only the specified path (used for individual tab files)
- `Recursive`: Watch the path and all subdirectories (used for explorer root)

Using recursive mode on the explorer root catches all file additions, deletions, and renames within the tree.

### Path.starts_with() for hierarchy check

To determine if a changed path is within the explorer root:
```rust
if path.starts_with(root) {
    refresh_tree = true;
}
```

## Architecture

### Modified Structs

```rust
struct MarkdownApp {
    // ... existing fields ...
    watched_paths: HashSet<PathBuf>,       // Individual tab files (NonRecursive)
    watched_explorer_root: Option<PathBuf>, // Explorer root (Recursive) - NEW
}
```

### Modified Functions

| Function | Change |
|----------|--------|
| `start_watching()` | Also watches explorer root with RecursiveMode::Recursive |
| `stop_watching()` | Clears watched_explorer_root |
| `check_file_changes()` | Updated recovery check to consider explorer root |
| `reload_changed_tabs()` | Now also refreshes file tree when changes within explorer root |

## Testing Notes

Test scenarios:
1. Start with `-w` flag, create new .md file in explorer directory → tree updates
2. Delete an open tab's file → tab shows stale content, tree removes file
3. Rename a file → tree updates with new name
4. Create file in subdirectory → tree updates (requires directory to be expanded to see)

## Future Improvements

- [ ] Debounce tree refresh if many rapid changes occur
- [ ] Only refresh affected subtree instead of full tree
- [ ] Highlight newly added files with different flash color
