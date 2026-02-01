# Feature: Font Fallback for Unicode Support

**Status:** ✅ Complete
**Branch:** `feature/font-fallback`
**Date:** 2026-02-01
**Lines Changed:** TBD

## Summary

Add font fallback support to properly render Unicode characters that aren't in egui's default font. Without fallbacks, missing characters appear as red triangles (egui's missing glyph indicator).

## Features

- [x] System font fallback (Noto Sans family for text)
- [x] CJK font fallback (Chinese, Japanese, Korean characters)
- [x] Arabic, Hebrew, Hindi, Thai scripts
- [x] Mathematical symbols and dingbats
- [x] Emoji support (monochrome only - egui limitation)
- [x] Graceful degradation when fonts not available

## Key Discoveries

### egui Font Loading

Fonts must be configured in `CreationContext` callback using `cc.egui_ctx.set_fonts()`.

```rust
let mut fonts = egui::FontDefinitions::default();

// Add font data
fonts.font_data.insert(
    "NotoSans".to_owned(),
    egui::FontData::from_static(include_bytes!("/path/to/font.ttf")),
);

// Add to font family as fallback
fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap()
    .push("NotoSans".to_owned());

cc.egui_ctx.set_fonts(fonts);
```

### System Font Paths (Arch Linux)

```
/usr/share/fonts/noto/NotoSans-Regular.ttf
/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc
/usr/share/fonts/noto-emoji/NotoColorEmoji.ttf
```

### Red Triangle = Missing Glyph

egui draws a red triangle when no font has a glyph for a character. Common culprits:
- Emojis (🎉, 😀, ✅)
- Non-Latin scripts (Chinese, Japanese, Arabic)
- Math symbols not in default font

### Color Emoji Limitation

egui's font renderer (ab_glyph/owned_ttf_parser) **does not support color emoji formats** (COLR/CPAL, CBDT/CBLC bitmap tables). When loading NotoColorEmoji.ttf:
- The font loads successfully
- Only monochrome fallback glyphs are rendered
- Emojis appear as simple black/white outlines

This is an upstream egui limitation, not fixable without changes to ab_glyph.

### FontData requires Arc wrapper in egui 0.33

```rust
// egui 0.33 uses Arc<FontData> not FontData directly
fonts.font_data.insert(
    font_name.to_string(),
    egui::FontData::from_owned(font_data).into(), // .into() converts to Arc
);
```

## Architecture

### New Function

| Function | Purpose |
|----------|---------|
| `setup_fonts()` | Load system fonts as fallbacks at startup |

## Testing Notes

Test with documents containing:
- Emojis: 🎉 ✅ ❌ 😀 🚀
- CJK: 你好世界 こんにちは 안녕하세요
- Special symbols: → ← ↑ ↓ • ◦ ■

## Future Improvements

- [ ] User-configurable font paths
- [ ] Font size scaling for different font families
- [ ] Embed small emoji subset for systems without Noto Emoji
