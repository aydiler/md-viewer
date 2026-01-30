# egui Immediate Mode Patterns

## Core Rule: Never Parse in UI Code

egui runs UI code 60+ times per second. Heavy work belongs in load/change handlers.

```rust
// BAD: Parsing every frame
fn ui(&mut self, ui: &mut egui::Ui) {
    let headers = parse_headers(&self.content); // DON'T
}

// GOOD: Parse once on file load
fn load_file(&mut self, path: &Path) {
    self.content = fs::read_to_string(path)?;
    self.headers = parse_headers(&self.content); // Cache result
}
```

## The Four Phases

1. **State**: Define struct with cached data
2. **Logic**: Parse/compute outside UI, call on data change only
3. **UI**: Read cached state, set flags for actions
4. **Async**: Use channels, poll with `try_recv()`

## Critical: CommonMarkCache Must Persist

```rust
struct Tab {
    cache: CommonMarkCache, // NEVER recreate per frame
}

fn load_file(&mut self) {
    self.cache = CommonMarkCache::default(); // Reset only on file load
}
```

## Pattern: Deferred Actions

```rust
fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    let mut action = None;

    egui::CentralPanel::default().show(ctx, |ui| {
        if ui.button("Close").clicked() {
            action = Some(Action::Close); // Set flag, don't act
        }
    });

    // Act AFTER UI rendering
    if let Some(Action::Close) = action {
        self.close_tab();
    }
}
```

See `docs/EGUI_WORKFLOW.md` for complete guide.
