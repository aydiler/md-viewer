# Adding Editing: Approach Comparison (Obsidian-style / Slack-canvas-style)

**Status:** Research · **Date:** 2026-02 · **Branch:** `feature/editing-research`

How could md-viewer gain editing? This document surveys how established products
(Obsidian, Typora, Slack Canvas, Notion) implement markdown/rich-text editing,
inventories the Rust/egui building blocks available today, maps six candidate
approaches onto *this* codebase, compares them, and recommends a phased path.

---

## 1. Current state & constraints (what we already have)

The app is a read-only viewer: `Tab { content: String, cache: CommonMarkCache, … }`
rendered through a vendored `egui_commonmark` fork (`crates/egui_commonmark/`).
Facts that materially shape any editing design:

| # | Fact | Where | Consequence for editing |
|---|------|-------|------------------------|
| F1 | Renderer caches parsed `(pulldown_cmark::Event, Range<usize>)` pairs keyed by content hash / monotonic `content_version` | `misc.rs:1766` (`cached_events`), `lib.rs:513-535`, `main.rs:659` | **Byte spans of every event already exist**; re-parse per keystroke is one pass over the doc (~52 ms @ 100k lines per fork comment; sub-ms for typical docs) |
| F2 | Renderer records `split_points = Vec<(event_index, start_y, end_y)>` at *safe block boundaries* (not inside lists/tables/blockquotes), **but viewport-skip paint is disabled as buggy** — rendering is a forced full paint every frame (~5.7 ms @ 2.5k events, 229 ms @ 100k) | `parsers/pulldown.rs:658-679` (recording), `:833-843` (skip disabled), `:844-994` (dead skip code) | Block boundaries + ys are available for segmentation/hit-testing; but there is **no virtualization today**, so large-doc edit performance depends on cache reuse, not skipped painting |
| F3 | Search-highlight feature already splits `Event::Text`/`Event::Code` at byte boundaries and restyles spans | `set_search_ranges`, `misc.rs:1782-1790`; handler at `pulldown.rs:1352+` | Precedent for span-level surgery inside the renderer |
| F4 | Fork has a `mutable` mode: interactive checkboxes emit `CheckboxClickEvent { checked, span }` for source writeback | `lib.rs:478`, `pulldown.rs:1408-1419` | Precedent for rendered-view → source-range writeback |
| F5 | File watcher auto-reloads tabs on external change: unconditional `tab.reload()` | `reload_changed_tabs`, `main.rs:2020-2050` | **Would clobber unsaved edits**, including edits written by the app itself → any edit mode needs dirty-tracking + self-write suppression + conflict UX |
| F6 | Derived caches parsed once per load: outline headers (`parse_headers`, regex), `local_links`, search matches | `main.rs:908, 845, 973` | Must be refreshed on edit (debounced), not per keystroke |
| F7 | Mermaid/math render states keyed by content hash, rendered on background threads | `misc.rs:1792-1836` | Typing inside a diagram/formula triggers background re-renders → debounce needed |
| F8 | Repo pattern is State → Logic → UI → Async; no parsing in UI code | `docs/EGUI_WORKFLOW.md` | Edit state machine must live in state/logic layer, UI only reads |
| F9 | Fork already diverged from upstream egui_commonmark (search ranges, strong font, table scroll, latex delimiters, event cache) | `crates/egui_commonmark/` | More divergence has real maintenance cost, but the team owns the fork |
| F10 | **No save infrastructure exists**: no `fs::write` anywhere, no dirty flag, `PersistedState` stores only paths/settings | `main.rs:148-160`, grep for `fs::write` | Save path, dirty markers, and unsaved-buffer persistence are all greenfield |
| F11 | Keystroke invalidation is currently **global**: both cached events and syntect `LayoutJob`s are keyed by whole-document content hash, so one keystroke re-runs syntax highlighting for every code block | `misc.rs:1766, 1773`; cleared in `pulldown.rs:597-606` path | Code-heavy docs pay full syntect re-cost per keystroke → block-scoped cache keys are a prerequisite for comfortable editing |
| F12 | Scroll/search y-positions assume immutable content (`header_positions`, active-match y recorded during paint) | `misc.rs:1776-1790`, `main.rs:2203` | Mid-edit scroll jumps degrade gracefully but need version guards or recompute-on-commit |

## 2. Reference architectures

