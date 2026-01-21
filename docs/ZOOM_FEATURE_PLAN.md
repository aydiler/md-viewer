# Zoom Feature Implementation Plan

## Overview

Add the ability to zoom in/out (scale text larger/smaller) using **Ctrl + Mouse Wheel**.

## Current State Analysis

- Single-file Rust application (~616 lines) in `src/main.rs`
- Uses `eframe 0.33` / `egui 0.33` for GUI
- State persistence already implemented via `PersistedState` struct
- Input handling exists in `update()` method for keyboard shortcuts
- Current shortcuts: Ctrl+O (open), Ctrl+W (watch), Ctrl+D (dark mode), Ctrl+Q (quit)

## Implementation Approach

### Option A: Global UI Scale (Recommended)

Use `ctx.set_pixels_per_point()` to scale the entire UI uniformly.

**Pros:**
- Simple implementation (single API call)
- Scales everything consistently (text, images, UI elements)
- Native egui approach

**Cons:**
- Scales entire UI, not just markdown content

### Option B: Font-Only Scaling

Modify text styles via `ctx.style_mut()` to only scale fonts.

**Pros:**
- Only affects text size
- UI chrome stays at fixed size

**Cons:**
- More complex implementation
- May cause layout issues with mixed content
- Code blocks and images won't scale

**Decision:** Use **Option A** for simplicity and consistency.

## Implementation Steps

### 1. Add Zoom State to `MarkdownApp`

```rust
struct MarkdownApp {
    // ... existing fields ...
    zoom_level: f32,  // 1.0 = 100%, range: 0.5 to 3.0
}
```

Default: `1.0` (100%)
Range: `0.5` (50%) to `3.0` (300%)
Step: `0.1` per scroll tick

### 2. Persist Zoom Level in `PersistedState`

```rust
#[derive(Serialize, Deserialize, Default)]
struct PersistedState {
    dark_mode: Option<bool>,
    last_file: Option<PathBuf>,
    zoom_level: Option<f32>,  // NEW
}
```

### 3. Handle Ctrl + Mouse Wheel Input

In `update()`, inside the `ctx.input()` block:

```rust
ctx.input(|i| {
    // Ctrl + scroll wheel for zoom
    if i.modifiers.ctrl {
        let scroll_delta = i.raw.scroll_delta.y;
        if scroll_delta != 0.0 {
            let zoom_step = 0.1;
            let delta = if scroll_delta > 0.0 { zoom_step } else { -zoom_step };
            self.zoom_level = (self.zoom_level + delta).clamp(0.5, 3.0);
        }
    }
});
```

**Note:** Must consume the scroll event when Ctrl is held to prevent simultaneous scrolling.

### 4. Apply Zoom Level

At the start of `update()`, apply the zoom:

```rust
ctx.set_pixels_per_point(self.zoom_level * ctx.native_pixels_per_point().unwrap_or(1.0));
```

Or simpler approach using egui's zoom factor:

```rust
ctx.set_zoom_factor(self.zoom_level);
```

### 5. Add Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Ctrl + Mouse Wheel | Zoom in/out |
| Ctrl + Plus/= | Zoom in |
| Ctrl + Minus | Zoom out |
| Ctrl + 0 | Reset zoom to 100% |

```rust
// Ctrl+Plus or Ctrl+=: Zoom in
if i.modifiers.ctrl && (i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals)) {
    self.zoom_level = (self.zoom_level + 0.1).min(3.0);
}
// Ctrl+Minus: Zoom out
if i.modifiers.ctrl && i.key_pressed(egui::Key::Minus) {
    self.zoom_level = (self.zoom_level - 0.1).max(0.5);
}
// Ctrl+0: Reset zoom
if i.modifiers.ctrl && i.key_pressed(egui::Key::Num0) {
    self.zoom_level = 1.0;
}
```

### 6. Add View Menu Items

```rust
ui.menu_button("View", |ui| {
    // ... existing dark mode toggle ...

    ui.separator();

    if ui.add(egui::Button::new("Zoom In").shortcut_text("Ctrl++")).clicked() {
        self.zoom_level = (self.zoom_level + 0.1).min(3.0);
        ui.close();
    }
    if ui.add(egui::Button::new("Zoom Out").shortcut_text("Ctrl+-")).clicked() {
        self.zoom_level = (self.zoom_level - 0.1).max(0.5);
        ui.close();
    }
    if ui.add(egui::Button::new("Reset Zoom").shortcut_text("Ctrl+0")).clicked() {
        self.zoom_level = 1.0;
        ui.close();
    }
});
```

### 7. Show Zoom Level Indicator (Optional)

In the menu bar's right-aligned section, show current zoom:

```rust
if (self.zoom_level - 1.0).abs() > 0.01 {
    ui.label(
        egui::RichText::new(format!("{}%", (self.zoom_level * 100.0).round() as i32))
            .small()
            .color(ui.visuals().weak_text_color())
    );
    ui.separator();
}
```

## Files to Modify

1. `src/main.rs` - All changes in single file

## Code Locations

| Change | Location |
|--------|----------|
| Add `zoom_level` field | `MarkdownApp` struct (line ~61) |
| Add to persisted state | `PersistedState` struct (line ~18) |
| Initialize zoom | `MarkdownApp::new()` (line ~78) |
| Save zoom | `App::save()` (line ~306) |
| Handle Ctrl+scroll | `App::update()` input block (line ~344) |
| Handle keyboard shortcuts | `App::update()` input block (line ~344) |
| Apply zoom | `App::update()` after theme (line ~326) |
| View menu items | `App::update()` View menu (line ~433) |
| Zoom indicator | Menu bar right section (line ~442) |

## Testing Plan

1. Build and run: `cargo run`
2. Test Ctrl + scroll up → zoom in
3. Test Ctrl + scroll down → zoom out
4. Test Ctrl+Plus, Ctrl+Minus, Ctrl+0
5. Verify zoom persists after restart
6. Verify zoom stays within bounds (50%-300%)
7. Test with different DPI settings
8. Verify no conflicts with normal scrolling

## Edge Cases

- Scroll delta varies by platform/input device
- System DPI scaling interaction
- Very small or very large zoom levels may affect layout
- Ensure smooth scrolling still works when not holding Ctrl

## Estimated Changes

- ~50 lines of new code
- ~5 lines of modified code
- No new dependencies
