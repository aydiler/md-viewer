# Feature: Editing Phase 0/1 — Safe Save Pipeline + Source-Mode Editing

**Status:** ✅ Complete
**Branch:** `feature/editing` (research groundwork on `feature/editing-research`)
**Date:** 2026-02
**Lines Changed:** ~+600 / -40 in `src/main.rs`

## Summary

First two phases of the editing roadmap from `docs/research/editing-approaches.md`.
Phase 0 builds the data-safety plumbing every later phase depends on (dirty
tracking, background atomic saves, watcher self-write suppression, conflict
UX, unsaved-changes guards). Phase 1 delivers usable editing: Ctrl+E flips the
active tab to a raw-markdown editor pane; Ctrl+S saves. The rendered view and
all existing viewer behavior are untouched.

## Features

- [x] Dirty tracking (`Tab.dirty`, synced/in-flight content hashes)
- [x] Background save thread — UI thread only clones + hashes (State→Logic→UI→Async)
- [x] Atomic writes: temp file + `fs::rename`, never truncate-in-place
- [x] Watcher self-write suppression via pure `classify_disk_change()` policy
- [x] Conflict banner when disk changes while tab is dirty (Reload / Keep mine)
- [x] Unsaved-changes guard for tab close + quit (Save / Discard / Cancel)
- [x] Ctrl+S save, Ctrl+E source-mode toggle, File/View menu entries
- [x] "Open in External Editor" (xdg-open) handoff
- [x] Dirty markers in tab bar ("file • ") and window title
- [x] Search jumps + outline clicks move caret in source mode
- [x] Debounced (250 ms) derived-cache rebuild after typing stops

## Key Discoveries

### 1. egui TextEdit cursor indices are chars, not bytes

All search/header machinery in this app speaks **byte offsets**
(`SearchMatch.byte_start`, pulldown-cmark spans). egui's `CCursor.index` is a
**char index**. Every jump into the source editor goes through
`byte_offset_to_char_index()`, which also floors mid-UTF-8-sequence offsets to
the containing char boundary.

```rust
state.cursor.set_char_range(Some(
    egui::text::CCursorRange::one(egui::text::CCursor::new(char_idx)),
));
```

### 2. TextEdit scrolls its *enclosing* ScrollArea, not itself

egui's multiline TextEdit auto-sizes to content and calls
`ui.scroll_to_rect(cursor_rect)` when the selection changes — that call
targets the nearest enclosing ScrollArea. So the editor pane is
`ScrollArea::vertical { TextEdit }`, which makes programmatic caret jumps
(Ctrl+F matches, outline clicks) scroll into view for free.

### 3. Watcher policy must be pure and table-tested

The reload decision has five interacting states (synced hash, buffer hash,
dirty flag, in-flight save hash, incoming hash). Encoding it as
`classify_disk_change() -> Ignore | Resync | Conflict | Reload` made the
tricky races testable without files or a display:

- own-save echo while in flight → **Ignore** (previously clobbered buffers!)
- user undid edits back to disk state → **Resync** (clear dirty instead of prompting)

### 4. note_synced must not clear last_edit_at

A quick type→save can complete before the 250 ms debounced derived-cache
rebuild. If `note_synced()` cleared `last_edit_at`, the rebuild would never
fire and outline/search would go stale behind a clean-looking file. Save
bookkeeping and edit bookkeeping are deliberately independent now.

### 5. GUI smoke tests aren't possible in this sandbox

Xvfb aborts inside the execution sandbox (X11 socket dir restrictions), so
verification here = `cargo build` + `cargo clippy` (0 new warnings) +
55 unit tests. Runtime behavior should get one manual check on a real desktop
before release: open file → Ctrl+E → type → Ctrl+S → confirm file contents +
watcher doesn't fight the save.

## Architecture

### Tab fields added

```rust
synced_content_hash: u64,        // buffer state known to match disk
in_flight_save_hash: Option<u64>, // snapshot being written right now
dirty: bool,
external_change_hash: Option<u64>, // pending conflict (dedupes events)
source_mode: bool,
derived_stale: bool,              // outline/links/matches need rebuild
last_edit_at: Option<Instant>,    // debounce anchor
close_after_save: bool,           // "Save & Close" flow
pending_caret_byte: Option<usize>, // caret jumps into the source editor
```

### New plumbing

```rust
struct SaveJob { path, content, hash }      // UI → worker
struct SaveOutcome { path, hash, result }   // worker → UI
enum UnsavedConfirm { CloseTab(PathBuf), Quit }
enum DiskChange { Ignore, Resync, Conflict, Reload }
fn classify_disk_change(...) -> DiskChange  // pure, unit-tested
fn render_source_editor_ui(ui, tab)         // TextEdit + caret consumption
```

## Limitations / Next steps

- Conflict banner only shows when the conflicted tab is active.
- Source mode uses plain TextEdit: no syntax highlighting yet
  (`egui_code_editor` 0.2.x is egui-0.33-compatible if wanted), no inline
  preview.
- Autosave is intentionally absent; explicit Ctrl+S only.
- Phase 2 (section click-to-edit) and Phase 3 (block-level live preview) are
  scoped in `docs/research/editing-approaches.md` §6.
