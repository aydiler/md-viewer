# Editing Architecture Overview

**Status:** Decision document · **Branch:** `feature/editing` · **Supersedes:** §6 phasing in `editing-approaches.md`

Which architecture should md-viewer use for live-preview editing? This document
lays out every viable option, the evidence for each, and a concrete
recommendation with a migration path from the current prototype.

---

## 1. The problem, stated precisely

egui has no contentEditable. A browser (Notion, Slack Canvas, Tiptap) edits
*inside* the rendered result because the DOM both renders and accepts text
input. In egui, `TextEdit` is plain text: it cannot contain rendered headings,
images, or math, and its glyphs come from one monospace-ish font run unless a
custom layouter intervenes. So "render and edit the same text" must be
reconstructed by hand. Every candidate below is a different answer to one
question: **where does the editing surface live relative to the rendering
surface?**

## 2. Evidence base (all verified)

| Source | Key finding |
|---|---|
| **Obsidian/CM6** | Raw markdown buffer + `Decoration.replace` hides syntax outside the caret; viewport-only paint; transactions |
| **Typora** | DOM-only trick; no native equivalent exists |
| **Notion / Slack Canvas** | Rich block tree is canonical; markdown only at import/export (Canvas API stores plain markdown!) |
| **transcendence (our own project)** | Tiptap (ProseMirror schema) + Yjs CRDT (`yrs`/Axum) + Vue — rich-tree canonical, DOM does render+edit |
| **Ferrite ★1.8k (source dive)** | Persistent *styled* TextEdits per block — headings are always frameless single-line editors at heading scale; structural syntax hidden by **buffer design** (commit re-adds `#`/markers on exit); formatted spans shown as styled galleys when inactive, raw while active; keystrokes never touch source (per-block buffers, commit-on-exit, epoch-keyed widget IDs); caret via `galley.cursor_from_pos`; dismissal via global clicked-outside flag ("rect hit tests unreliable") |
| **egui 0.33 source + tracker** | `TextEdit::layouter` returns a full `Arc<Galley>`; ALL cursor/selection hit-testing runs on the returned galley and `CCursor.index` is a **char** offset — equal-char-count glyph substitution keeps every mapping correct (in-tree proof: password masking). Per-section styling confirmed (`TextFormat`: size/weight/color/italics/underline/line-height; `LayoutJob::format_at_byte`, PR #8244). ⚠️ egui#5885 (open): **clipboard text comes from the galley** — never skip bytes; equal-count substitution keeps copy/paste intact (hidden markers copy as blanks) |
| **Zed** | Inline-rendered markdown *editing* is still an OPEN feature request (zed#21717, citing Obsidian); preview is a separate pane. Even Zed hasn't built it |
| **Ecosystem sweep 2026-08** | GitHub+crates.io searched ("egui richtext/wysiwyg/markdown/code editor"): **no maintained egui component provides inline syntax-hiding editing**. Ferrite is the only shipped rendered-mode editing app, engine unpublished. Render-only: tektite, membrane-io/egui_markdown, imgui_markdown 1.3k★, imgui_md. Dual-pane: md-echo, rustdown. Popover pattern: only Obsidian Hover Editor (Electron). Minor finds: Lilo, text-typeset |
| **Our v1 prototype** | Block-swap works end-to-end after 5 fixes (cache keying, boundary backfill, screen→content space, auto-focus, cut-segment ranges). Confirmed weaknesses: raw look, swap jump |

## 3. Candidate architectures

### A. Current v1 — swap-in plain TextEdit *(shipped)*
Rendered doc; clicked block becomes a monospace `TextEdit` seeded from its byte range.
- ✓ Works; minimal surface. ✗ Ugly (raw markdown), swap jump, caret lands wrong, typing felt dead until auto-focus fix.

### B+. Layouter-decorated swap *(upgrade of B)*
Same swap, but the active block paints through a custom `layouter`: markup
punctuation hidden via equal-char-count substitution (e.g. `**`→two spaces),
bold/heading/code runs styled per-section, caret line reveals raw syntax.
- ✓ True Obsidian feel inside stock TextEdit; cursor/selection/IME intact by
  the char-index contract; copy/paste safe as long as bytes are never skipped
  (egui#5885 constraint). Biggest perceived-quality jump for days of work.
- ✗ Still a *swap*: entering/leaving changes geometry (CLS risk documented in
  CM6 land); only one block ever looks rendered.

### C. Persistent Styled Editors — **recommended** *(Ferrite model, adapted)*
Live mode stops swapping entirely. Every top-level block is **permanently an
editor**, styled to look like its rendered form:

| Block kind | Surface in Live mode |
|---|---|
| Heading | Single-line `TextEdit`, heading font size/weight/color, `# ` stripped from buffer |
| Paragraph / list item / quote line | Multiline `TextEdit`, body font, `- `/quote markers stripped; list indent preserved |
| Inline bold/italic/code/links | Styled via `layouter` (dimmed markers, styled runs) — password-mask technique |
| Code fence | Mono `TextEdit` (already looks right) |
| Image / math / mermaid / html-table | **Rendered widget, read-only** — interleaved from the fork's normal pipeline (fidelity the editors can't provide) |

Commit semantics (Ferrite-proven): keystrokes mutate only per-block buffers;
on block exit the buffer is re-materialized into markdown (`# ` + text) and
spliced once. Widget IDs key off `(block identity, source_epoch)` — never a
content hash — so commits don't remap focus (the exact race that made our v1
feel broken). Caret placement on activation: build a display galley with the
same metrics and `cursor_from_pos(click)` — kills the offset class of bugs.
Dismissal: global "clicked outside active block" flag, not rect tests.

- ✓ Looks rendered *and* is always editable; no swap jump; scroll-stable;
  keeps `.md` canonical; preserves math/mermaid/image fidelity via interleave.
- ✗ Biggest build of the realistic options; per-block buffers + commit/undo
  bookkeeping; two sources of truth during a block session (manageable —
  Ferrite shipped it).

### D. Custom decoration engine ("mini-CodeMirror")
Own galley assembly, hidden ranges, hit-testing, IME over one big buffer.
True CM6 parity including reveal-at-caret. **XL greenfield** — revisit only if
md-viewer pivots to editor-product territory.

### E. Webview + Tiptap/Yjs *(transcendence stack)*
Instant WYSIWYG; wry↔eframe/glow coexistence is GTK-hack territory, browser
engine weight, IPC file bridge. Rejected previously; stands.

### F. Block-graph storage (Notion-style)
Lossy against arbitrary user markdown; against the mission. Rejected; stands.

## 4. Comparison

| Axis | A (v1) | B+ layouter swap | C persistent styled | D custom engine | E webview | F block-graph |
|---|---|---|---|---|---|---|
| Feels rendered while editing | ✗ | partial | **✓** | ✓✓ | ✓✓ | ✓✓ |
| Swap jump / CLS | ✗ | ✗ | **none** | none | none | none |
| Caret accuracy | poor | medium | **good (galley)** | best | best | n/a |
| `.md` canonical | ✓ | ✓ | **✓** | ✓ | ✓ | ✗ |
| Math/mermaid/image fidelity in edit mode | ✓ (outside block) | ✓ | **✓ (interleaved widgets)** | hard | ✓ | lossy |
| Effort from current state | done | S/M | **M/L** | XL | L+fragile | XL |
| Prior art in egui | ours | none known | **Ferrite (proven)** | none | external | Ferrite-lite |

## 5. Recommendation

**Adopt C (Persistent Styled Editors) as the Live-mode target, reached through
B+ as the first shipping step.** Rationale: C is the only option that fixes
all three of your reported complaints structurally (position via galley
caret, look via always-styled editors, jump via no-swap) while keeping the
viewer's fidelity promises and the `.md` file canonical. It is also the only
option with shipped prior art in egui (Ferrite) whose design docs validate the
hard parts we already stumbled over.

The independent ecosystem scan ranks B+ (styled layouter inside the existing
swap) as the single best effort/payoff move — days of work, cursor-safe by
the char-index contract, and it directly addresses the "doesn't look rendered"
complaint. The two recommendations compose: **ship B+ now, evolve to C.**
Both scans independently rule out D (single-surface WYSIWYG): no prior art in
egui, ImGui, or even Zed (open feature request #21717) — cost far exceeds
payoff over the block-swap family.

### Migration plan from current v1

1. **PSE-0 — correctness debt on v1** *(done)*: cache-keying, backfill,
   content-space boundaries, auto-focus.
2. **PSE-1 — persistent editors for headings/paragraphs/lists** in Live mode:
   strip/re-add structural markers at commit; per-block buffers; epoch-keyed
   IDs; galley-caret activation; global dismiss flag. Rendered mode untouched.
3. **PSE-2 — layouter inline decoration**: dim/hide emphasis markers, styled
   runs inside the active block (password-mask technique); code fences become
   mono editors.
4. **PSE-3 — interleave rendered widgets** for image/math/mermaid/tables
   between editors; tables stay read-only initially.
5. **PSE-4 (optional)** — reveal-raw-at-caret-line (full Obsidian parity),
   slash/format shortcuts.

Each step ships independently behind Live mode; Rendered/Source modes are
untouched throughout.

## 6. Open questions

- Undo across blocks: per-block Undoer + commit-level stack (Ferrite's
  `rendered_commit_undo` pattern) — needs design before PSE-2.
- Very large docs: PSE paints N editors/frame like Ferrite; acceptable up to
  ~10k events per the fork's bootstrap measurements; virtualization later.
- IME inside restyled layouters: password fields prove glyph substitution
  composes with IME; verify CJK early in PSE-2.
