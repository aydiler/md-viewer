# Feature: Keyboard document scroll keys

**Status:** Implemented; validation recorded
**Branch:** `feature/issue-29-keyboard-scroll`
**Date:** 2026-06-03
**Issue:** aydiler/md-viewer#29

## Summary

Keyboard document scrolling has been implemented in `src/main.rs` so users can move the active markdown document without the mouse. This docs task records the behavior, implementation flow, and latest validation evidence only; it does not mark the PR as complete.

## User-Facing Behavior

- Up/Down scroll the document by a fixed line step when the find bar is closed.
- Page Up/Page Down scroll the document by a viewport-relative page step.
- Search keeps its existing arrow-key behavior while the find bar is open.
- Ctrl/Alt/Command-modified keypresses remain reserved for existing shortcuts instead of document scrolling.

## Chronology

- Task 1: Code implementation added keyboard document scrolling in `src/main.rs`.
- Task 2: Review validated formatting, focused keyboard-scroll tests, full tests, build, clippy, and source diff whitespace.
- Task 3: Repo-local docs updated for shortcuts, architecture flow, devlog status, and one durable lesson.

## Architecture Notes

Keyboard shortcuts are collected in `MarkdownApp::update` as deferred actions. Document scroll keys set a `KeyboardScrollAction`; after input collection, the active tab's current `scroll_offset`, `last_viewport_height`, and `last_content_height` are passed to `keyboard_scroll_target`. The resulting clamped y offset is stored in `pending_scroll_offset`, letting the existing renderer-owned `ScrollArea` apply the movement on the next render pass.

The shortcut handling is intentionally gated by UI state and modifiers: the find bar keeps arrow keys for match navigation, and Ctrl/Alt/Command-modified keypresses stay available to existing app/system shortcuts.

## Validation Evidence

Fresh closeout validation on 2026-06-03:

- `cargo fmt --check --manifest-path /home/akiro/Coding/md-viewer-keyboard-scroll/Cargo.toml` — PASS no output.
- `git -C /home/akiro/Coding/md-viewer-keyboard-scroll diff --check` — PASS no output.
- `cargo test --manifest-path /home/akiro/Coding/md-viewer-keyboard-scroll/Cargo.toml` — PASS 19 passed; pre-existing vendored warnings observed.
- `cargo clippy --manifest-path /home/akiro/Coding/md-viewer-keyboard-scroll/Cargo.toml --all-targets --all-features` — PASS; pre-existing vendored warnings observed.
- `cargo build --manifest-path /home/akiro/Coding/md-viewer-keyboard-scroll/Cargo.toml` — PASS; pre-existing vendored warnings observed.

Earlier targeted validation from Task 2 review:

- `cargo test --manifest-path "/home/akiro/Coding/md-viewer-keyboard-scroll/Cargo.toml" keyboard_scroll_target -- --nocapture` — PASS 4 passed.

Manual UI validation on 2026-06-04:

- Launched `/home/akiro/Coding/md-viewer-keyboard-scroll/target/debug/md-viewer /home/akiro/Coding/md-viewer-keyboard-scroll/docs/LESSONS.md` from this session; process exited cleanly after user closed it.
- User confirmed keyboard scrolling behavior: "perfect, works great."

## Files Updated In This Docs Task

- `docs/KEYBOARD_SHORTCUTS.md`
- `docs/ARCHITECTURE.md`
- `docs/devlog/034-keyboard-scroll-keys.md`
- `docs/LESSONS.md`

## Future Improvements

- Add automated E2E UI evidence if project scope later requires it.
- Consider documenting Home/End only if they are implemented in a future keyboard-scroll expansion.
