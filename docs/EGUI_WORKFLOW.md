# egui Development Workflow

This document defines the canonical workflow for implementing features in egui applications. **Read this before starting any feature work.**

## Core Principle: Immediate Mode UI

egui redraws the entire UI every frame (~60 FPS). This means:
- UI code runs 60+ times per second
- **NEVER** do expensive work inside `ui()` functions
- **ALWAYS** cache computed results
- State changes trigger automatic repaints

---

## The Four-Phase Pattern

Every feature follows: **State → Logic → UI → Async**

### Phase 1: State Management

Define state structs with cached data. Add to main App struct.

```rust
// GOOD: State struct with pre-computed data
struct OutlineState {
    headers: Vec<Header>,           // Cached headers (parsed once)
    document_title: Option<String>, // Cached title
    expanded: HashSet<usize>,       // UI state
}

// BAD: Parsing on every frame
fn ui(&mut self, ui: &mut egui::Ui) {
    let headers = parse_headers(&self.content); // DON'T DO THIS
}
```

**Checklist:**
- [ ] Define struct for feature state
- [ ] Add to `MarkdownApp` or `Tab` struct
- [ ] Initialize in `Default` impl or constructor
- [ ] Store computed data, not raw inputs

### Phase 2: Logic Layer

Create parsing/processing functions OUTSIDE ui code. Call only on data change.

```rust
// GOOD: Parse once when file loads
impl Tab {
    fn load_file(&mut self, path: &Path) {
        self.content = fs::read_to_string(path)?;

        // Parse headers ONCE, cache result
        let parsed = parse_headers(&self.content);
        self.outline.headers = parsed.headers;
        self.outline.document_title = parsed.title;

        // Reset cache on content change
        self.cache = CommonMarkCache::default();
    }
}

// GOOD: Separate parsing function
fn parse_headers(content: &str) -> ParsedHeaders {
    // Heavy regex work happens here, once
    let header_re = Regex::new(r"(?m)^(#{1,6})\s+(.+)$").unwrap();
    // ...
}
```

**Checklist:**
- [ ] Create functions in impl block (not in ui closure)
- [ ] Return owned data structures
- [ ] Call only when input data changes
- [ ] Never call from inside `show()` or render loop

### Phase 3: UI Layer

Read from cached state only. Never block the render loop.

```rust
// GOOD: UI reads cached state
fn render_outline(&mut self, ui: &mut egui::Ui) {
    // Read from pre-parsed cache
    for header in &self.tab.outline.headers {
        let indent = (header.level - 1) * 8;
        ui.horizontal(|ui| {
            ui.add_space(indent as f32);
            if ui.selectable_label(false, &header.title).clicked() {
                // Set a flag, don't do heavy work here
                self.pending_scroll_to = Some(header.position);
            }
        });
    }
}

// BAD: Heavy work in UI code
fn render_outline(&mut self, ui: &mut egui::Ui) {
    for line in self.content.lines() {
        if line.starts_with('#') {
            // Parsing 60 times per second!
        }
    }
}
```

