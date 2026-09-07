# 067 — A permanent probe for slice *range selection* (issue #140)

**Branch:** `diag/140-split-probe`
**Date:** 2026-09-07
**Status:** complete; held until v0.2.0 is tagged

## Why

`MDV_DIAG_SLICE` reports where a viewport slice is *placed*. On #167 it did its
job perfectly and was still not enough: it reported **zero** off-screen
placements in both the working and the broken run. That true negative correctly
excluded the placement hypothesis — and could say nothing about what was
actually wrong.

The answer was in how the event *range* is chosen:

```rust
let below = split_points.partition_point(|(_, start, _)| start.y <= viewport.max.y);
let last_event_index = split_points.get(below + 1)...;
```

`below` had collapsed to 1, so the range ended at event 11 of 63 and everything
below the frontmatter table went unpainted. A throwaway probe showed that in one
frame. This makes it permanent, because #140 is still open and is the same class
of failure.

## What it reports

Every frame, one line:

```
DIAG split viewport=[876,1626] extent=1626 above=10 below=17 events=[32,134)/134 sp=17
```

`extent` is `page_size.y`, the height the bootstrap pass measured. #140's
candidate path is a stored scroll offset that briefly exceeds the updated
extent, so the viewport and the extent are printed side by side and the line is
marked `OFFSET>EXTENT` when the former runs past the latter.

The split-point table is dumped only when the selection looks degenerate — an
empty range, or one stopping at the first boundary while the viewport reaches
further. Dumping 17 rows every frame would bury the signal on a long document.

**The summary line prints unconditionally** while enabled, so its silence means
the probe was not running, not that nothing happened. That distinction is the
whole reason the existing probe is written the same way.

## Verification

Both halves, same document, same harness:

| run | DIAG lines |
|---|---|
| `MDV_DIAG_SPLIT=1` | **486** |
| without the variable | **0** |

On the healthy run, `OFFSET>EXTENT` and degenerate-table dumps were both 0 —
genuine zeroes from a probe proven to be reporting, not an unimplemented check.
At the document bottom the line reads `viewport=[876,1626] extent=1626`: equal,
never exceeding, which is the post-render clamp behaving.

## Cost

One relaxed bool load per painted slice when disabled, matching
`diag_report_slice`. Same `OnceLock` pattern.

## Release timing

Touches the vendored renderer (workspace 0.28.0, unpublished). Held until the
`v0.2.0` tag so a verified release is not disturbed.

## Files

- `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`
