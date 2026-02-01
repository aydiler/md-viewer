# Lessons Learned

Reusable fixes and non-obvious solutions discovered during development. Check here before debugging similar issues.

---

## egui_commonmark

### Link hooks reset automatically
**Context:** Implementing link navigation
**Gotcha:** Hooks registered with `cache.add_link_hook()` reset to `false` before each `show()` call automatically. No need to manually reset them.
**Files:** `src/main.rs`

### Anchor-only links cause browser errors
**Context:** Links like `#section` passed to browser open `file:///path/#section` and fail
**Fix:** Register hooks for anchor-only links too, but ignore them in navigation handler
```rust
// Register ALL local links including anchors
cache.add_link_hook("#section");
// In navigate_to_link(), skip if path starts with #
```
**Files:** `src/main.rs`

### Line height not exposed by default
**Context:** Wanted to set 1.5x line height for readability
**Problem:** egui_commonmark doesn't expose `line_height` configuration despite egui's `TextFormat.line_height` existing
**Fix:** Fork egui_commonmark and wire up the API
**Files:** `crates/egui_commonmark/`

### TextFormat::simple() ignores line height
**Context:** Setting line height on code blocks
**Problem:** `TextFormat::simple(font_id, color)` doesn't support line height
**Fix:** Create format manually:
```rust
let mut format = egui::TextFormat::simple(font_id, color);
format.line_height = Some(line_height);  // Must set explicitly
```
**Files:** `crates/egui_commonmark/egui_commonmark_backend/src/misc.rs`

### default_width overflows at narrow widths
**Context:** Text was cut off instead of wrapping when window narrowed below 600px
**Problem:** `CommonMarkOptions::max_width()` returned `default_width` even when larger than `available_width`
**Fix:** Cap the width at `available_width`:
```rust
// Before (buggy):
if default_width as f32 > max_width { default_width as f32 }

// After (fixed):
(default_width as f32).min(available_width)
```
**Files:** `crates/egui_commonmark/egui_commonmark_backend/src/misc.rs`

### Line ratio scroll calculation is inaccurate
**Context:** Clicking outline headers scrolled to wrong position
**Problem:** Using `line_number / content_lines * content_height` assumes linear relationship between line count and rendered height, but markdown has variable heights (headers, code blocks, spacing).
**Fix:** Track actual header positions during rendering by adding position tracking to `CommonMarkCache`:
```rust
// In CommonMarkCache - track positions
cache.set_scroll_offset(viewport.min.y);  // Before render
cache.record_header_position(title, y);   // During render
let pos = cache.get_header_position(title); // For scroll
```
**Key insight:** `ui.cursor().top()` inside `show_viewport` is viewport-relative, not content-relative. Add `viewport.min.y` to convert.
**Files:** `crates/egui_commonmark/`, `src/main.rs`

---

## egui

### CommonMarkCache must persist across frames
**Context:** Markdown rendering was flickering/broken
**Problem:** Recreating `CommonMarkCache` every frame breaks rendering state
**Fix:** Store cache in app struct, only reset on file load (not per-frame)
```rust
struct MarkdownApp {
    cache: CommonMarkCache,  // Persist this!
}
```
**Files:** `src/main.rs`

### Typography multipliers are context-dependent
**Context:** Line height of 1.5 produced different pixel values for body vs code
**Explanation:** Multipliers resolve against the font size of that context:
- Body text (16px): 1.5x = 24px
- Code blocks (14px): 1.3x = 18.2px

**Files:** `crates/egui_commonmark/egui_commonmark_backend/src/typography.rs`

### Font fallback for Unicode support
**Context:** Red triangles appearing for emojis, CJK, and non-Latin scripts
**Problem:** egui's default fonts don't include most Unicode characters
**Fix:** Load system fonts (Noto family) as fallbacks at startup:
```rust
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // Load font file
    let font_data = fs::read("/usr/share/fonts/noto/NotoSans-Regular.ttf")?;
    fonts.font_data.insert(
        "NotoSans".to_string(),
        egui::FontData::from_owned(font_data).into(), // .into() for Arc
    );
    // Add as fallback (after default fonts)
    fonts.families.get_mut(&egui::FontFamily::Proportional)
        .unwrap().push("NotoSans".to_string());
    ctx.set_fonts(fonts);
}
```
**Files:** `src/main.rs`

### egui 0.33 FontData requires Arc wrapper
**Context:** Compiler error when adding fonts
**Problem:** `fonts.font_data.insert()` expects `Arc<FontData>`, not `FontData`
**Fix:** Add `.into()` to convert:
```rust
egui::FontData::from_owned(font_data).into()
```
**Files:** `src/main.rs`

### Color emojis not supported in egui
**Context:** Emojis render as monochrome outlines despite loading NotoColorEmoji
**Root cause:** egui's font renderer (ab_glyph) doesn't support color font formats (COLR/CPAL, CBDT/CBLC)
**Workaround:** None - this is an upstream limitation. Emojis will render as simple outlines.
**Files:** `src/main.rs`

