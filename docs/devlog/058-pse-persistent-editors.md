# Feature: 058 — PSE Persistent Styled Editors (Live mode rewrite)

**Status:** ✅ Complete (v1)
**Branch:** `feature/editing`
**Lines Changed:** ~+400 / −260 across `src/main.rs`, fork `misc.rs`/`pulldown.rs`/`styler.rs`

## Summary

Implements architecture C from `docs/research/editing-architecture-overview.md`:
Live mode no longer swaps a single editor in and out. **Every text block
(heading, paragraph, quote, list) is permanently a styled TextEdit**, laid out
in document flow; non-text blocks (images, math, mermaid, tables, code fences)
continue rendering through the normal pipeline between editors. The swap jump,
focus race, and click-position bugs of the previous design are structurally
impossible here — clicks land natively in whichever editor is under the
cursor.

## Design

- **Fork**: `EditSessionConfig { id_salt, blocks: Vec<SessionBlock{src,kind}> }`
  via `viewer.edit_session(...)`. Paint loop detects which session block each
  event belongs to; text blocks paint a TextEdit bound to per-block egui-temp
  buffers (seeded from source on first sight) with the markdown-aware
  layouter; non-text blocks process normally. Per-block feedback
  (`index/text/changed`) is collected into a per-frame map, flushed to the
  cache at end of show(), read non-destructively by the app.
- **App**: `Tab.session_buffers/kinds/dirty/last_blocks`. Entering Live seeds
  buffers from serialized content (Heading strips `# `, Quote strips `> `);
  keystrokes mutate buffers only; serialization folds everything back —
  markers re-materialized — on Ctrl+S or mode exit. Cache-refresh gaps fall
  back to last known blocks so editors never flicker away.
- Retired for PSE: active-block anchor, calibration bias, boundary hit-test,
  synthetic-click gate (kept env hooks for future e2e).

## Gotchas

- egui multi-pass layouts run closures more than once → feedback must be
  read non-destructively (`get_session_feedback`) and change detection done
  by comparing buffer text, not flags.
- Session state must reset on `reload()`/`load_file()` or stale buffers paint
  over fresh content.
- Fork examples don't compile in this vendored tree (upstream crate names) —
  pre-existing; test with `--lib --tests`.

## Verification

- Headless e2e `live_session.rs`: 4 editors painted; keystroke routed to
  focused block only; others untouched.
- Suites: app 57 ✓ · backend lib 24 ✓ · frontend 37 + live_preview 3 +
  live_session 1 ✓. Clippy clean on touched code.

## Known limitations (v1)

- List blocks edit as one raw unit (markers visible); per-item editing later.
- Search/outline operate on last-serialized content while typing.
- Arrow-key crossing between blocks not implemented.
