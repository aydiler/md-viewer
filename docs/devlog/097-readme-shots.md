# 097 — README screenshot touch-up

**Branch:** docs/readme-shots
**Date:** 2026-11-16
**Status:** Done

## Scope
Retake four README screenshots on main @ 9cef86c, which now has #247 and #248:

- `dark-mode.png` / `light-mode.png`: the caption promised mermaid diagrams, but only the top box of the Pipeline flowchart was on screen. Scrolled three wheel steps so the whole flowchart and the end of the highlighted code block are visible, at the same offset in both themes. Caption now names what is shown.
- `tables.png`, `view-menu.png`: taken before the inline-code column floor (#248), so the `budget` chip touched its column divider. Retaken with the floor.

## Key Discoveries
- The demo folder `/tmp/mdv-demo` had picked up two test fixtures from the #247/#248 work (`tablecode.md`, `tablemath.md`). They showed up in the explorer and were moved out before capturing.
- egui shows the floating scrollbar while the pointer is over the document. Park the pointer in a corner before each capture.

## Future Improvements
- None.