### 2.1 Obsidian — CodeMirror 6 "Live Preview" *(verified)*
Obsidian embeds **CodeMirror 6** ("Obsidian uses CodeMirror 6 (CM6) to power
the Markdown editor" — official dev docs). It offers two editing modes:
**Live Preview** ("shows formatted text inline while hiding most Markdown
syntax. When your cursor enters formatted content, the underlying syntax
becomes visible") and **Source mode** (everything raw); Reading view is
non-editable rendered output.

Mechanism: the buffer always holds raw markdown; a `Decoration.replace({})`
layer hides markup characters *without changing line height*, computed by view
plugins scoped around the cursor; whole-block widgets (checkboxes, tables) are
widget decorations replacing spans. CM6 provides immutable state + dispatched
transactions (typed change sets → undo/history), composable decorations,
incremental Lezer parsing, and **viewport-only rendering**.

⚠️ **Layout-thrash lesson** from a documented CM6 live-preview clone
(atomic-editor): their first cut swapped each block to a rendered widget on
cursor exit → measurable layout shift (~0.1 CLS per 10 cursor moves); inline
decoration-hiding with stable line heights fixed it. Any egui block-swap
design must keep swap-in-place geometry stable or it will jump on every click.

**Transferable idea:** plain text stays the single source of truth;
"rendered vs raw" is a *per-region visual mode* driven by cursor position,
implemented as decorations over the buffer — not a separate rich tree.

### 2.2 Typora — seamless WYSIWYG *(partially verified)*
Markets exactly the Obsidian-live-preview feel with no mode switcher
(typora.io). Bundles CodeMirror plus markdown-it/MathJax/Mermaid
(acknowledgements page). Internal renderer is undocumented; consensus says a
DOM-based custom renderer re-rendering blocks around the caret. Why it's rare
outside browsers: the DOM gives free bidirectional keystroke↔layout mapping,
text measurement, IME and mixed text/widget selection — all of which a custom
toolkit must own itself.

### 2.3 Slack Canvas — markdown-canonical storage, block-feeling surface *(verified at API layer)*
The programmatic content model of canvases is plain **markdown**:
`document_content { type, markdown }`, "currently the only supported type is
`markdown`", 1 MiB cap, fixed element vocabulary; Block Kit unsupported. No
public engineering writeup of the client editor stack exists — treat any
ProseMirror/Lexical attribution as rumor. Interaction grammar (sections with
drag handles, `/` commands, hover toolbar) is observable but undocumented.

**Precedent value:** Slack keeps the *stored* format markdown-simple while the
editing surface feels block-structured — direct support for keeping `.md`
canonical here.

### 2.4 Notion — block-graph canonical storage *(API verified; storage folklore secondary)*
Every block object has UUID `id`, `parent`, `has_children`, `type` enum +
payload incl. `rich_text`; pages index their child blocks. Widely reported
Postgres row-per-block implementation. Pros: cheap reorder/partial sync,
per-block ACLs, structured types beyond markdown. Cons: **lossy markdown
round-trip** (toggles/columns/nested DBs don't map to CommonMark), heavier
render stack (every block its own editable region).

### 2.5 Native-toolkit prior art
No mainstream product ships Obsidian-style live preview in an immediate-mode
GUI toolkit. **Zed** (GPUI, GPU-drawn rects/glyphs, explicitly post-Electron)
ships source-mode markdown editing, not hybrid preview. The best non-DOM proof
that live preview doesn't need a browser is CM6 itself: virtualized text core +
decoration overlays. In egui specifically, no equivalent component is known
*(ecosystem agent to confirm)* — if absent, this is greenfield UX work built on
`TextEdit`, not a drop-in crate.

## 3. Candidate approaches

### A. Two-pane: source editor + live preview ("VS Code style")
Edit toggle per tab; left pane `egui::TextEdit::multiline(&mut tab.content)`,
right pane existing `CommonMarkViewer`. Debounce re-render (bump
`content_version` on change). Save via Ctrl+S (+ autosave option).

- Hits: F1 (version-keyed cache makes re-render cheap), zero fork changes.
- Misses: not "live preview"; duplicate scroll contexts; pane-width cost.
- Effort: **S/M** — mostly app-level; watcher fix (F5) mandatory regardless.

Variant A′ (same pane, mode toggle): Ctrl+E flips tab between Rendered and
Source view — same TextEdit, fullscreen. Even cheaper; Obsidian's Source mode.

### B. Hybrid live preview, per-block swap ("Obsidian style") ★ flagship
Rendered view everywhere; the focused block renders as raw-source `TextEdit`
with rendered output everywhere else. Mechanics in this codebase:

1. **Block model (logic layer):** derive `Vec<Block { event_range, src_range,
   kind }>` per render pass from the cached events+spans (F1) — block
   boundaries are already detected (`is_block_end_tag`, F2), so no parser
   changes; lists/tables/blockquotes stay atomic, matching split-point safety.
   Recomputed on version bump, not per frame.
2. **Hit-test:** click y → block whose recorded y-range contains it
   (split points already record start/end ys); store `active_block` in Tab.
3. **Swap:** the force-bootstrap full-paint actually *simplifies* this — in the
   event loop, when iteration enters the active block's event range, emit a
   `TextEdit::multiline` seeded with `&content[src_range]` instead of the
   normal widgets. On change, splice back via one `replace_range`
   (`CheckboxClickEvent` writeback is the precedent, F4).
4. **Block exit rules:** click into another block / ↑↓ at text edges / Esc /
   Ctrl+Enter → commit + activate neighbor (Typora-style). Multi-block
   selection and cross-block drag-editing deferred.
5. **Per-block cache keys:** re-key `syntax_layouts` (F11) by
   `(block_src_hash, lang, theme)` so keystrokes only re-highlight the edited
   block; debounce derived caches (F6) and mermaid/math (F7).

- Hits: keeps `.md` sacred (data model stays one `String`, same as A);
  builds directly on F1–F4 machinery confirmed present; closest to Obsidian feel;
  validated by Slack storing canvases as plain markdown while their surface
  *feels* block-structured (§2.3).
- Risks: caret semantics across blocks (Enter splitting list items),
  undo stacks fragment per-TextEdit (mitigate: commit-on-exit + app-level undo
  buffer), IME composition inside swapped widget, inline-format toolbar extra —
  and **swap-induced layout jump**: the CM6 atomic-editor project measured
  ~0.1 CLS per 10 cursor moves when swapping block↔widget with unstable
  geometry (§2.1); mitigation is keeping the TextEdit metrics identical to the
  rendered block (same font/wrap width, reserve line heights) or hiding
  decorations in place instead of widget replacement for inline spans.
- Effort: **M/L**, incremental — paragraph/heading swap first, then lists,
  then tables. Fork extension: an "active block override" hook in the paint loop.

### C. Click-to-edit section overlays ("section edit", wiki-style)
Like B but coarser: double-click swaps a whole section (heading + until next
heading) for a plain TextEdit overlay; rest stays rendered. No block model
finer than headings needed (reuse `parse_headers` offsets).

- Cheaper than B (M effort), noticeably less slick; good intermediate step.

### D. Full block-graph canvas editor ("Slack canvas style")
Restructure storage into typed blocks with drag handles/slash menus, serialize
back to markdown on save.

- Directly conflicts with the app's promise (faithful rendering of *arbitrary*
  user markdown: tables, footnotes, math, mermaid, HTML blocks — all lossy or
  unrepresentable in a hand-built block schema). Notion's own model pays this
  cost deliberately; Slack notably did *not* — canvases store plain markdown.
- Largest effort (XL): storage rewrite, DnD reorder UI, per-block chrome,
  round-trip tests. Rejected for this codebase's goals; revisit only if the
  product pivots to "canvas notes app".

### E. Embed a web editor (CodeMirror 6 / MilkDown in wry webview)
Instant fidelity to Obsidian's UX; but eframe(glow)+child-webview compositing
is fragile, adds a browser engine + IPC bridge to file IO, breaks the ~35 MB
native footprint story, two toolkits to maintain. Rejected on weight/coherence;
*(agent to confirm known wry+glow conflicts)*.

### F. External-editor handoff (baseline)
"Edit" menu item opens `$EDITOR`/system default; the existing watcher (F5)
live-refreshes the viewer. Near-zero code; useful stopgap and already 80%
present via watch mode.

## 4. Comparison matrix

| Axis | A/A′ two-pane | C section-overlay | B block-swap | D block-canvas | E webview |
|------|--------------|-------------------|--------------|----------------|-----------|
| "Live preview" feel | ✗ (split attention) | partial | **✓ closest to Obsidian** | ✓✓ canvas-y | ✓✓ |
| `.md` stays source of truth | ✓ | ✓ | ✓ | ✗ (lossy risk) | ✓ (if wired so) |
| Effort / risk in this codebase | S | M | M/L (incremental) | XL | L + foreign deps |
| Fork changes needed | none | none–minor | moderate (block segmentation + active-block paint hook + per-block cache keys) | large | none |
| Perf on large docs | good if F11 fixed; full repaint each frame either way (no virtualization, F2) | same as A | best of native options: per-block cache keys contain keystroke cost; still full repaint | unknown/new engine | outsourced |
| Editing UX completeness (undo, IME, selection) | TextEdit-grade | TextEdit-grade | TextEdit-grade per block; cross-block gaps initially | bespoke | best-in-class |
| Watcher conflict work (F5) | required | required | required | required | required |
| New infra required (save/dirty, F10) | all of it | all of it | all of it | all of it | all of it |
| Alignment w/ repo patterns (State→Logic→UI→Async) | high | high | high (block model = logic layer) | low | low |

## 5. Cross-cutting integration work (any approach)

Ranked blockers from the codebase audit:

1. **Watcher safety (F5) — blocker #1:** add `dirty: bool` +
   `last_saved_hash: u64`; in `reload_changed_tabs` skip reload when incoming
   content hash equals own last write; external-change banner offering
   Reload / Keep-mine. Touches `Tab::reload`, `check_file_changes`.
2. **Save path (F10):** Ctrl+S → async write channel (never in UI); dirty
   marker in tab title/window title; optional autosave debounce;
   `PersistedState` must not silently drop unsaved buffers on restore.
3. **Per-block cache keys (F11):** re-key syntect `syntax_layouts` by block
   source hash so a keystroke re-highlights one block, not the document.
4. **Derived-cache refresh:** outline headers, local links, search matches
   rebuild debounced (~250 ms idle) instead of per load (F6).
5. **Scroll anchoring while typing above viewport:** version-guard the
   recorded-y corrective scrolls (F12), reusing the existing machinery
   (`main.rs:2869-2891`) only when `content_version` hasn't changed mid-jump.
6. **Keyboard map:** Ctrl+E toggle edit/render; Ctrl+S save; Tab/Shift-Tab
   indent inside list blocks; arrows crossing block boundary commit+move.

## 6. Recommendation

Phased, each phase independently shippable:

1. **Phase 0 — save/dirty/watcher plumbing (+ F, edit-in-$EDITOR)**:
   blockers #1–2 fixed as pure app-level work. Unblocks everything.
2. **Phase 1 — A′/A (mode-toggle source editing; optional side-by-side)**:
   proves the save/dirty/conflict loop end-to-end with zero fork changes.
3. **Phase 2 — C (section click-to-edit)**: first rendered-surface editing;
   exercises hit-testing + swap UX using existing heading offsets.
4. **Phase 3 — B (block-level live preview)**: generalize C with the
   event-span block model (F1+F2) and per-block highlight keys (F11);
   fork gets an "active block override" hook in the paint loop. This is the
   Obsidian-like flagship.
5. **D/E explicitly out of scope** for the viewer's mission.

Rough sizing (focused work): P0 days, P1 ~1 wk, P2 ~1–2 wk, P3 multi-week
incremental behind a flag.

## 7. Sources & evidence

Product architecture (fetched primary sources):
- CodeMirror 6 guide — transactions, decorations, viewport-only rendering: <https://codemirror.net/docs/guide/>
- Obsidian dev docs — "Obsidian uses CodeMirror 6": <https://docs.obsidian.md/Reference/Editor/Editor+extensions>
- Obsidian help — Live Preview vs Source mode definitions: <https://help.obsidian.md/Editing+and+formatting/Live+preview+editing>
- atomic-editor (CM6 live-preview clone) — decoration mechanism + CLS layout-thrash lesson: <https://github.com/kenforthewin/atomic-editor/blob/main/docs/architecture.md>
- Typora positioning & bundled libs: <https://typora.io/> · <https://support.typora.io/Acknowledgement/>
- Slack Canvas API — markdown `document_content` model, 1 MiB cap: <https://api.slack.com/surfaces/canvases>
- Notion block object schema: <https://developers.notion.com/reference/block>
- Zed / GPUI native rendering: <https://zed.dev/blog/videogame> · <https://zed.dev/docs/languages/markdown>

Local code evidence: file:line references in the table in §1 and throughout
§3–5 were verified by direct inspection of this repository at commit `121ae04`
(`src/main.rs`, `crates/egui_commonmark/`) and of egui 0.33.3 sources in the
local cargo registry (`text_edit/{builder,state,output}.rs`, `data/input.rs`).
