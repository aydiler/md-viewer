# Side Panel Demo

A sample document for trying the **side-panel drag behavior** in this build.

Grab the separator between this text and the *Outline* panel on the right —
you can now catch it comfortably from either side, even right next to this
text column. Double-clicking a separator resets the panel to 200 px, and your
widths survive an app restart.

## Getting started

1. Drag the **right separator** (between content and Outline).
2. Drag the **left separator** (between Explorer and content).
3. Hover a separator: the line highlights and the cursor changes.
4. Double-click a separator to snap back to the default width.

> Tip: widths are clamped — Explorer 150–300 px, Outline 120–400 px.

## Sections for the outline

### Subsection A

The outline on the right lists every heading below. Click one to jump here.

### Subsection B

Text selection still works everywhere else in the document — presses near the
separator now belong to the resize handle instead.

## Formatting showcase

Some inline styles: **bold**, *italic*, `inline code`, and a [link](https://github.com/aydiler/md-viewer).

```rust
fn main() {
    println!("Hello from the markdown viewer!");
}
```

| Panel   | Min | Max | Reset |
|---------|-----|-----|-------|
| Explorer| 150 | 300 | 200   |
| Outline | 120 | 400 | 200   |

## More headings

### Deep dive one

More scrollable content so the vertical scrollbar appears next to the
separator — dragging the separator next to the scrollbar is exactly the case
this build fixes.

### Deep dive two

Resize while scrolled and watch the layout reflow smoothly.

#### Level four

Nested heading for the outline hierarchy.

### Deep dive three

That's all — enjoy the smoother separators!
