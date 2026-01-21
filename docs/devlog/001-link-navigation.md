# Feature: Link Navigation with History

**Status:** ✅ Complete
**Branch:** `feature/implement-first-phase-of-the-implementation-plan`
**Date:** 2026-01-21
**Lines Changed:** +209 in `src/main.rs`

## Summary

Implemented Phase D from the development plan: clicking local markdown links now navigates to that file instead of opening in browser, with back/forward history support.

## Features

- [x] Parse local markdown links from content
- [x] Register link hooks via egui_commonmark
- [x] Navigate to clicked links (resolve relative paths)
- [x] Back/forward navigation history
- [x] Keyboard shortcuts (Alt+←/→)
- [x] Navigate menu with Back/Forward items
- [x] Handle anchor-only links (#section) - intercept to prevent browser errors

## Key Discoveries

### egui_commonmark Link Hook API

The library provides a hook mechanism to intercept link clicks:

```rust
// Register hooks for links you want to handle
cache.add_link_hook("./other.md");

// After CommonMarkViewer::show(), check if clicked
if let Some(true) = cache.get_link_hook("./other.md") {
    // Link was clicked - handle navigation
}
```

**Important:** Hooks reset to `false` before each `show()` call automatically.

### Link Parsing Strategy

Used regex to find markdown links, skipping code blocks:
```rust
let link_re = Regex::new(r"\[([^\]]*)\]\(([^)]+)\)").unwrap();
```

Filter for local links by excluding:
- `http://`, `https://`, `mailto:`, `tel:`, `ftp://`
- Check for `.md`, `.markdown`, `.txt` extensions

### Anchor Links Gotcha

Anchor-only links (`#section`) must also be hooked, otherwise egui_commonmark passes them to the browser which tries to open `file:///path/#section` and fails.

Solution: Register hooks for anchor links too, but ignore them in `navigate_to_link()`.

### Path Resolution

Links are resolved relative to current file's directory:
```rust
let target_path = current_dir.join(path_part);
let target_path = target_path.canonicalize()?; // Resolves ../
```

## Architecture

### New Fields in MarkdownApp

```rust
history_back: Vec<PathBuf>,      // Back navigation stack
history_forward: Vec<PathBuf>,   // Forward navigation stack
local_links: Vec<String>,        // Cached links for current doc
```

### New Functions

| Function | Purpose |
|----------|---------|
| `parse_local_links()` | Extract local markdown links from content |
| `is_local_markdown_link()` | Check if link is local file (not http, etc.) |
| `navigate_to_link()` | Resolve and navigate to a link |
| `navigate_back()` | Go back in history |
| `navigate_forward()` | Go forward in history |
| `check_link_hooks()` | Check if any link was clicked |

## Future Improvements

- [ ] Implement anchor scrolling (`#section` jumps to header)
- [ ] Handle links with anchors (`file.md#section`)
- [ ] Add visual indicator for navigation history depth
- [ ] Consider caching resolved paths
