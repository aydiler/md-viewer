# Line Height Investigation

**Date**: 2026-01-21
**Branch**: feature/fix
**Status**: ✅ Implementation complete

## Type Guidelines (Research Findings)

From `TYPOGRAPHY-RESEARCH.md`:

| Parameter | Optimal | Range | Source |
|-----------|---------|-------|--------|
| **Line height** | **1.5×** | 1.4-1.6× | WCAG 2.1 SC 1.4.12 |
| Body text | 1.5× (24px for 16px) | - | Rello et al. (2016) |
| Headings | 1.2-1.3× | - | Best practice |
| Code blocks | 1.3-1.4× | - | Best practice |

Key research:
- **WCAG 2.1 SC 1.4.12**: Mandates 1.5× line height support
- **Rello et al. (CHI 2016)**: Eye-tracking study found 0.8× and 1.8× both impair readability; 1.5× optimal
- **Reading University**: 1.5× minimizes return sweep errors

## Why Line Height Wasn't Implemented

### Core Technical Limitation

**egui_commonmark doesn't expose line-height configuration.**

From `TYPOGRAPHY-IMPLEMENTATION-PLAN.md` (Phase 4, lines 409-417):

> egui_commonmark renders markdown with its own internal layout logic. The `item_spacing.y` primarily affects gaps between egui widgets, not within the markdown content itself.

### Root Causes

1. **Library Limitation**: `CommonMarkViewer` has no `.line_height()` method
2. **Architecture Constraint**: egui's immediate-mode rendering uses font's built-in vertical metrics directly
3. **No Public API**: The library controls internal paragraph/line spacing without exposing customization

### What CAN Be Changed

Via egui's `style.spacing.item_spacing.y`:
- Spacing between egui widgets (buttons, panels)
- NOT the line spacing within markdown paragraphs

Via `CommonMarkViewer`:
- `max_image_width()`
- `indentation_spaces()`
- `syntax_theme_dark()` / `syntax_theme_light()`
- NO line height control

## Options for Implementation

### Option 1: Accept Font Defaults (Easy)
- Use fonts with generous built-in leading (Inter, Noto Sans)
- Increase `item_spacing.y` for better widget spacing
- Accept that intra-paragraph line-height is font-determined

**Pros**: No library changes, immediate implementation
**Cons**: No true line-height control

### Option 2: Fork egui_commonmark (Medium)
- Fork the library
- Modify paragraph rendering to accept line-height parameter
- Maintain fork long-term

**Pros**: Full control
**Cons**: Maintenance burden, version drift

### Option 3: Custom Markdown Renderer (Hard)
- Use `pulldown_cmark` to parse markdown AST
- Render each paragraph as a separate egui widget
- Full control over spacing between widgets

**Pros**: Complete control, no fork needed
**Cons**: Significant code, lose egui_commonmark features (syntax highlighting integration, etc.)

### Option 4: Embed Custom Font with Modified Metrics (Hacky)
- Modify a font file's vertical metrics (OS/2 table)
- Embed the modified font
- Font's built-in "line gap" would be larger

**Pros**: Works within existing constraints
**Cons**: Font licensing issues, fragile

## Current Workarounds in typography-guidelines Branch

The existing plan (Phase 4) accepts the limitation and focuses on:
1. Choosing fonts with good built-in leading
2. Maximizing `item_spacing.y` from 4px to 12px
3. Increasing body font to 16px (larger text = proportionally larger spacing)

## Decision: Fork egui_commonmark

**Selected approach**: Fork egui_commonmark and add line-height parameter

---

## Fork Implementation Plan

### Upstream Details

- **Repository**: https://github.com/lampsitter/egui_commonmark
- **Current version**: 0.22
- **License**: MIT/Apache-2.0

### Architecture Overview

egui_commonmark is a workspace with three crates:
1. **egui_commonmark** - Main library with `CommonMarkViewer` builder
2. **egui_commonmark_backend** - Shared rendering logic (pulldown-cmark → egui)
3. **egui_commonmark_macros** - Compile-time markdown evaluation

### Key Technical Insight

egui already has `TextFormat.line_height: Option<f32>` - it's just not exposed by egui_commonmark's API.

```rust
// egui::TextFormat (already exists)
pub struct TextFormat {
    pub line_height: Option<f32>,  // ← This exists, just not wired up
    // ...
}
```

### Required Modifications

#### 1. CommonMarkViewer (egui_commonmark/src/lib.rs)

```rust
pub struct CommonMarkViewer {
    // ... existing fields ...
    line_height: Option<f32>,  // NEW
}

impl CommonMarkViewer {
    /// Set line height as a multiplier of font size (e.g., 1.5 for 150%)
    pub fn line_height(mut self, height: f32) -> Self {
        self.line_height = Some(height);
        self
    }
}
```

#### 2. Backend Rendering (egui_commonmark_backend/src/lib.rs)

When processing `Event::Text` in paragraphs:
- Pass `line_height` through the rendering pipeline
- Apply to `TextFormat` when creating `LayoutJob` or `RichText`

#### 3. Cargo.toml Change

```toml
# Before
egui_commonmark = { version = "0.22", features = [...] }

# After (git dependency)
egui_commonmark = { git = "https://github.com/<user>/egui_commonmark", branch = "line-height", features = [...] }
```

### Usage After Fork

```rust
CommonMarkViewer::new()
    .line_height(1.5)  // NEW: 1.5× line height
    .max_image_width(Some(800))
    .show(ui, &mut self.cache, &self.content);
```

---

## Implementation Plan (Approved)

### Decisions Made

| Decision | Choice |
|----------|--------|
| Fork location | **Vendor in-repo** (`crates/egui_commonmark/`) |
| API design | **Both options** - multiplier AND absolute pixels |
| Extra features | **All spacing controls** |

