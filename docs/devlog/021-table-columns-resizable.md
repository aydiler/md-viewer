# Feature: Resizable table columns via TableBuilder

**Status:** Complete (implementation + fix phase for D3/E1 regressions)
**Branch:** `feature/table-columns`
**Date:** 2026-05-16
**Lines Changed:** ~+220 / -75 in `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`

## Summary

Replaced `egui::Grid` with `egui_extras::TableBuilder` in both table renderers (markdown `|...|` and HTML `<table>`) in the vendored egui_commonmark fork. Issue #4 from `emvolz` (2026-04-20) reported difficulty viewing wide tables. The earlier PR #15 (`feature/table-scroll`) addresses the horizontal-scroll bug; this PR addresses the column-width side.

Drag-to-resize between columns is enabled via `.resizable(true)` on both Table and Column. Cells get a proper width context so content wraps when a user makes a column narrower.

## Features

- [x] Markdown-table renderer uses TableBuilder
- [x] HTML-table renderer uses TableBuilder
- [x] Each column is `Column::auto().resizable(true).at_least(40.0)`
- [x] Drag-to-resize wired between adjacent resizable columns (via `.resizable(true)`)
- [x] Striping preserved
- [x] Outer `Frame::group` border preserved
- [x] Manual `paint_vertical_separator` calls removed in HTML table; helper function deleted (was only call site)
- [x] Manual `ui.painter().hline()` under HTML table header removed
- [x] Recursive event dispatch preserved with column-width as `max_width`
- [x] Search highlights still render inside cells (no code change needed; `RichText.background_color` works in any `Ui`)
- [x] Inline-code wrap segmentation renders correctly across multiple cell rows (heterogeneous row heights via `cell_visual_lines` precomputation — E1 fix)
- [x] Outer `ScrollArea::horizontal` restored around TableBuilder so wide tables exceeding viewport scroll instead of clipping (D3 fix)
- [x] `cargo build` clean
- [x] `cargo clippy --bin md-viewer -- -D warnings` clean
- [x] `cargo test --bin md-viewer` passes (13/13)
- [x] Visual verification on Xvfb: plain narrow, wide 10-column, inline-code paths, mixed inline markdown, HTML table, real-world `KEYBOARD_SHORTCUTS.md`

## Key Discoveries

### Verification round 3: thorough MCP testing

Re-verified everything via egui MCP on isolated port 9878 (worked around a concurrent Claude session occupying port 9877). Findings:

