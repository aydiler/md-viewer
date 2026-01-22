# Feature: Expand/Collapse All Buttons

**Status:** ✅ Complete
**Branch:** `feature/expand-collapse-all`
**Date:** 2026-01-22
**Lines Changed:** +60 / -0 in `src/main.rs`

## Summary

Added expand all (⊞) and collapse all (⊟) buttons to the file explorer header, allowing users to quickly expand or collapse the entire directory tree.

## Features

- [x] Expand all button (⊞) - expands all directories recursively
- [x] Collapse all button (⊟) - collapses all directories
- [x] MCP widget registration for E2E testing

## Key Discoveries

### Icon-only buttons don't generate useful AccessKit labels

When testing with the egui MCP bridge, we discovered that icon-only buttons like `ui.small_button("⊞")` don't generate meaningful AccessKit node labels. This is because AccessKit uses the button text as the label, and Unicode symbols like ⊞ aren't semantically meaningful.

**Solution:** Icon-only buttons require explicit MCP widget registration to be clickable via the MCP bridge. We implemented this using the new `ManagedResponse` API.

### ManagedResponse pattern for MCP registration enforcement

Created a wrapper type that warns at development time when widgets aren't registered for MCP testing:

```rust
#[cfg(feature = "mcp")]
{
    if ui.small_button("⊟").on_hover_text("Collapse all")
        .managed_as("collapse all button")
        .register_button(&self.mcp_bridge, "Collapse All")
        .clicked()
    {
        self.file_explorer.collapse_all();
    }
}
```

The `.managed_as()` extension provides a hint for better warning messages if registration is forgotten.

### AccessKit bounds extraction for coordinate-based clicks

During this work, we also enhanced the egui-mcp-bridge to extract bounding box coordinates from AccessKit nodes. This enables coordinate-based click injection for widgets that have bounds but lack explicit registration.

## Architecture

### New Functions on FileExplorer

| Function | Purpose |
|----------|---------|
| `expand_all()` | Sets `expanded_dirs` to contain all directories in the tree |
| `collapse_all()` | Clears `expanded_dirs` HashSet |
| `collect_all_dirs()` | Recursively collects all directory paths from tree nodes |

```rust
impl FileExplorer {
    fn expand_all(&mut self) {
        self.expanded_dirs = Self::collect_all_dirs(&self.tree);
    }

    fn collapse_all(&mut self) {
        self.expanded_dirs.clear();
    }

    fn collect_all_dirs(nodes: &[FileTreeNode]) -> HashSet<PathBuf> {
        let mut dirs = HashSet::new();
        for node in nodes {
            if let FileTreeNode::Directory { path, children, .. } = node {
                dirs.insert(path.clone());
                dirs.extend(Self::collect_all_dirs(children));
            }
        }
        dirs
    }
}
```

## Testing Notes

Tested via egui MCP bridge:
1. Connect to app on port 9877
2. Take snapshot - verify buttons appear with refs
3. Click "Expand All" - verify all directories expand (node count increases)
4. Click "Collapse All" - verify all directories collapse (node count decreases)

## Future Improvements

- [ ] Add keyboard shortcuts for expand/collapse all (e.g., Ctrl+Shift+Plus/Minus)
- [ ] Consider adding "expand to level N" functionality