### New API Methods

```rust
CommonMarkViewer::new()
    // Line height
    .line_height(1.5)           // Multiplier (1.5× font size)
    .line_height_px(24.0)       // Absolute pixels

    // Paragraph spacing
    .paragraph_spacing(1.5)     // Multiplier (1.5× font size)
    .paragraph_spacing_px(24.0) // Absolute pixels

    // Heading spacing
    .heading_spacing_above(2.0) // Multiplier before headings
    .heading_spacing_below(0.5) // Multiplier after headings
    // Or absolute:
    .heading_spacing_above_px(32.0)
    .heading_spacing_below_px(8.0)

    .show(ui, &mut self.cache, &self.content);
```

### Project Structure

```
markdown-viewer/worktrees/fix/
├── Cargo.toml                    # Points to local crate
├── crates/
│   └── egui_commonmark/          # Vendored fork
│       ├── Cargo.toml
│       ├── egui_commonmark/      # Main crate
│       ├── egui_commonmark_backend/
│       └── egui_commonmark_macros/
└── src/
    └── main.rs
```

### Cargo.toml Change

```toml
# Before
egui_commonmark = { version = "0.22", features = [...] }

# After
egui_commonmark = { path = "crates/egui_commonmark/egui_commonmark", features = [...] }
```

### Implementation Steps

1. **Vendor the source**
   - Clone egui_commonmark v0.22 into `crates/egui_commonmark/`
   - Update workspace Cargo.toml for local dependency

2. **Add spacing struct**
   ```rust
   pub struct TypographyConfig {
       pub line_height: Option<LineHeight>,
       pub paragraph_spacing: Option<Spacing>,
       pub heading_spacing_above: Option<Spacing>,
       pub heading_spacing_below: Option<Spacing>,
   }

   pub enum LineHeight {
       Multiplier(f32),  // 1.5 = 150% of font size
       Pixels(f32),      // 24.0 = 24px
   }

   pub enum Spacing {
       Multiplier(f32),
       Pixels(f32),
   }
   ```

3. **Add builder methods to CommonMarkViewer**
   - `line_height(f32)` / `line_height_px(f32)`
   - `paragraph_spacing(f32)` / `paragraph_spacing_px(f32)`
   - `heading_spacing_above(f32)` / `heading_spacing_above_px(f32)`
   - `heading_spacing_below(f32)` / `heading_spacing_below_px(f32)`

4. **Modify backend rendering**
   - Pass typography config through render pipeline
   - Apply `TextFormat.line_height` for paragraphs
   - Add `ui.add_space()` calls for paragraph/heading spacing

5. **Update markdown-viewer to use new API**
   ```rust
   CommonMarkViewer::new()
       .line_height(1.5)
       .paragraph_spacing(1.5)
       .heading_spacing_above(2.0)
       .heading_spacing_below(0.5)
       .max_image_width(Some(800))
       .show(ui, &mut self.cache, &self.content);
   ```

6. **Test with sample documents**
   - Long documents
   - Various heading levels
   - Mixed content (paragraphs, code, lists, images)

### Default Values (from research)

| Parameter | Default | Source |
|-----------|---------|--------|
| Line height | 1.5× | WCAG 2.1 SC 1.4.12 |
| Paragraph spacing | 1.5× font size | Chaparro et al. |
| Heading spacing above | 2.0× font size | Typography best practice |
| Heading spacing below | 0.5× font size | Typography best practice |

### Maintenance Notes

- Vendored fork means manual updates when upstream releases new versions
- Keep modifications minimal and well-documented for easier rebasing
- Consider upstreaming changes if stable

---

## Implementation Complete

### Files Modified/Added

**New Files:**
- `crates/egui_commonmark/` - Vendored fork of egui_commonmark v0.22
- `crates/egui_commonmark/egui_commonmark_backend/src/typography.rs` - Typography types

**Modified Files:**
- `Cargo.toml` - Changed to path dependency
- `crates/egui_commonmark/Cargo.toml` - Fixed Rust edition/version
- `crates/egui_commonmark/egui_commonmark_backend/src/lib.rs` - Export typography module
- `crates/egui_commonmark/egui_commonmark_backend/src/misc.rs` - Added typography to CommonMarkOptions
- `crates/egui_commonmark/egui_commonmark_backend/src/elements.rs` - Added spacing helper functions
- `crates/egui_commonmark/egui_commonmark/src/lib.rs` - Added builder methods
- `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` - Applied typography in rendering
- `src/main.rs` - Enabled typography settings

### API Summary

```rust
// All new builder methods on CommonMarkViewer:
.line_height(1.5)              // Line height as multiplier
.line_height_px(24.0)          // Line height as pixels
.paragraph_spacing(1.5)        // Paragraph spacing as multiplier
.paragraph_spacing_px(24.0)    // Paragraph spacing as pixels
.heading_spacing_above(2.0)    // Heading above spacing as multiplier
.heading_spacing_above_px(32.0)// Heading above spacing as pixels
.heading_spacing_below(0.5)    // Heading below spacing as multiplier
.heading_spacing_below_px(8.0) // Heading below spacing as pixels
.typography_recommended()      // Apply all recommended defaults at once
```

### Testing

Tested with sample markdown document containing:
- Multiple paragraphs
- Various heading levels (H1-H4)
- Lists, blockquotes, code blocks
- Bold and italic text

All typography settings apply correctly:
- ✅ Line height (1.5×) improves text readability
- ✅ Paragraph spacing creates clear visual separation
- ✅ Heading spacing adds hierarchy and breathing room
- ✅ Headings use tighter line height (scaled down from body)
