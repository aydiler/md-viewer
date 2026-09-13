# Live Preview Test Doc

Click into **any block** below — it becomes an editable raw-markdown box
while everything else keeps rendering. Press `Esc` to stop editing,
`Ctrl+E` to leave Live mode, `Ctrl+S` to save.

## Paragraphs

This paragraph supports *italics*, **bold**, `inline code`, and [links](https://example.com).
Type here and watch the rendered view update when you click away.

## Lists

- First item
- Second item
  - Nested item
- Third item

Task lists render as checkboxes:

- [x] Ship Phase 0/1 (save pipeline + source mode)
- [ ] Try editing this checklist in Live mode

> Blockquotes stay atomic while editing —
> the whole quote is one editor block.

## Table

| Feature | Status | Notes |
|---------|--------|-------|
| Source mode | ✅ | Ctrl+E cycles |
| Save pipeline | ✅ | Atomic writes |
| Live preview | ✅ | This mode! |

## Code

```rust
fn main() {
    println!("code blocks are one editable unit"); 
}
```

---

Search still works in Live mode: press `Ctrl+F`, type *table*, hit Enter —
the containing block activates.
