# Feature: Bold Markdown Rendering

**Status:** 🚧 In Progress
**Branch:** `fix/39-bold-rendering`
**Date:** 2026-07-04
**Lines Changed:** +239 / -8 in renderer and app font setup before docs

## Summary

Issue #39 reported that `**bold**` markdown rendered with no visible bold weight. The fix keeps generic `egui_commonmark` behavior safe by default, then lets md-viewer opt into a registered `MarkdownStrong` font family backed by a real bold font face when available.

## Features

- [x] Added opt-in `CommonMarkViewer::use_strong_font_family(true)` for strong markdown spans.
- [x] Exported `STRONG_FONT_FAMILY` so callers can register the named family before enabling the option.
- [x] Registered md-viewer's `MarkdownStrong` family during font setup, preferring Noto Sans Bold and falling back to existing proportional fonts.
- [x] Preserved inline-code font family for `**\`code\`**` spans.
- [x] Added backend unit coverage for safe default behavior, opt-in strong font selection, and strong inline code.

## Key Discoveries

### `RichText::strong()` is not enough for visible bold everywhere

`Style::to_richtext` already called `RichText::strong()` for `Tag::Strong`, but md-viewer's rendered output could still look unchanged when egui used the same regular font face. Selecting a distinct named font family makes markdown strong spans produce a visible formatting difference in md-viewer.

```rust
if self.strong {
    rich_text = rich_text.strong();
    if use_strong_font_family && !self.code {
        rich_text = rich_text.font(egui::FontId::new(
            selected_font_size,
            egui::FontFamily::Name(STRONG_FONT_FAMILY.into()),
        ));
    }
}
```

### Strong-font override must be opt-in for library safety

A first-pass renderer-only fix would have made every `egui_commonmark` consumer emit the md-viewer-specific named font family. That can panic or render poorly for consumers that never registered the family. `CommonMarkOptions::use_strong_font_family` defaults to `false`; md-viewer enables it only after `setup_fonts` registers `MarkdownStrong`.

```rust
CommonMarkViewer::new()
    .use_strong_font_family(true)
    .show_scrollable(tab.id, ui, &mut tab.cache, &tab.content);
```

### Strong inline code must keep monospace

Inline code calls `RichText::code()` later in the style path. The strong-family override skips `self.code`, so `**\`code\`**` keeps the same monospace family as normal inline code instead of switching to proportional bold.

## Architecture

### Modified Renderer API

| Function / field | Purpose |
|------------------|---------|
| `CommonMarkViewer::use_strong_font_family(bool)` | Opt into using the registered strong font family for markdown strong spans. |
| `CommonMarkOptions::use_strong_font_family` | Carries the opt-in through the renderer backend; defaults to `false`. |
| `STRONG_FONT_FAMILY` | Shared font-family name (`MarkdownStrong`) exported by the vendored renderer. |
| `Style::to_richtext_with_options` | Applies typography and strong-font options through one formatting path. |

### Modified App Font Setup

`src/main.rs` adds `setup_strong_font_family`, which tries known Noto Sans Bold paths, registers `MarkdownStrong`, and appends existing proportional fallbacks so bold text still covers broad Unicode. The family is registered even without a bold face to avoid runtime font lookup failures on systems missing Noto Sans Bold.

## Testing Notes

Validation run on 2026-07-04 in `/home/akiro/Coding/md-viewer-fix-39-bold-rendering`:

```bash
cargo test -p egui_commonmark_backend_extended strong --lib
# 3 passed; 0 failed

cargo test -p md-viewer
# 29 passed; 0 failed

cargo clippy --all-targets --all-features
# finished successfully; existing warnings remain:
# - unused variable `max_width` in egui_commonmark_backend/src/elements.rs
# - deprecated `Ui::allocate_ui_at_rect` in parsers/pulldown.rs

git diff --check
# no output
```

Independent verification passed on 2026-07-04 via `verification` agent. Local spot-checks reran backend strong tests, md-viewer unit tests, and `git diff --check` with matching pass results.

## Future Improvements

- [ ] Add platform-specific strong-font candidates for Windows/macOS while addressing issue #40.
- [ ] Consider a visual/snapshot-style rendering test if md-viewer gains stable UI snapshot infrastructure.
