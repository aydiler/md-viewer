# Feature: Recent Folders dims missing folders

**Status:** ✅ Complete
**Branch:** `feature/recent-folders-missing`
**Date:** 2026-11-27
**Lines Changed:** `src/main.rs` (`render_empty_explorer`, `render_welcome`)

## Summary

Two suggestions from the #297 review (devlog 125), done after it merged:

- The welcome page dims and disables a recent file that no longer exists;
  Recent Folders showed a missing folder like any other, and clicking it
  reported "Folder is unavailable". It now matches the welcome page.
- The new ✕, folder and Clear History buttons had no MCP registration.

## Features

- [x] `render_empty_explorer` checks `is_dir()` for each listed folder while
  the list is shown, as `render_welcome` checks `exists()`. A missing folder's
  name is weak, its button disabled, and its tooltip "Folder not found:
  <path>". Its ✕ stays enabled.
- [x] A missing file on the welcome page gets a "File not found: <path>"
  tooltip too.
- [x] `#[cfg(feature = "mcp")]` registrations: `Recent Folders: Clear
  History`, `Recent Folder: <name>` (value `missing` when absent), `Remove
  Recent Folder: <name>`, `Recent Files: Clear History`, `Remove Recent File:
  <name>`.

## Key Discoveries

### A disabled widget ignores `on_hover_text`

egui shows `on_hover_text` only while a widget is enabled; a disabled one
shows `on_disabled_hover_text`. The welcome page's missing files therefore had
no tooltip at all, although the code passed the path to `on_hover_text`. Both
lists now set both.

### The MCP registrations cannot be compiled

The `mcp` feature does not build against egui 0.37 (LESSONS, "The egui MCP
bridge does not build against egui 0.37"), so the new registrations copy the
existing call shape but have not been compiled.

## Testing Notes

On Xvfb, with a state file listing an existing and a missing folder (and an
existing and a missing recent file), against main built at the same commit:

| | branch | main |
|---|---|---|
| missing folder's entry | dimmed | like the others |
| hover it | "Folder not found: <path>" | path |
| click it | nothing happens | "Folder is unavailable" error bar |
| hover the missing recent file | "File not found: <path>" | no tooltip |
| ✕ on the missing folder, quit, restart | gone; saved list has only the existing folder | — |
| click the existing folder | opens it | opens it |

App tests pass; the test list is unchanged (128 on both); `clippy -D
warnings` and `fmt` are clean.

## Future Improvements

- [ ] Check existence once per change of the list rather than on every
  frame the list is shown (the welcome page does the same per frame).
