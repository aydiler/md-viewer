# Target Metrics

- Binary size: ~35 MB release build (includes full syntax highlighting via syntect, mermaid renderer, math rendering, image support, Wayland+X11). ~7 MB compressed as snap.
- Startup time: < 200ms
- Render: 60 FPS with viewport-based lazy rendering
- Platform: Linux X11 and Wayland

## Feature Progress

- **Phase A**: Multi-window support via egui viewports - COMPLETED (now replaced by tabs)
- **Phase B**: Tab system (egui_dock) - COMPLETED
- **Phase C**: Hybrid tabs + multi-window - Future

## Planned (Open Feature Requests)

Tracked in issue #4. Recommended order:

1. ~~**Search / find-all (Ctrl+F)**~~ — shipped in v0.1.4 (PR #14).
2. ~~**Table horizontal-scroll UX**~~ — wide-table overflow remains reachable through the nested `egui::ScrollArea::horizontal()` bottom scrollbar, native horizontal input, and `Shift+vertical-wheel` while the cursor hovers the table (`forward_shift_wheel_to_horizontal_scroll`). The unconditional wheel-routing helper was removed because it made normal document scrolling nudge wide tables horizontally (#22); Shift is the explicit opt-in for sideways scrolling without grabbing the bottom scrollbar.
3. **Resizable table columns** — large refactor: swap `egui::Grid` for `egui_extras::TableBuilder` in the vendored fork. Regression-testing all existing table edge cases required.
