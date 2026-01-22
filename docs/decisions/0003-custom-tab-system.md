# ADR-0003: Custom Tab System Instead of egui_dock

**Status:** Accepted
**Date:** 2026-01-22
**Deciders:** Ahmet

## Context

The markdown-viewer needed multi-document support. Users should be able to open multiple markdown files and switch between them. The question was whether to use an existing docking/tab library or build a simple custom solution.

## Decision Drivers

- Only need tabs, not full docking/splitting functionality
- Want minimal dependencies and binary size
- Need per-tab state (scroll position, navigation history, cache)
- Must integrate with existing outline and file explorer sidebars

## Considered Options

### Option 1: egui_dock

Use the egui_dock crate for full docking support:
```toml
egui_dock = "0.15"
```

**Pros:**
- Full-featured: tabs, splits, floating windows
- Active community
- Drag-and-drop tab reordering

**Cons:**
- Heavyweight for our needs (we only want tabs)
- Complex state management with DockState
- Would need to adapt our sidebars to work with docking
- Extra dependency ~100KB

### Option 2: Custom `Vec<Tab>` implementation

Simple `Vec<Tab>` with manual tab bar rendering:
```rust
struct MarkdownApp {
    tabs: Vec<Tab>,
    active_tab: usize,
}
```

**Pros:**
- Exactly the features we need, nothing more
- Full control over rendering and behavior
- Easy to add per-tab state
- No external dependency
- Simple debugging

**Cons:**
- Must implement tab bar UI ourselves
- No drag-to-reorder (not needed currently)
- No split views (not planned)

### Option 3: egui built-in tabs

Use egui's `ui.selectable_label()` in a horizontal layout:
```rust
ui.horizontal(|ui| {
    for (i, tab) in tabs.iter().enumerate() {
        if ui.selectable_label(i == active, &tab.title).clicked() {
            active = i;
        }
    }
});
```

**Pros:**
- Uses only egui primitives
- Very lightweight
- Easy to style consistently

**Cons:**
- Same implementation effort as Option 2
- No real advantage over a proper Tab struct

## Decision

Custom `Vec<Tab>` with `ui.selectable_label()` for the tab bar (hybrid of Options 2 and 3).

Each `Tab` struct holds all per-document state:
- Path and content
- CommonMarkCache (must persist across frames)
- Scroll position and pending scroll targets
- Navigation history (back/forward stacks)
- Parsed headers for outline

This gives us exactly what we need with zero external dependencies.

## Consequences

### Positive

- Minimal complexity, easy to understand
- Per-tab state is explicit and type-safe
- No fighting with library abstractions
- Binary size unchanged

### Negative

- No drag-to-reorder tabs (acceptable for now)
- If we later want split views, would need rework
- Must handle edge cases ourselves (empty tabs, close last tab)

## Related

- `docs/ARCHITECTURE.md` - Tab struct documentation
- `docs/LESSONS.md` - "Avoid iterating and mutating tabs simultaneously"
- `src/main.rs` - Tab and MarkdownApp implementation
