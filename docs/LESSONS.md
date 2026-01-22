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
