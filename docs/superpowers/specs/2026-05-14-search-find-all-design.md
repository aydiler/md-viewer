# Search And Find-All Design

## Metadata

- Date: 2026-05-14
- Issue: aydiler/md-viewer#4
- Scope: current-document search plus find-all across open tabs
- Deferred: table resizing and table horizontal-scroll usability

## Context

Issue #4 requests two feature areas: a simple search/find-all function and better large-table usability. This spec covers only the search/find-all slice. Table resizing and table scrolling remain separate work.

`md-viewer` already has tab state, per-tab content strings, line counts, estimated scroll positioning for outline navigation, and top-panel UI patterns. Search can build on those without changing the Markdown renderer in the first implementation.

## Goals

- Add a `Ctrl+F` search bar for the active document.
- Search current document as the user types.
- Show active-document match count and current match position.
- Support next/previous navigation with `Enter`, `Shift+Enter`, buttons, and menu/toolbar UI.
- Add `Find All` results across all open tabs.
- Let clicking a find-all result switch to the matching tab and scroll near the matching line.
- Keep first PR plain text and low risk.

## Non-Goals

- No regex search.
- No whole-word option.
- No workspace/file-tree search.
- No table resizing or table scroll changes.
- No renderer-level inline highlighting in first cut unless it turns out to be trivial and non-invasive.

## User Interface

Search UI appears as a top panel below the menu/tab area when opened.

Controls:

- Text input field.
- Active-document match label, e.g. `2/14` or `0 matches`.
- `Prev` button.
- `Next` button.
- `Find All` button.
- Close button.

Keyboard behavior:

- `Ctrl+F`: open search bar and focus input.
- `Enter`: move to next active-document match when search input is focused.
- `Shift+Enter`: move to previous active-document match when search input is focused.
- `Esc`: close search bar when search UI is open.

Find-all results appear in a compact panel when requested. Each row shows tab title, line number, and excerpt. Clicking a row activates that tab and scrolls near the result line.

## Data Model

Add `SearchState` to `MarkdownApp`:

- `is_open: bool`
- `query: String`
- `focus_requested: bool`
- `active_match_index: usize`
- `results: Vec<SearchMatch>`
- `show_all_results: bool`

Add `SearchMatch`:

- `tab_index: usize`
- `line_number: usize`
- `line_start_byte: usize`
- `match_start_byte: usize`
- `match_end_byte: usize`
- `excerpt: String`

The search state is app-level because find-all spans multiple open tabs. Per-tab content stays in `Tab`.

## Search Behavior

Search is case-insensitive plain text. Empty query produces no matches.

Search result rebuilding happens when:

- query changes
- tab content reloads
- tab opens/closes
- active tab switches, for current-result count/index normalization

Search scans each tab line by line, preserves 1-based line numbers, and records match byte ranges within original content. Multiple matches on the same line become separate results.

Current-document search uses the subset of `results` matching `active_tab`. Find-all displays all `results` grouped or naturally sorted by open tab order, then line order.

## Navigation And Scrolling

Next/previous search navigation only moves among matches in the active document. If no match exists, buttons are disabled and `Enter` does nothing.

Clicking find-all result:

1. Sets `active_tab` to result tab index.
2. Sets active match index to corresponding result in that tab.
3. Sets `pending_scroll_offset` using line-ratio estimate:
   - `line_number / content_lines * last_content_height`
4. Keeps search bar open and results panel visible.

This mirrors existing outline fallback behavior and avoids renderer changes.

## Error Handling

No modal errors for normal search states.

- Empty query: no results.
- No matches: display `0 matches`.
- Closed/reloaded tab invalidates stale results by rebuilding.
- Missing tab index after close: clamp active match and rebuild before rendering.

## Testing

Unit tests should cover pure search helpers:

- empty query returns no matches
- case-insensitive match
- multiple matches on one line
- line numbers are 1-based
- excerpts include matched text and stay bounded
- results across multiple tab contents preserve tab index/order

Manual smoke tests:

- `Ctrl+F` opens and focuses search.
- Typing query updates active-document count.
- `Enter` and `Shift+Enter` move through matches.
- `Find All` lists matches across open tabs.
- Clicking result switches tab and scrolls near matching line.
- `Esc` closes search UI.

Run:

```bash
cargo test --locked
cargo check --locked
```

## Documentation

Update repo docs:

- `docs/devlog/017-search-find-all.md`
- `docs/KEYBOARD_SHORTCUTS.md`

Update System Notes after implementation because project behavior changes:

- `System Notes/30-39 Projects/31-md-viewer/31.12-md-viewer-Implementation-Tracker.md`
- `System Notes/30-39 Projects/31-md-viewer/31.11-md-viewer-Runbooks.md` if validation/run commands change

## PR Scope

PR should reference issue #4 and state it implements search/find-all only. Table resizing remains later work.
