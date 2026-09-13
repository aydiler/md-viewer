# Fix: reserve a table's own height instead of everything below the cursor

**Status:** ✅ Complete
**Branch:** `fix/table-height-reserve`
**Date:** 2026-09-04

## Summary

Clicking a heading in the outline scrolled past it. On a 120-section document
with a 12-row table per section, clicking "Section 30" put the *end* of section
30 at the top of the viewport — `entry_30_11`, `entry_30_12`, "Closing text for
section 30" — with the heading itself scrolled off above. Roughly one section of
overshoot, about a full viewport on this document.

## Root cause

`egui_extras` accounts for the rows it skips only once its loop reaches the
first *visible* row (`heterogeneous_rows` → `add_buffer`). A table lying
entirely above the visible range never reaches that point and reserves nothing,
collapsing to zero height. Every block below it then lays out too high.

An outline click forces a full paint at the estimated offset so headings can
record their y. That paint is scrolled, so any table above the viewport has
collapsed and the recorded positions are wrong.

The table scope previously reserved `ui.max_rect().bottom()` — "everything
below the cursor" — which does not describe the table's actual extent. Stating
the real height makes the table occupy its space whether or not its rows were
culled.

## Provenance

The analysis and the approach come from uncommitted work on
`fix/viewport-slice-layout`, which also carries a *different*, committed
approach to the same area: discarding position recordings made during a
scrolled paint. That one is deliberately **not** included here — it is a
workaround for the distortion this change removes at the source, and with the
distortion gone it would only discard legitimate re-measurements.

That branch's commit message describes a 3x undershoot ("Section 30 landed on
Section 10"). That symptom **no longer reproduces**: #113 and #114 removed the
layout distortion it described. What remains is the overshoot measured above —
same mechanism, opposite sign, much smaller.

## Key discoveries

### The reservation is necessarily an estimate

Row heights are computed twice, and that is not redundancy. The reservation
runs *before* the table is built and can only use `initial_widths` — the
pre-layout estimate. The existing computation inside `body()` uses
`body.widths()`, the widths the table actually laid out with. Two different
questions at two different times.

So the reserved height can be slightly wrong. Measured on a table with images:
content below the table sits **1 px lower** than on `main` — 0.3 % of a ~330 px
table, in the over-reserving direction. Documents without images are
pixel-identical.

### It is not free of cost, but the cost is unmeasurable here

Doubling the per-table row-height computation sounded expensive enough to check
before proposing it. Over 200 wheel clicks on a 120-table document: **22.71 s
CPU on `main`, 22.64 s with the reservation** — 0.3 %, inside the noise. The
row-height work is small next to painting.

### The symptom in the source branch no longer exists

`fix/viewport-slice-layout` describes a 3x undershoot ("Section 30 landed on
Section 10"). Measured today on `main`: clicking Section 30 lands at the *end*
of section 30 — an overshoot of about one section. Same mechanism, opposite
sign, much smaller, because #113 and #114 removed the larger distortion.

Concluding from "the described symptom is gone" that "the branch is obsolete"
would have been wrong, and was the first conclusion drawn here. The click was
still landing in the wrong place; only the direction had changed.

## Verification

| check | result |
|---|---|
| Outline click, "Section 30" | `main`: end of section 30. This branch: **the heading** |
| Scroll CPU, 200 wheel clicks | 22.71 s → 22.64 s |
| Tables without images | pixel-identical |
| Tables with images | 1 px vertical shift below the table |
| Frontmatter / math documents | pixel-identical |
| `scripts/scroll-regression.sh` | PASS, bottom at frame 103 (same as `main`) |
| All suites | root 60, renderer 65+1+18+16, backend 27+1 |

The control run matters for the image case: capturing the same build twice is
pixel-identical, so the 1 px is caused by this change and not by async image
loading.

## Future improvements

- The estimate/actual width split is the only source of the 1 px. Reserving
  from the same widths the table lays out with would need the reservation to
  happen after `body()`, which is exactly what it cannot do.
- Only Markdown tables reserve. `render_html_table` uses a separate path and
  still reserves "everything below the cursor".
