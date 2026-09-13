# Feature: Deep-scroll regression guard

**Status:** ✅ Complete
**Branch:** `test/deep-scroll-probe`
**Date:** 2026-09-03
**Lines Changed:** +~200 in `scripts/scroll-regression.sh`, `docs/LESSONS.md`

## Summary

`scripts/scroll-regression.sh` walks a document in fine scroll increments and fails if any frame paints an empty document pane, or if scrolling stops advancing before the bottom is reached.

It exists because neither CI nor `scripts/visual-regression.sh` catches that failure. On `7b8f53b`, the build that issue #121 was filed against, `visual-regression.sh` reports PASS while four of sixty-one captured frames render a completely empty document pane.

## Why a second script instead of more assertions in the first

Two independent reasons, both measured rather than assumed:

1. **Depth.** `visual-regression.sh` walks 7 steps of 10 wheel clicks. On the document from #121 the first blank frame appears around step 14, so the shorter walk never reaches it.
2. **The metric does not generalize.** `visual-regression.sh` flags a horizontal shift when the leftmost painted pixel moves. That is right for its own flush-left fixture, but on a document with an indented list the leftmost element is legitimately further right — it reported a bogus 26 px "shift" on the #121 document. This guard keys off painted-content volume in the document pane plus frame-to-frame difference, which does not depend on document shape.

Both scripts stay: they check different things and fail on different builds.

## Key Discoveries

### A fixture that does not reproduce is worse than no fixture

The first generated fixture — five identical 40-row tables — reached the document bottom at frame 13 and reported PASS on a build with a known, reproducible blank-frame bug. Matching the real document's shape fixed it: twenty tables from 4 to 51 rows, cell text of varying length so row heights differ, and a nested index list at the top. That pushed the bottom out to frame 31, comparable to the real document's 35.

### Granularity was the second trap

Even the corrected fixture reported PASS at 60 steps of 10 clicks. The blank states occupy narrow scroll windows and coarse steps jump straight over them. At 150 steps of 3 clicks the same fixture on the same binary fails. The real document was more forgiving only because it has more and taller tables, offering more windows to land in.

Defaults are therefore 150 × 3, not 60 × 10.

### The validation matrix

A scroll guard is not trustworthy until every cell is confirmed:

| build | fixture 60×10 | fixture 150×3 | #121 document 60×10 |
|---|---|---|---|
| `7b8f53b` (pre-#113) | PASS — useless | **FAIL** | **FAIL**, 4 blank frames |
| `c5ea1a6` (post-#113) | PASS | **FAIL** — see #125 | PASS |

### Distinguishing "stuck" from "at the bottom"

A run of identical frames at the tail is the document bottom and is correct; the same run in the middle means scrolling stopped advancing. The analysis finds the tail run first, then only flags non-advancing frames before it.

### Measure the document pane, not the window

Explorer and outline keep painting when the document pane goes blank, so a whole-window measurement hides exactly the failure being hunted. The analysis crops to the pane between them.

## Result

The guard found a real bug on its first run: a blank pane at the Section 6/7 table boundary on the post-#113 build, unaffected by #113 and reproducing identically on `7b8f53b`. Filed as #125; #114 fixed it. Green on `main` as of 7a6346c.

Tuning the guard until it passed would have reproduced the #96 mistake documented in LESSONS.md, where a regression test asserted the buggy behaviour as correct. Shipping it red was the right call at the time.

### Verifying the fix needed a control run

#114 changes table layout, so a clean run on the new build could just mean the blank moved out of the walk's path. Three measurements settle it:

| build | mode | result |
|---|---|---|
| post-#113, pre-#114 | default | blank at f032 (196 content px) |
| `main` 7a6346c | Full Width off | no blanks, bottom at frame 103 |
| `main` 7a6346c | Full Width on | no blanks, f032 at 3152 content px |

The first row is the control: the same harness, same session, same display still reports the blank on the old build, so the PASS on the new one is real. The two width modes are two different table geometries, so the absence is not an artefact of the one layout #114 optimises for.

Now that it is green, it is a candidate for CI. Left out for the moment because it needs Xvfb, xdotool and ImageMagick, and takes roughly a hundred screenshots per run.

## Harness pitfalls

Inherited from `visual-regression.sh`, all of which silently produce wrong evidence: match process *names* (a `pkill -f` pattern matches the script itself), assert exactly one window (screenshots of an obscured X11 window return the overlapping window's pixels), isolate `XDG_DATA_HOME`/`XDG_CONFIG_HOME` (md-viewer persists scroll position), and redirect a backgrounded `Xvfb` (it holds the pipe open and hangs `script.sh | tail`).

One more, learned here: **do not edit a shell script while a run of it is in flight.** bash reads scripts lazily and will execute the edited bytes mid-run.
