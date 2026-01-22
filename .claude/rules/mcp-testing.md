# MCP Testing Rules

This project supports E2E testing via egui-mcp-bridge. Follow these rules when adding UI widgets to ensure testability.

## Quick Reference

```rust
// Register widgets after creating them
let btn = ui.small_button("⊞").on_hover_text("Expand all");
#[cfg(feature = "mcp")]
self.mcp_bridge.register_widget("Explorer: Expand All", "button", &btn, None);
if btn.clicked() { ... }
```

## When to Register Widgets

### Always Register (Interactive)

These widgets MUST be registered for MCP testing:

| Widget Type | Registration | Example |
|-------------|--------------|---------|
| Buttons | `register_widget(name, "button", &response, None)` | `register_widget("Save", "button", &btn, None)` |
| Small buttons | `register_widget(name, "button", &response, None)` | `register_widget("Close Tab", "button", &btn, None)` |
| Selectable labels | `register_widget(name, "tab", &response, value)` | `register_widget("Tab: README", "tab", &resp, Some("active"))` |
| Checkboxes | `register_widget(name, "checkbox", &response, value)` | `register_widget("Dark Mode", "checkbox", &resp, Some("checked"))` |

### Register When Stateful (Informational)

Register labels/text that display testable state:

```rust
// Directory with expand/collapse state
#[cfg(feature = "mcp")]
self.mcp_bridge.register_widget(
    &format!("Toggle: {}", name),
    "button",
    &toggle_response,
    Some(if expanded { "expanded" } else { "collapsed" })
);
```

### Skip Registration (Decorative)

These typically don't need registration:
- Pure decorative icons/separators
- Static labels without testable state
- Layout helpers (spacers, etc.)

## Icon-Only Buttons

Icon-only buttons like `"⊞"`, `"×"`, `"↻"` don't generate meaningful AccessKit labels. They **always** need explicit registration:

```rust
// WRONG - not testable via MCP
ui.small_button("×").clicked()

// CORRECT - testable
let btn = ui.small_button("×");
#[cfg(feature = "mcp")]
self.mcp_bridge.register_widget("Close Tab", "button", &btn, None);
if btn.clicked() { ... }
```

## Naming Conventions

Use descriptive, unique names that identify the widget's purpose:

| Pattern | Example |
|---------|---------|
| Action buttons | `"Save"`, `"Close"`, `"Refresh"` |
| Toggle buttons | `"Toggle: Dark Mode"`, `"Toggle: Outline"` |
| Tab buttons | `"Tab: filename.md"` |
| File entries | `"File: filename.md"` |
| Directory entries | `"Directory: dirname"` |
| Panel buttons | `"Explorer: Expand All"`, `"Outline: Collapse All"` |

## Feature Flag Handling

Use `#[cfg(feature = "mcp")]` to conditionally compile MCP registration:

```rust
let btn = ui.button("Save");
#[cfg(feature = "mcp")]
self.mcp_bridge.register_widget("Save", "button", &btn, None);
if btn.clicked() {
    save_file();
}
```

## Testing Your Widgets

After adding widgets, verify they're testable:

1. Run with MCP: `cargo run --features mcp -- file.md`
2. Connect: `egui_connect({ host: "127.0.0.1", port: 9877 })`
3. Snapshot: `egui_snapshot()` - verify your widget appears with a ref
4. Click: `egui_click({ ref: "nXX" })` - verify it works

## Checklist for New UI Code

- [ ] Interactive widgets have `register_widget()` calls with `#[cfg(feature = "mcp")]`
- [ ] Icon-only buttons have descriptive names
- [ ] Stateful widgets include value parameter (e.g., "expanded", "active")
- [ ] Names are unique and descriptive
- [ ] Tested widget appears in `egui_snapshot()` output
