# Editing Feature — Current State

**Branch:** `feature/editing` (worktree `worktrees/editing`) — 30 commits ahead of `main`
**Delta:** +4,212 / −61 across 26 files (`src/main.rs`, vendored fork, docs, scripts)
**Status:** PSE v1 complete and manually verified; ready for PR review
**Last updated:** 2026-02 (session of 2026-08-24)

---

## 1. What exists now

### Editing modes (per tab, Ctrl+E cycles; View menu has all three)

| Mode | Behavior |
|---|---|
| **Rendered** | Original viewer, untouched |
| **Live (PSE)** | Every text block is a permanently-styled TextEdit laid out in document flow; images/math/mermaid/tables/code render normally between editors |
| **Source** | Whole-file monospace editor |

### Live mode (PSE) — the flagship

- **Fidelity**: editors adopt renderer typography by construction — Major Third
  heading scale (H1 2.0× → H6 0.875×) on the strong family, tight heading line
  height (~1.3× equiv), body 1.5× line height, weak color for quotes,
  typography spacing above/below headings and after paragraphs. Full-width
  top-down rows so positions match the rendered view across Ctrl+E toggles.
- **Inline formatting**: strong/emphasis/code/link runs styled per-section;
  structural markers (`# `, `- `, `> `, `**`, backticks, link URLs) hidden via
  equal-char-count glyph substitution — cursor/selection/IME math exact.
- **Caret-line reveal**: the caret's line shows raw markdown; everything else
  in the block stays styled (Obsidian Live Preview signature).
- **Session buffers**: keystrokes mutate per-block egui-temp buffers only.
  Serialization (buffers → markdown with markers re-materialized) happens on
  **Ctrl+S** and on **mode exit** — never mid-typing, so no reflow races.

### Phase 0 infrastructure (all modes benefit)

- Atomic background saves (temp file + rename), async worker thread
- Dirty tracking with synced/in-flight content hashes; dirty markers in tab
  bar and window title
- Watcher self-write suppression via pure `classify_disk_change()` policy;
  external-change conflict banner (Reload from disk / Keep my version)
- Unsaved-changes guards on close/quit (Save / Discard / Cancel)
- File → Save / Open in External Editor menu entries

### MCP test automation (new capability)

- `mcp` cargo feature registers mode/save buttons with the egui-mcp bridge
- `scripts/mcp_e2e.py`: JSON-RPC driver — menu clicks, focus, type_text,
  get_value round-trip, save, disk verification
- `MDV_SIM_CLICK_Y` env hook injects synthetic clicks through the real path
- `MDV_EDIT_DEBUG=1` enables per-frame/per-block paint tracing

---

## 2. Architecture summary

```
Live frame:
  app builds EditSessionConfig { salt, blocks[{src, kind}] }
    ← spans from cached events + kinds from first-line sniffing
  fork paints each event stream position:
    text block     → Frame > TextEdit(multiline, markdown_block_job layouter,
                                     per-block temp buffer, caret temp)
    non-text block → normal renderer widgets (image/math/mermaid/table/code)
  after paint:
    feedback = cache.get_session_feedback(salt)   // non-destructive (multi-pass!)
    changed buffers update Tab.session_buffers, set dirty
Serialize (Ctrl+S / mode exit):
  walk blocks: edited → materialize(kind, buffer)   // markers re-added
               passthrough → original slice
  replace Tab.content atomically; bump version; mark_edited
```

Key invariants:
- Char-count-preserving substitution keeps CCursor math exact (egui#5885 safe)
- Keystrokes never touch source mid-session; serialize is a single fold
- Session state resets on reload/load_file (fresh seed from disk)
- Editors paint in explicit-size top-down rows (inline-flow would cascade them
  sideways — fixed and regression-documented)

## 3. Verification state

| Suite | Count | Status |
|---|---|---|
| App unit tests | 57 | ✓ |
| Backend lib (incl. 7 styler, segmentation) | 24 | ✓ |
| Frontend lib + live_preview e2e ×3 | 37+3 | ✓ |
| live_session e2e (editors paint, keystroke routing) | 1 | ✓ |

Manual: click/type/save loop verified on desktop across sessions; crash paths
(stale-range panic) hardened; `RUST_BACKTRACE=1` launches available.

## 4. Known limitations (v1)

1. Lists edit as one raw unit (markers visible); no per-item editing
2. Arrow keys don't cross block boundaries
3. Search/outline operate on last-serialized content while typing
4. Copying selected text inside a styled editor yields galley text (hidden
   markers copy as blanks) — egui#5885 constraint
5. No formatting shortcuts (Ctrl+B/I), hover toolbar, or slash commands
6. IME composition inside restyled layouters untested (CJK verification pending)
7. Conflict banner only shows for the active tab
8. Very large docs (>10k events) repaint fully every frame (fork limitation,
   pre-existing)

## 5. Suggested next steps (priority order)

1. **PR review + merge** of `feature/editing` into main
2. Manual CJK/IME pass (wtype or real keyboard)
3. Arrow-key block crossing (needs caret-edge signal from fork)
4. Formatting shortcuts (Ctrl+B/I/K inserting markers at selection)
5. Per-item list editing (split ListItem blocks further)
6. Virtualization restoration in fork for huge docs

## 6. Key files

| Path | Role |
|---|---|
| `src/main.rs` | Modes, session state, save pipeline, watcher policy, UI wiring |
| `crates/.../backend/src/styler.rs` | `markdown_block_job` + style config (the look) |
| `crates/.../backend/src/misc.rs` | Session config/feedback, boundaries, cache APIs |
| `crates/.../egui_commonmark/src/parsers/pulldown.rs` | Session paint loop, boundary recording |
| `docs/research/editing-approaches.md` | Research + decision record |
| `docs/research/editing-architecture-overview.md` | Architecture comparison |
| `docs/devlog/055–058` | Implementation narratives |

## 7. Debug hooks

| Hook | Effect |
|---|---|
| `RUST_LOG=info` | Mode switches, activations, seeds, splices |
| `MDV_EDIT_DEBUG=1` | Per-block painted rects + activation marker file |
| `MDV_SIM_CLICK_Y=<px>` | Inject synthetic click at document offset (headless) |