### egui 0.33 Painter API changes
**Context:** Implementing minimap with custom drawing
**Changes:**
- `screen_rect()` deprecated - use `available_rect()` or `content_rect()`
- `rect_stroke()` now requires 4th argument: `StrokeKind` (Inside, Outside, Middle)
```rust
painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Outside);
```
**Files:** `src/main.rs`

### allocate_painter for custom drawing
**Context:** Drawing minimap blocks
**Pattern:** Use `ui.allocate_painter(size, sense)` to get painter and response:
```rust
let (response, painter) = ui.allocate_painter(
    egui::vec2(width, height),
    egui::Sense::click_and_drag(),
);
painter.rect_filled(rect, rounding, color);
```
**Files:** `src/main.rs`

### Text selection clears when content leaves viewport (BY DESIGN)
**Context:** Implementing scroll-while-selecting feature
**Problem:** Text selection disappears when selected content scrolls out of view
**Root cause:** egui intentionally clears selection in `label_text_selection.rs`:
```rust
if !state.has_reached_primary || !state.has_reached_secondary {
    // We didn't see both cursors this frame - deselect to avoid glitches
    let prev_selection = state.selection.take();
}
```
**Why:** egui validates selection every frame by checking if cursor endpoints were "seen" during rendering. When labels scroll out of view, they aren't rendered, endpoints aren't reached, selection is cleared.
**Workarounds:**
- Accept limitation (selection works if content stays in viewport)
- Disable selection: `ui.style_mut().interaction.selectable_labels = false;`
- Use TextEdit instead of Labels (handles own selection state)
- Implement custom selection tracking (significant effort)
**Files:** `src/main.rs`, `docs/devlog/009-drag-scroll.md`

### Scroll during selection requires post-render state modification
**Context:** Mouse wheel scroll while selecting text
**Problem:** Setting `scroll_area.vertical_scroll_offset()` BEFORE rendering breaks selection
**Fix:** Modify scroll state AFTER `show_viewport()`:
```rust
let mut scroll_output = scroll_area.show_viewport(ui, |ui, viewport| { ... });

// Apply scroll AFTER rendering
if raw_scroll.abs() > 0.0 {
    scroll_output.state.offset.y = new_offset;
    scroll_output.state.store(ui.ctx(), scroll_output.id);
}
```
**Gotcha:** Avoid `state.store()` at scroll boundaries (offset near 0 or max) - can still break selection
**Files:** `src/main.rs`

### Resize handle and scrollbar overlap causes jitter
**Context:** Resizing outline panel caused mouse jitter when near content scrollbar
**Problem:** The SidePanel resize handle sensing area overlaps with the adjacent ScrollArea scrollbar, causing rapid switching between resize and scroll modes.
**Fix:** Combine reduced grab radius with minimal margin:
```rust
// 1. Reduce resize grab radius
style.interaction.resize_grab_radius_side = 2.0; // Default ~5.0

// 2. Add minimal right margin to content area (not visually noticeable)
egui::Frame::none()
    .inner_margin(egui::Margin { right: 3, ..Default::default() })
    .show(ui, |ui| { /* content */ });
```
**Why both?** Reduced grab radius alone isn't enough. The 3px margin provides physical separation without creating a visible gap (8px was too much and created a black gap).
**Files:** `src/main.rs`

---

## Custom Tab System

### Avoid iterating and mutating tabs simultaneously
**Context:** Rendering tab bar while handling close actions
**Problem:** Can't mutate `self.tabs` while iterating over it in a closure
**Fix:** Collect tab info (title, active state) before iterating, then process actions after:
```rust
// Collect data first
let tab_info: Vec<(String, bool)> = self.tabs.iter()
    .enumerate()
    .map(|(idx, tab)| (tab.title(), idx == self.active_tab))
    .collect();

// Then iterate over collected data
for (idx, (title, is_active)) in tab_info.iter().enumerate() {
    // Render without borrowing self.tabs
}

// Handle mutations after the loop
if let Some(idx) = tab_to_close {
    self.close_tab(idx);
}
```
**Files:** `src/main.rs`

### ui.close() replaces ui.close_menu()
**Context:** Closing context menus in egui 0.33
**Problem:** `ui.close_menu()` is deprecated
**Fix:** Use `ui.close()` instead
**Files:** `src/main.rs`

---

## notify / File Watching

### notify-debouncer-mini 0.4 requires notify 6.x
**Context:** Dependency resolution failed
**Problem:** Version mismatch between notify crate versions
**Fix:** Pin `notify = "6.1"` when using `notify-debouncer-mini = "0.4"`
**Files:** `Cargo.toml`

### File watcher can fail silently
**Context:** Live reload stopped working after system sleep
**Fix:** Implement auto-recovery with retry limit:
```rust
// Auto-recover up to 3 times on watcher failure
if watcher_error && recovery_count < 3 {
    self.setup_watcher();
    recovery_count += 1;
}
```
**Files:** `src/main.rs`

---

## Path Resolution