**Checklist:**
- [ ] Read from cached state only
- [ ] Set flags for actions (don't execute immediately)
- [ ] Keep render functions pure (no I/O, no heavy computation)
- [ ] Handle flag-triggered actions AFTER ui rendering

### Phase 4: Async (if needed)

Use channels for background work. Poll results in update loop.

```rust
// For file watching, network requests, etc.
struct MarkdownApp {
    // Channel receiver for async events
    watcher_rx: Option<Receiver<DebouncedEvent>>,
}

fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    // Poll async channel (non-blocking)
    if let Some(rx) = &self.watcher_rx {
        if let Ok(event) = rx.try_recv() {
            self.handle_file_change(event);
        }
    }

    // Request periodic repaints when watching
    if self.watch_enabled {
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    // ... rest of UI rendering
}
```

**Checklist:**
- [ ] Use `mpsc::channel` for thread communication
- [ ] Poll with `try_recv()` (never block)
- [ ] Call `request_repaint_after()` for periodic checks
- [ ] Handle disconnection gracefully

---

## Common Patterns

### Pattern: Lazy-Loaded Content

```rust
struct Tab {
    content: String,
    // None = not yet parsed, Some = cached result
    cached_layout: Option<Vec<LayoutJob>>,
}

impl Tab {
    fn get_layout(&mut self) -> &[LayoutJob] {
        if self.cached_layout.is_none() {
            self.cached_layout = Some(self.compute_layout());
        }
        self.cached_layout.as_ref().unwrap()
    }

    fn invalidate_cache(&mut self) {
        self.cached_layout = None;
    }
}
```

### Pattern: Deferred Actions

```rust
fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    // Collect actions during UI rendering
    let mut action: Option<Action> = None;

    egui::CentralPanel::default().show(ctx, |ui| {
        if ui.button("Open File").clicked() {
            action = Some(Action::OpenFile);
        }
    });

    // Execute actions AFTER UI rendering
    match action {
        Some(Action::OpenFile) => self.open_file_dialog(),
        Some(Action::CloseTab(idx)) => self.close_tab(idx),
        None => {}
    }
}
```

### Pattern: Per-Tab State

```rust
struct Tab {
    id: egui::Id,
    path: PathBuf,
    content: String,
    cache: CommonMarkCache,  // MUST persist across frames
    scroll_offset: f32,
    history_back: Vec<PathBuf>,
    history_forward: Vec<PathBuf>,
}

// Each tab is independent - no shared mutable state
```

### Pattern: Viewport-Based Rendering

```rust
// For large documents, only render visible portion
ScrollArea::vertical()
    .show_viewport(ui, |ui, viewport| {
        // viewport.min.y = top of visible area
        // viewport.max.y = bottom of visible area

        CommonMarkViewer::new()
            .show(ui, &mut self.cache, &self.content);

        // Track scroll offset for outline sync
        self.scroll_offset = viewport.min.y;
    });
```

---

## Anti-Patterns (Don't Do These)

### 1. Parsing in Render Loop
```rust
// BAD
fn ui(&mut self, ui: &mut egui::Ui) {
    let links = extract_links(&self.content); // 60x per second!
}

// GOOD
fn load_file(&mut self) {
    self.links = extract_links(&self.content); // Once
}
```

### 2. Recreating Cache Per Frame
```rust
// BAD
fn ui(&mut self, ui: &mut egui::Ui) {
    let mut cache = CommonMarkCache::default(); // Breaks rendering!
    CommonMarkViewer::new().show(ui, &mut cache, &content);
}

// GOOD
struct App {
    cache: CommonMarkCache, // Persists across frames
}
```

### 3. Blocking I/O in UI
```rust
// BAD
fn ui(&mut self, ui: &mut egui::Ui) {
    if ui.button("Save").clicked() {
        fs::write(&path, &content).unwrap(); // Blocks UI thread
    }
}

// GOOD: Use channel for async I/O or do after render loop
```

### 4. Allocating in Hot Path
```rust
// BAD
fn ui(&mut self, ui: &mut egui::Ui) {
    let formatted = format!("{} lines", self.content.lines().count());
}

// GOOD: Cache formatted strings if they don't change often
```

---

## Feature Implementation Checklist

Before starting a feature:

- [ ] Read `docs/LESSONS.md` for relevant gotchas
- [ ] Check `git log -5` for recent changes
- [ ] Read files you'll modify (don't rely on memory)
- [ ] Create devlog: `docs/devlog/NNN-feature-name.md`

During implementation:

- [ ] **Phase 1:** Define state struct with cached data
- [ ] **Phase 2:** Create parsing/logic functions outside UI
- [ ] **Phase 3:** UI reads cached state, sets action flags
- [ ] **Phase 4:** Handle async via channels if needed
- [ ] Test after each phase (incremental changes)

After implementation:

- [ ] Run `cargo clippy` - no warnings
- [ ] Update devlog with discoveries
- [ ] Add gotchas to `docs/LESSONS.md`
- [ ] Verify no regressions in existing features

---

## Quick Reference

| Do | Don't |
|----|-------|
| Parse data in `load_file()` | Parse data in `ui()` |
| Store cache in struct | Create cache per frame |
| Use `try_recv()` for channels | Use blocking `recv()` |
| Set flags in UI, act after | Act immediately in UI |
| Use `show_viewport` for large docs | Render everything always |
| Read files before editing | Assume you know the contents |

---

## Related Documentation

- `docs/ARCHITECTURE.md` - Component structure and rendering flow
- `docs/LESSONS.md` - Gotchas and non-obvious fixes
- `.claude/rules/refactoring-rules.md` - Incremental change guidelines
- `.claude/rules/context-awareness.md` - Pre-coding checklist
