# Fix: List-item fenced code block layout

**Status:** Complete
**Branch:** `fix/44-list-blocks`
**Date:** 2026-07-12
**Files Changed:** renderer row boundaries, geometry regression tests, and documentation

## Summary

Issue #44 exposed fenced code blocks overlapping adjacent text when rendered inside list items. List-item content uses egui horizontal wrapped rows, while fenced code renders as a block widget. Without explicit row boundaries, text before the fence, the code block, or text after the fence could share layout space.

The renderer now ends the active list row immediately before and after each fenced code block. Top-level code blocks retain their existing path because row boundaries are conditional on active list state.

## Implementation

- End list-item row before starting a code block.
- Render the code block through the existing `end_code_block` path.
- End list-item row after the block so following item text starts below it.
- Add painted-shape geometry assertions covering unordered, ordered, code-only, repeated, nested, and top-level-control cases.

## Key Discovery

Markdown indentation alone does not always create a new semantic block. This fixture ending:

```markdown
    NESTED_AFTER
  OUTER_AFTER
```

parses both lines as one nested paragraph separated by a soft break. Equal Y placement is therefore valid. Making `OUTER_AFTER` an outer sibling list item:

```markdown
    NESTED_AFTER
- OUTER_AFTER
```

expresses the intended block transition without forcing soft breaks apart in renderer code.

## Coding Style And Technique Rationale

The fix stays inside existing immediate-mode list state and uses `ui.end_row()` at the two block boundaries. This is smaller and clearer than introducing special paragraph parsing or post-layout correction. Conditional checks preserve top-level behavior and avoid changing soft-break semantics.

Geometry tests inspect final-pass painted text rectangles rather than only response height. Strict top-to-bottom comparisons directly detect overlap at both code-block edges. Reusing one persistent `CommonMarkCache` across two passes matches production behavior and lets egui settle font/layout caches before assertions.

## RED / GREEN Evidence

- Baseline renderer with corrected fixture: `list_code_block_uses_separate_rows` RED because `OUTER_BEFORE` overlaps `OUTER_CODE`.
- Focused corrected nested test: GREEN, 1 passed.
- Full wrapping suite: GREEN, 12 passed.

## Validation

Run from vendored workspace via explicit manifest path:

```bash
cargo test --manifest-path crates/egui_commonmark/Cargo.toml -p egui_commonmark_extended --test wrapping nested_list_code_blocks_keep_order_and_deeper_indentation
cargo test --manifest-path crates/egui_commonmark/Cargo.toml -p egui_commonmark_extended --test wrapping
```

Root formatting, root tests, clippy, and diff checks were run before completion. Vendored `cargo fmt --all --check` also ran but reported repository-wide pre-existing formatting drift across untouched files; no unrelated mass formatting was applied.

## Impact

Fenced code blocks inside list items now occupy dedicated rows, preserving order and nested indentation. Markdown soft breaks remain unchanged. No public API or persisted-state changes.