### Relative links need canonicalize()
**Context:** Links like `../other/file.md` weren't resolving
**Fix:** Use `canonicalize()` to resolve `../` in paths:
```rust
let target_path = current_dir.join(path_part);
let target_path = target_path.canonicalize()?;  // Resolves ../
```
**Files:** `src/main.rs`

---

## Vendoring Dependencies

### Prefer in-repo vendoring over git dependencies
**Context:** Forking egui_commonmark for typography
**Decision:** Vendor in `crates/egui_commonmark/` rather than git dependency
**Why:**
- Easier to modify and debug
- No network dependency for builds
- Clear diff of changes vs upstream
- Simpler rebasing when upstream updates

**Files:** `Cargo.toml`, `crates/`

---

## Typography Research

### WCAG 2.1 SC 1.4.12 line height requirement
**Context:** Accessibility compliance
**Standard:** Must support 1.5x line height minimum
**Source:** Rello et al. (CHI 2016) eye-tracking study confirmed 1.5x optimal

### Optimal line length is 55-75 CPL
**Context:** Setting max content width
**Finding:** ~600px max width for 16px body text
**Source:** Dyson & Haselgrove (2001)

### Off-white reduces eye strain
**Context:** Background color selection
**Fix:** Use `#F8F8F8` instead of pure white (anti-halation)
**Source:** Material Design guidelines

---

## Git Worktrees

### Branch protection via Claude Code hook
**Context:** Preventing accidental edits to main
**Solution:** Hook in `.claude/hooks/` that blocks edits on main branch
**Files:** `.claude/hooks/`

### Bare repo structure for worktrees
**Context:** Managing multiple feature branches
**Structure:**
```
~/markdown-viewer/
├── .bare/           # Git database
└── worktrees/
    ├── main/        # Main branch
    └── feature-x/   # Feature branches
```
**Benefit:** Each worktree is fully isolated with own working directory

---

## egui MCP / Virtual Display Testing

### Wayland systems ignore DISPLAY env var
**Context:** Testing egui apps on virtual X11 display (Xvfb)
**Problem:** Setting `DISPLAY=:99` doesn't work on Wayland systems - app still opens on real screen
**Root cause:** egui/winit defaults to Wayland when available, ignoring DISPLAY
**Solution:** Use `egui_launch` MCP tool which auto-detects this and forces X11 mode:
```
egui_launch({
  applicationPath: "./target/debug/app",
  env: { "DISPLAY": ":99" }  // Tool auto-adds WINIT_UNIX_BACKEND=x11
})
```
**Manual workaround:**
```bash
DISPLAY=:99 WINIT_UNIX_BACKEND=x11 WAYLAND_DISPLAY= ./app
```
**Files:** `~/.claude/CLAUDE.md`, `~/egui-mcp/`

### Inline code in headers renders incorrectly
**Context:** Headers like `### Title (`code`)` rendered garbled text
**Problem:** Each text fragment in a heading was getting its own `allocate_ui_at_rect()` call to force left alignment, causing each fragment to reset to x=0 instead of flowing inline.
**Fix:** Accumulate all heading RichText fragments, render once at end:
```rust
// Add field to accumulate fragments
current_heading_rich_texts: Vec<egui::RichText>,

// In event_text (heading case) - accumulate, don't render
self.current_heading_rich_texts.push(rich_text);

// In end_tag(Heading) - render all at once
let rich_texts = std::mem::take(&mut self.current_heading_rich_texts);
ui.allocate_ui_at_rect(heading_rect, |ui| {
    for rt in rich_texts {
        ui.label(rt);
    }
});
```
**Key insight:** Single `allocate_ui_at_rect` for left alignment + multiple labels inside for inline flow.
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`

### High CPU on virtual displays (Xvfb)
**Context:** E2E testing on Xvfb uses 500%+ CPU
**Problem:** Virtual displays lack vsync, so egui renders at unlimited FPS
**Root cause:**
1. `ctx.request_repaint()` (without delay) triggers unlimited repaints
2. Even with `request_repaint_after()`, the rendering loop runs continuously because Xvfb doesn't block on vsync
**Fix:** Detect virtual display and add explicit sleep:
```rust
// In struct
is_virtual_display: bool,

// In constructor
let is_virtual_display = std::env::var("DISPLAY")
    .map(|d| d != ":0" && d != ":0.0" && !d.is_empty())
    .unwrap_or(false);

// In update()
if self.is_virtual_display {
    std::thread::sleep(Duration::from_millis(16)); // ~60 FPS cap
}

// For MCP polling, use delay:
ctx.request_repaint_after(Duration::from_millis(50)); // NOT request_repaint()
```
**Result:** CPU drops from 500%+ to ~220%
**Files:** `src/main.rs`

### "Connection reset by peer" during egui_launch
**Context:** Bridge logs errors during MCP connection polling
**Problem:** `egui_launch` polls every 200ms to check if bridge is ready, connecting then disconnecting
**Explanation:** This is normal behavior during startup - not an actual error
**Note:** The error can be ignored; it's just polling to detect when the app is ready
**Files:** `~/egui-mcp/crates/egui-mcp-bridge/src/server.rs`
