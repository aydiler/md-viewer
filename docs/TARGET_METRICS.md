# Target Metrics

- Binary size: ~8.7MB (includes full syntax highlighting via syntect, image support, Wayland+X11)
- Startup time: < 200ms
- Render: 60 FPS with viewport-based lazy rendering
- Platform: Linux X11 and Wayland

## Feature Progress

- **Phase A**: Multi-window support via egui viewports - COMPLETED (now replaced by tabs)
- **Phase B**: Tab system (egui_dock) - COMPLETED
- **Phase C**: Hybrid tabs + multi-window - Future

## Planned (Open Feature Requests)

Tracked in issue #4. Recommended order:

1. **Search / find-all (Ctrl+F)** — bounded, ~300-500 LoC in `src/main.rs`. Draft spec exists from the PR #5 contributor.
2. **Table horizontal-scroll UX** — small (~30 lines) fix at `crates/egui_commonmark/.../pulldown.rs:572` to route wheel events through nested ScrollAreas.
3. **Resizable table columns** — large refactor: swap `egui::Grid` for `egui_extras::TableBuilder` in the vendored fork. Regression-testing all existing table edge cases required.
