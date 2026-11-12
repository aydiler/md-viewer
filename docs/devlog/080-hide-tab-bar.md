# Hide the tab bar (#208)

**Status:** Complete
**Branch:** `feature/hide-tab-bar`
**Date:** 2026-11-12

## Summary

Issue #208 asks for a way to hide the tab bar, for using md-viewer as a
single-file viewer. **View → Show Tab Bar** now toggles it, and the choice is
persisted.

## The catch

The tab bar is not only tabs. Since #205 it also holds the **Preview | Edit**
toggle, the unsaved-changes dot and the "Saved" confirmation. Hiding it
unconditionally would make Edit mode unreachable and hide unsaved changes.

So:
- The bar stays visible while the active tab is in Edit mode, whatever the
  setting (`tab_bar_visible(show_tab_bar, active_tab_editing)`).
- Edit mode gets two entry points that do not need the bar: **View → Edit
  Mode** and **Ctrl+E** (`ctrl && !shift`, so Ctrl+Shift+E still toggles the
  explorer).

## Architecture

- `MarkdownApp::show_tab_bar: bool`, `PersistedState::show_tab_bar:
  Option<bool>`. A state file without the field (every file written before
  this change) shows the bar.
- `fn tab_bar_visible` gates the `TopBottomPanel::top("tab_bar")`.
- View menu: "Show Tab Bar" and "Edit Mode", both registered for MCP
  ("Menu: View → Show Tab Bar", "Menu: View → Edit Mode").
- Ctrl+E handler next to the other view shortcuts; it calls
  `Tab::set_edit_mode`, the same function the segmented toggle uses.

## Verification

- Unit tests: `tab_bar_shows_unless_hidden_and_always_while_editing` (seen red
  with the edit-mode exception removed) and
  `hidden_tab_bar_survives_a_persistence_roundtrip` (also decodes an empty,
  pre-#208 state).
- `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --locked`
  (80 passed).
- Xvfb E2E (MCP build, isolated `XDG_*` dirs, README.md):
  - View → Show Tab Bar off: the snapshot loses `Tab: README.md`, and the
    screenshot shows the document directly under the menu bar.
  - View → Edit Mode: the bar comes back with Preview | Edit and the raw text.
  - Preview: the bar hides again.
  - Saved state has `show_tab_bar:Some(false)`; after a restart the tab
    widgets are still absent.

## Key Discoveries

- Clicking View-menu items through `egui_click` was unreliable on Xvfb: one
  click opened or closed the menu out of step with the snapshot, and one landed
  on Dark Mode instead of Edit Mode. Real pointer clicks
  (`xdotool mousemove --window <id> x y click 1`) at coordinates read off a
  screenshot were reliable.

## Future Improvements

- An "auto-hide with one tab" mode, if someone asks for it.