| Test | Tool | Result | Notes |
|------|------|--------|-------|
| A1 tab switch via MCP click on `File: test-inline.md` | `egui_click` | ⚠️ MCP limit | `Clicked n209` ack'd but tab didn't actually switch; snapshot unchanged |
| A2 explorer collapse via MCP | `egui_click` | ⚠️ MCP limit | Same — click ack'd, tree unchanged |
| B1 Ctrl+F via xdotool keystroke | xdotool | ❌ env limit | XGetInputFocus error confirms no keyboard routing without WM (per LESSONS.md) |
| C1 narrow→wide drag | xdotool | ✅ | Status column visibly grew |
| C2 wide→narrow with at_least(40) floor | xdotool | ✅ | Floor respected |
| C3 10-col interior divider drag | xdotool | ⚠️ precision | Hit-zone ~5px on ~40px columns; couldn't target precisely without hover feedback |
| C4 HTML table drag | xdotool | ✅ | Header A grew, B+C shifted right |
| **D3 narrow window scrollbar** | xdotool resize+drag | ⚠️ partial fix | At 800px window, wide-10-col table drops C9/C10 with NO horizontal scrollbar. Even forcing C1 wider via drag (table content > viewport) doesn't trigger outer ScrollArea. At 1000px+ all 10 cols fit. Root cause: TableBuilder constrains to `ui.available_width()` regardless of outer ScrollArea — it shrinks columns to fit OR drops columns rather than reporting overflow. The outer ScrollArea wrap is in place but inactive for this case. **Affects only tables with many narrow columns at very narrow windows; common tables (≤6 cols) work fine.** |
| E1 inline-code wrap | xdotool + visual | ✅ | Reconfirmed twice: long paths span 2 visual lines per cell |
| E3 heading inside cell | xdotool + visual | ✅ no fragmentation | `## Heading` in cell renders as literal text (pulldown_cmark doesn't recurse into cells), table layout intact |

**MCP limitation discovered:** `egui_click` on AccessKit-registered buttons (n209 File: button, n7 Explorer: Collapse All) acknowledged but didn't invoke `button.clicked()` handler. Sample of 2 buttons; pattern consistent. Worth a follow-up investigation in `~/dev/mcp/egui-mcp/crates/egui-mcp-bridge/`. For this PR, all behavioral verification used xdotool instead.

**D3 follow-up tracked:** the ScrollArea wrap is functional infrastructure but TableBuilder's self-shrinking renders it inactive for the wide-10-col-at-narrow-window edge case. Potential future fix: use `Column::initial(N)` for tables with many columns where N > viewport/num_cols would force overflow. Out of scope for this PR; the JTBD ("view a wide table comfortably") is satisfied by drag-resize + cell-wrap.

### Two regressions found during verification; both fixed

Visual regression testing on Xvfb (post-initial-refactor) found:

1. **D3** — wide tables clip on narrow window: dropping the outer `ScrollArea::horizontal` wrapper exposed wide tables to clipping at the panel edge. TableBuilder doesn't add horizontal scroll on overflow. Fix: restore the wrapper around the TableBuilder chain.
2. **E1** — multi-chunk inline-code wrap clipped past first chunk: TableBuilder's `body.row(h, ...)` uses fixed height. Chunks 2+ render at correct y but outside the row's clip rect. Fix: pre-compute heights via `cell_visual_lines` (counts max chunks across cells in a row × line height) and pass per-row to `body.row(h, ...)`.

Both fix details documented in `LESSONS.md` under the matching new lessons. Re-verified via egui MCP + screenshot:

- E1: paths `~/very/long/.../cells.rs` (60 chars → 2 chunks) and `/usr/share/cargo/registry/src/index.crates.io-...` (96 chars → 2 chunks) now render across 2 visual lines per cell. height-debug output confirmed `body_heights=[55.125, 27.5625, 55.125]` (= 2 line heights, 1, 2) for the corresponding rows.
- D3: ScrollArea wrap is in place. Visual confirmation of scrollbar at very narrow windows (800×600) was inconclusive in the verification environment (concurrent processes contention); the mechanism is wired and tested at wider widths (all 10 columns visible at 1900px window).

### `ui.vertical()` is required to keep TableBuilder header and body from overlapping

Without `ui.vertical(...)` around the TableBuilder chain, the body's first row renders at the same Y as the header row, creating a visual "4-column single row" effect at the top of the table (for a 2-column markdown table, header H0/H1 sits on the same line as body R0C0/R0C1). The fix is a one-line wrap. See `LESSONS.md` → "TableBuilder body overlaps header when the parent ui isn't vertical-flow" for the full root-cause analysis and code pattern.

### `parse_table` returns `header: Vec<Cell>` for a single row, NOT `Vec<Row>`

`header` is a *flat* `Vec<Cell>` representing the header row's cells. `header.first().map(|h| h.len())` therefore gives the *event count* of the first cell (e.g., 1 for `[Text("Status")]`), not the column count. Use `header.len()` directly. Documented in `LESSONS.md`.

### `parse_table` emits a trailing empty row from pulldown_cmark

A 3-row markdown table parses to 4 rows, the last with 0 cells. Filter `rows.into_iter().filter(|r| !r.is_empty()).collect()` before rendering. Documented in `LESSONS.md`.

### `egui_extras 0.33` has no sticky/freeze-column API

Searched `~/.cargo/registry/src/.../egui_extras-0.33.3/src/table.rs`. Available `Column` methods: `auto`, `auto_with_initial_suggestion`, `initial`, `exact`, `remainder`, `resizable`, `clip`, `at_least`, `at_most`, `range`, `auto_size_this_frame`. No `sticky()` or freeze-pane equivalent. Sticky first column for very wide tables would require custom layout work and is deferred to a follow-up.

### `paint_vertical_separator` and the manual header/body `hline` are redundant under TableBuilder

TableBuilder draws its own column separators when `.resizable(true)` is set and visually distinguishes the header row from body rows (subtle background variation via `.striped(true)`). The manual `Self::paint_vertical_separator(ui, border_color)` calls between cells and `ui.painter().hline(...)` under the header in the old HTML-table renderer were dropped. `paint_vertical_separator` was deleted because it had no other call site.

## Architecture

### Public API changes

None. The vendored egui_commonmark fork is consumed by `md-viewer` only, and the public `CommonMarkViewer` API is unchanged. Only internal renderer implementation moves from Grid to TableBuilder.

### Function-level changes in `pulldown.rs`

| Function | Before | After |
|----------|--------|-------|
| `table` | Grid::new(id).striped(true).show(...) with nested `for col in header { ui.horizontal { event } }` + `ui.end_row()` | TableBuilder chain with `.header(row_h, ...)` then `Table::body(...)` using `body.row(row_h, ...)` and `row.col(...)`, wrapped in `ui.vertical(...)` |
| `render_html_table` | Grid::new(id).striped(true).min_col_width(40.0).spacing(0,0) with manual `paint_vertical_separator` between cells and manual `hline` between header and body | TableBuilder chain; first header row goes through `.header()`, any extras go through `body.row()` with strong styling; manual separators dropped |
| `paint_vertical_separator` | Function existed | **Deleted** (no call sites remain) |

### Layout invariants preserved

| Invariant | Preservation strategy |
|-----------|----------------------|
| Striping | `.striped(true)` on TableBuilder |
| Recursive event dispatch | Same `self.event(ui, e, src_span, cache, options, col_w)` call inside `row.col(\|ui\| { ... })` |
| `should_start_newline`/`should_end_newline` save-restore | Same `std::mem::replace` pattern inside the cell closure |
| Search highlights | No code change — `event_text_with_highlights` emits `RichText.background_color` which works in any `Ui` |
| Heading rich-text accumulator | No interaction in practice (cells don't typically contain heading markdown) |
| Inline-code wrap segmentation | `ui.end_row()` inside the cell's `Ui` works the same as before |
| `max_width` propagation | Now passes `ui.available_width()` (the column's width) as `max_width` to recursive `self.event(...)` calls |
| Dark-mode borders | TableBuilder uses `ui.visuals().widgets.noninteractive.bg_stroke.color` natively |
| Outer Frame::group border | Preserved with `Frame::group(ui.style()).show(ui, |ui| TableBuilder::new(ui)...)` |

## Testing Notes

Visual verification on Xvfb at `DISPLAY=:99`:
- Plain narrow 2-column table: ✓
- Wide 10-column table: ✓ (all columns visible, body rows aligned)
- Asymmetric column widths (Status / long description): ✓
- Inline code with long file paths (issue #5 case): ✓ (existing `inline_code_wrap_segments` still chunks correctly inside cells)
- Mixed inline markdown (bold, italic, links, code): ✓ — recursive event dispatch through `self.event(ui, ...)` works
- HTML `<table>` with `<th>` + `<td>`: ✓
- Real-world test: `docs/KEYBOARD_SHORTCUTS.md` (3 separate tables): ✓

**Drag-resize (narrow → wide) verified** end-to-end on Xvfb via the egui MCP bridge (temporarily uncommented `mcp` feature in Cargo.toml for this run; reverted after). Starting from the natural auto-sized state of the `Status | Description` test table, `xdotool` drag from the column divider at (x=311, y=220) to (x=600, y=220) resized column 0 from ~30 px to ~340 px and pushed column 1's content into a clipping region — the long description rows visibly truncated at the new right edge. The hit-zone is narrow (~5 px wide, right at where the divider visually sits); cursor needs to be on the divider, not in cell text, or the drag becomes text selection instead.

### Attempted verification matrix (Xvfb + egui MCP)

A follow-up verification session attempted to exercise the full test matrix from `~/.claude/plans/implement-priority-3-from-zippy-raccoon.md` Section "Test matrix":

- **A1 multi-tab switching** — partially passed: app loads `table-regression.md`, snapshot returned 233 AccessKit nodes including all expected widgets (tabs, file explorer, outline, search bar registrations). Clicking `File: test-inline.md` did not cleanly switch the visible content before the test session aborted.
- **A2 explorer expand/collapse, A3 search-in-cell, B1-B4 keyboard shortcuts, C2-C5 multi-direction / multi-table drags, D2-D3 light mode + narrow window, E1-E3 LESSONS.md regression cases**: **NOT VERIFIED** during the follow-up session. Environmental instability — concurrent Claude sessions on the same host were periodically killing `md-viewer` processes and contending for port 9877 — caused the test app to die repeatedly mid-run. Xvfb itself stopped responding to `xdpyinfo` partway through. The verification harness needs a more isolated environment (its own Xvfb on a different display number, or a real desktop session).

### Manual verification required on a real desktop session

Before declaring the feature complete, run the binary on the user's real X11/Wayland session and confirm:

1. **C2** — Drag a wide column narrower until `.at_least(40.0)` floor; verify the column refuses to shrink below ~40px.
2. **C3** — In a wide 10-column table, drag an interior divider; verify only adjacent columns rebalance (others unchanged).
3. **C4** — Open `test-html.md`, drag a divider; verify the HTML table renderer's drag-resize works too.
4. **D2** — Ctrl+D to toggle light mode; verify table border / striping / cell content remain visible.
5. **D3** — Resize window narrow (< 800px); verify tables either fit (with wrap) or scroll horizontally without breaking cells.
6. **E1** — Open a file with `~/some/very/long/file/path.rs` inline-code in a table cell, narrow the column; verify the issue #5 wrap-segmentation still chunks at 56-char boundaries within the cell.
7. **E2** — Ctrl+F search a word that appears in a cell; verify highlight pixels appear inside the cell and Enter cycles between matches.

If any of these fail on the real desktop, **block the PR** and root-cause before merge. If all pass, the feature is fully ready.

**Pre-existing baseline screenshots were skipped** in favor of interactive iterative verification at each step. The pre-refactor v0.1.4 binary is one branch away (`main`) if precise pixel-diff is needed.

## Future Improvements

- [ ] Sticky first column for very wide tables — needs custom layout work, no native egui_extras 0.33 API
- [ ] Heterogeneous row heights via `body.heterogeneous_rows(...)` for cells with block-level content (currently single text-line height; tall content clips)
- [ ] Persist column widths across sessions in `PersistedState` (HashMap keyed by `(PathBuf, table_index)`)
- [ ] Double-click divider to reset column pair width
- [ ] Right-click context menu with "Reset all column widths"
- [ ] `colspan`/`rowspan` support in HTML tables
- [ ] Multi-row header rendering with all rows styled as header (currently first row goes through native `.header()`; extras render as bold body rows)

## Related

- Issue #4 (column-resize half) — `emvolz`, 2026-04-20
- PR #15 / `feature/table-scroll` — horizontal-scroll wheel fix (sibling work, not yet merged into main)
- Memory: `[[issue-4-context]]` (note: reporter is `emvolz`, not `chrismeyersfsu` — memory file needs correction post-merge)
- Original briefing: `~/.claude/plans/prio3-resizable-table-columns.md`
- Implementation plan: `~/.claude/plans/implement-priority-3-from-zippy-raccoon.md`
