# Fix: Search-active scroll lock (issue #19)

**Status:** ✅ Complete
**Branch:** `fix/search-scroll-lock`
**Date:** 2026-05-23
**Lines Changed:** +41 / -15 in `src/main.rs`
**Issue:** #19

## Summary

With the find bar open, wheel-scrolling away from the active match snapped the view back to the match every frame, leaving the user "locked" near the active result. Closing the bar (Esc) restored scrolling. The fix is a one-shot guard on the post-render corrective scroll: it now fires once per `scroll_to_active_match` call and stops fighting subsequent user input.

## Root Cause

The post-render corrective scroll block at `render_tab_content` (around `src/main.rs:2554`) was designed as stage 2 of a two-stage scroll: `scroll_to_active_match` sets `pending_scroll_offset` from a line-ratio estimate (gets the view roughly in the right area), then the next frame the renderer records the active match's real content-y via `cache.record_active_search_y_viewport`, and the corrective block uses that precise y to snap. Worked when the system was originally designed.

Two interacting facts made it perpetual after commit `21d43c5` ("Disable show_scrollable virtualization"):

1. **`active_search_y` never expires.** It's set whenever the renderer paints an Active highlight segment (`crates/egui_commonmark/.../pulldown.rs:1372`) and cleared only when the active range changes or search closes. With virtualization disabled, the renderer walks the full event stream every frame — egui's clip rect skips painting but not widget layout, so `record_active_search_y_viewport` fires every frame, keeping `active_search_y` perpetually fresh and accurate even when the match is off-screen.

2. **The corrective block had no guard for "user just scrolled."** It only checked `if let Some(actual_y) = tab.cache.active_search_y()`. Once the user scrolled the match out of viewport, the block snapped back to it next frame, undoing the user's wheel input. Loop forever.

Pre-`21d43c5` (virtualization enabled), `active_search_y` only got recorded when the active match was in the rendered viewport slice. Scrolling past it left a stale value but didn't matter because the LESSONS entry "Outline scroll-to: virtualization breaks the corrective y-record loop" already noted off-screen blocks didn't paint. Disabling virtualization shipped this regression as a side effect — `record_active_search_y_viewport` started firing every frame for any active match.

## Repro

`/tmp/search-repro.md` — 3 sections with `findme` on each page.

```bash
md-viewer /tmp/search-repro.md
# Ctrl+F → findme → try to wheel-scroll past page 1
```

For automated repro on Xvfb, an internal `--debug-search QUERY` CLI flag was added (then stripped from the final commit) that opens the find bar at startup. With it, 500 `xdotool` wheel-down events produced 16 px of net scroll before the fix; 213 of those frames had the corrective block set `pending_scroll_offset = Some(0.0)` (snap-back to match 1). After the fix, the same 500-event test scrolled ~2800 px (reached page 3); 0 frames fired the corrective.

## Fix

One-shot `bool` flag on `Tab`:

```rust
struct Tab {
    // ...
    correct_active_search_pending: bool,
    // ...
}
```

Set by `scroll_to_active_match` (called from `jump_match` and `maybe_rebuild_search`):

```rust
tab.pending_scroll_offset = Some((estimated_y - margin).max(0.0));
tab.correct_active_search_pending = true;  // grant one frame of corrective permission
```

Gates the corrective block, which clears the flag after running once (whichever branch):

```rust
if tab.correct_active_search_pending {
    if let Some(actual_y) = tab.cache.active_search_y() {
        // ... compute needs_correction, set pending_scroll_offset if needed ...
        tab.correct_active_search_pending = false;  // one-shot consumed
    }
}
```

The two-stage scroll still works because the flag stays `true` until the corrective block has had its chance:

- Frame N (`jump_match` or query change): `scroll_to_active_match` sets `pending_scroll_offset` + flag.
- Frame N+1: ScrollArea applies pending → renderer paints, records `active_search_y` → corrective block reads it, either snaps (`needs_correction=true`, delta > 2 px) or no-ops; flag cleared.
- Frame N+2+: flag is `false`; user wheel input is no longer overridden.

## Key Discoveries

### `record_active_search_y_viewport` fires even when the match is off-screen

egui's clip rect culls *painting*, not widget layout. The renderer's `event_text_with_highlights` walks every event and calls `record_active_search_y_viewport` for each Active segment regardless of viewport intersection. Once virtualization was disabled, that means it fires every paint for the active match — making `active_search_y()` perpetually fresh, perpetually accurate, and perpetually load-bearing for the corrective-scroll loop.

### Pre-existing one-shot pattern: `pending_header_click_key.take()`

The outline-click corrective scroll already uses `.take()` on an `Option<String>` for the same one-shot semantics. The search-corrective block could have followed that pattern (`Option<()>` taken), but a `bool` matches the existing `correct_*_pending` naming convention better and reads less weird than `Option<()>` at a setter site.

## Architecture

### Modified Struct

```rust
struct Tab {
    // ...
    correct_active_search_pending: bool,  // new — one-shot for corrective scroll
    // ...
}
```

### Modified Functions

| Function | Change |
|----------|--------|
| `scroll_to_active_match` | Sets `correct_active_search_pending = true` after setting `pending_scroll_offset` |
| `render_tab_content` (corrective block) | Gates the existing `if let Some(actual_y) = ...` on the flag; clears the flag inside the gate |

## Testing Notes

Manual: confirmed by user on real display with `/tmp/search-repro.md`. Reported "working."

Automated repro (pre- and post-fix): `Xvfb :99` + `xdotool mousemove ... click --repeat 500 5` over the content area, with a temporary `--debug-search findme` CLI flag and `eprintln!` traces in the corrective block. Pre-fix: 213 FIRING events, max `cur` of 406 before snap-back, screenshot still showed page 1. Post-fix: 0 FIRING events after the initial paint, screenshot showed page 3 ("End of page 3" visible). Tracing and the CLI flag were stripped before the final commit.

## Future Improvements

- **`MCP keystroke injection`**: the only way to repro this through MCP was a temporary CLI debug flag plus `xdotool` for the wheel. An `egui_keystroke` MCP primitive would make this and any other keyboard-shortcut-only feature testable end-to-end without scaffolding. Tracked cross-repo in `~/dev/mcp/egui-mcp/`.
- **Consolidate the two corrective-scroll blocks**: the outline-click block (`pending_header_click_key.take()`) and the search block (`correct_active_search_pending` bool) share a structure. A small helper that takes a `cache.get_recorded_y()` closure plus the source flag would deduplicate, but the two diverge in `inset` (35% of viewport vs. fixed −50 px) and in whether the flag is `Option<String>` (the key) or `bool` (just a permission). Not worth a refactor yet.
