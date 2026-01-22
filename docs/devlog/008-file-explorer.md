# Feature: File Explorer Sidebar

**Status:** ✅ Complete
**Branch:** `feature/file-explorer`
**Date:** 2026-01-22
**Lines Changed:** +200 in `src/main.rs`

## Summary

Add a left sidebar file explorer showing all markdown files (.md, .markdown, .txt) in a hierarchical directory tree. Clicking a file opens it in a new tab.

## Features

- [x] FileTreeNode enum for tree structure
- [x] FileExplorer struct with root, tree, expanded_dirs
- [x] Recursive directory scanning with depth limit (10 levels)
- [x] Render hierarchical tree with expand/collapse
- [x] Click to open file in new tab
- [x] Ctrl+Shift+E keyboard shortcut
- [x] Session persistence (show_explorer, explorer_root, expanded_dirs)
- [x] View menu integration

## Key Discoveries

### Recursive rendering with borrow checker

When rendering a tree recursively, you need to clone the tree data to avoid borrow conflicts with `&mut self`. The tree is cloned before iteration, and mutable operations (like toggling expanded dirs) are done through `self.file_explorer` which remains accessible.

```rust
// Clone tree to avoid borrow issues
let tree = self.file_explorer.tree.clone();
for node in &tree {
    if let Some(path) = self.render_tree_node(ui, node, 0, &open_paths) {
        file_to_open = Some(path);
    }
}
```

### Root directory determination priority

The explorer root is determined with a clear priority:
1. Parent directory of CLI-provided file
2. Persisted state (if directory still exists)
3. Parent directory of first open tab
4. Current working directory (fallback)

## Architecture

### New/Modified Structs

```rust
enum FileTreeNode {
    File { path: PathBuf, name: String },
    Directory { path: PathBuf, name: String, children: Vec<FileTreeNode> },
}

struct FileExplorer {
    root: Option<PathBuf>,
    tree: Vec<FileTreeNode>,
    expanded_dirs: HashSet<PathBuf>,
}

// PersistedState additions:
show_explorer: Option<bool>,
explorer_root: Option<PathBuf>,
expanded_dirs: Option<Vec<PathBuf>>,

// MarkdownApp additions:
file_explorer: FileExplorer,
show_explorer: bool,
```

### New Functions

| Function | Purpose |
|----------|---------|
| `FileExplorer::scan_directory()` | Recursively scan directory for markdown files |
| `FileExplorer::set_root()` | Set root directory and trigger scan |
| `FileExplorer::refresh()` | Rescan current root directory |
| `FileExplorer::toggle_expanded()` | Toggle directory expansion |
| `FileExplorer::is_expanded()` | Check if directory is expanded |
| `render_file_explorer()` | Render left sidebar panel |
| `render_tree_node()` | Recursive tree node rendering |

## Testing Notes

- Verify tree displays correctly with nested directories
- Test file opening creates new tabs
- Test expand/collapse persists across sessions
- Test Ctrl+Shift+E toggles visibility
- Test refresh button rescans directory

## Future Improvements

- [ ] Search/filter within file tree
- [ ] File rename/delete from explorer
- [ ] Drag files to reorder tabs
- [ ] Show file modification dates
- [ ] Context menu for files (copy path, open in editor, etc.)
- [ ] Watch directory for changes and auto-refresh
