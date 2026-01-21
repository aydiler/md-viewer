# Feature: Evidence-based Typography

**Status:** Complete
**Branch:** `feature/evidence-typography`
**Date:** 2026-01-22

## Summary

Apply research-backed typography settings derived from peer-reviewed HCI studies, WCAG accessibility standards, and vision science. Key improvements target font size, line width constraints, color contrast, paragraph spacing, and heading hierarchy.

## Research Sources

- Rello et al. (CHI 2016) - Eye-tracking study on line height
- Dyson & Haselgrove (2001) - Line length comprehension study
- WCAG 2.1 SC 1.4.12 - Accessibility requirements
- Material Design - Color contrast guidelines
- Piepenbrock et al. (2013) - Light vs dark mode accuracy

## Features

- [x] Update body font size from 14px to 16px
- [x] Implement line width constraint (55-75 CPL / ~600px)
- [x] Add evidence-based colors (off-white #F8F8F8, dark gray #333333)
- [x] Update paragraph spacing from 1.5x to 2x
- [x] Implement Major Third (1.25x) heading scale
- [x] Set code block line height to 1.3x
- [x] Update typography_recommended() preset

## Configuration Changes

| Setting | Before | After | Research Basis |
|---------|--------|-------|----------------|
| Body font size | 14px | 16px | Rello et al. (18pt+), WCAG |
| Max line width | None | ~600px | Dyson (55-66 CPL optimal) |
| Paragraph spacing | 1.5x | 2x | WCAG 1.4.12 |
| Light bg color | #FFFFFF | #F8F8F8 | Material Design (anti-halation) |
| Light text color | #000000 | #333333 | ~12:1 contrast ratio |
| Dark bg color | #000000 | #121212 | Material Design |
| Dark text color | #FFFFFF | #E0E0E0 | 87% white (Material) |
| Code line height | Inherited | 1.3x | PPIG research |

## Heading Scale (Major Third 1.25x)

| Level | Before | After | Ratio |
|-------|--------|-------|-------|
| H1 | 32px | 32px | 2x base |
| H2 | ~26px | 25.6px | 1.6x base |
| H3 | ~24px | 20px | 1.25x base |
| Body | 14px | 16px | base |

## Key Discoveries

### Code Line Height Implementation

The `code_line_height` API was defined in `TypographyConfig` but not actually applied to code blocks. Fixed by:

1. **Pass typography config through code block rendering**: Modified `CodeBlock::end()` to calculate `code_line_height` from typography config using monospace font size
2. **Update syntax highlighting functions**: Added `code_line_height: Option<f32>` parameter to `simple_highlighting`, `plain_highlighting`, and `syntax_highlighting`
3. **Apply line height to TextFormat**: Set `format.line_height = Some(line_height)` on each text section in the LayoutJob

### egui TextFormat.line_height

egui's `TextFormat::simple()` doesn't support line height directly. Must create the format manually and set `line_height` field:
```rust
let mut format = egui::TextFormat::simple(font_id, color);
format.line_height = Some(line_height);
```

### Typography Multiplier Resolution

Typography measurements use the `Measurement` enum which resolves multipliers against font size:
- Body text uses `TextStyle::Body` font size (16px)
- Code blocks use `TextStyle::Monospace` font size (14px)
- Each context resolves line height against its own font size

## Architecture

### Modified Areas

- `src/main.rs` - Font size configuration, color customization
- `egui_commonmark_backend/src/typography.rs` - Typography preset values
- `egui_commonmark_backend/src/misc.rs` - Heading size calculations, code line height
- `egui_commonmark_backend/src/elements.rs` - Spacing application

## Future Improvements

- [ ] Font selection API (specify Verdana, Georgia, etc.)
- [ ] Per-element line height configuration
- [ ] User-configurable color themes
- [ ] High-DPI aware font scaling
