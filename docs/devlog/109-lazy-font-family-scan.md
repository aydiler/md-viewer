# Perf: scan the font picker's families when the dialog opens

**Status:** ✅ Complete
**Branch:** `perf/lazy-font-family-scan`
**Date:** 2026-11-21
**Lines Changed:** `src/main.rs` (`FontFamilyList`, `spawn_font_family_scan`, `MarkdownApp::new`, `render_font_settings`, `update`, tests)

## Summary

`MarkdownApp::new` started a `font-family-scan` thread at every launch. The
thread runs `scan_pickable_font_families` (`src/system_fonts.rs`), which loads
a face for every installed family to check that it covers Latin. Callgrind
measured it at 1.95 G instructions (devlog 103's session). Its result feeds one
widget, the family list in View → Fonts, which most sessions never open. On a
2-core machine it competed with the first frames.

The scan now starts the first time the dialog opens, at most once per run.

## Architecture

- `FontFamilyList` replaces the three fields `available_font_families`,
  `available_font_families_lower` and `pending_font_family_scan`:
  - `request(spawn)` starts the scan unless this run already started one;
  - `scanning()` drives the existing "Scanning installed fonts…" spinner;
  - `poll()` is the old `try_recv` block from `update()`, including the
    disconnected case, and returns true when the list arrives so `update()`
    repaints.
- `spawn_font_family_scan()` is the old spawn code from `new()`, with the same
  thread name. `render_font_settings` calls `request` once the dialog is open,
  which covers every way it can open.

The dialog needed no new UI: it already showed a spinner when it was opened
before the startup scan finished.

## Measurements

Release builds of main (e7005c9) and the branch, launched alternately with the
README on a private Xvfb. Per launch: the process's `utime + stime` after 3 s
(this includes threads that have exited), and whether a thread named
`font-family-sca` (`comm` is cut to 15 characters) appeared.

| | main | branch |
|---|---|---|
| CPU after 3 s, median of 15 | 1.13 s | 0.82 s |
| minimum | 1.07 s | 0.79 s |
| scan thread seen | 15/15 | 0/15 |

A first set of 5 runs per build gave the same direction (median 1.19 → 0.97 s)
with more noise. One branch run took 1.86 s while the machine was loaded, which
is why the 15-run set is the one reported.

Dialog on the branch build (xdotool, screenshots):

- before opening it, there is no scan thread;
- 0.15 s after clicking View → Fonts, one scan thread and the spinner;
- 3 s later, the thread is gone and the full list shows;
- picking "Adwaita Sans" applies it;
- closing and reopening shows the list at once, with no thread and no spinner.

## Tests

- `the_font_family_scan_does_not_start_at_startup`: a new `FontFamilyList` has
  not requested a scan.
- `the_font_family_scan_starts_once_and_delivers`: the spawn closure runs once
  across two requests, and `poll` fills `names` and `names_lower`.
- `a_scan_that_ends_without_a_result_stops_scanning`: a dropped sender ends the
  spinner.

Mutants observed red:

- removing the once-guard in `request` fails the second test;
- keeping `pending` on `Disconnected` fails the third.

The first test checks the type's initial state only. A request added to
`MarkdownApp::new` would not fail it, because no test can build the app
without an eframe context. The thread check above is what covers that.

## Future Improvements

- Prefetch on hovering View → Fonts, if the half-second spinner on first open
  is ever reported as annoying.
