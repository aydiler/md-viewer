# Fix: `cases` columns are a quad apart

**Status:** ✅ Complete
**Branch:** `fix/cases-columns`
**Date:** 2026-11-27
**Lines Changed:** renderer backend `misc.rs` (`MITEX_PREAMBLE`, tests)

## Summary

A follow-up from devlog 124. In `\begin{cases} a & x > 0 \\ b & x \le 0 \end{cases}`
each value touched its condition. amsmath's `cases` is an array whose two
left-aligned columns are a quad apart (`@{}l@{\quad}l@{}`). MiTeX turns the
environment into `cases(a & x > 0, b & x <= 0)` (mitex 0.2.4,
`converter.rs`: `&` stays `&`, `\\` becomes `,`), and Typst's `cases` takes
`&` as an alignment point only. Measured at 3 px per pt and 16 pt, the ink
gap between the columns was 3 px, the side bearings alone.

## Fix

The preamble defines `cases` and `rcases` itself. `mitex-cases-row` adds
`h(1em)` after each top-level alignment point of a row, and both call
`math.cases` with the rows changed, passing named arguments through. The gap is
now 51 px, the 3 px plus one quad (48 px).

## Key Discoveries

### How `&` looks in the content tree

`$a & x > 0$.body` is a `sequence` whose children are `[a]`, `[ ]`,
`align-point()`, `[ ]`, `[x]`, … The element is not reachable as
`math.align-point`, so the row helper compares `repr(child.func())` with
`"align-point"`. A row without `&` is a single element, not a sequence, and is
left alone.

### A preamble `#let` shadows a math builtin

`#let cases(..) = …` in the preamble takes precedence over `math.cases` inside
`$…$`, so no output rewrite (`MITEX_NAME_FIXES`) was needed.

### The first column was already left-aligned

The plan assumed Typst right-aligns the column before `&`, as an `align`
environment does. Measured, `1` and `1000` in the same column start at the same
x. `cases_align_their_first_column_left` is therefore a guard that passed
before the fix too, not a red-to-green test.

### 1em is 48 px in the test images

`render_math_formula` rasterizes at 3 px per pt, so a 16 pt formula has 48 px
per em. The first threshold assumed 16 px per em and failed against the fixed
build at 51 px.

### `\mathclap` stays at full width

Devlog 124 listed `\mathclap`'s missing zero width as a follow-up. LaTeX lets
the content overlap its neighbours. Here the formula is rasterized from its
frame, with no horizontal padding for inline formulas, so ink that a zero width
pushed past the frame at a formula's edge would be cut off. Full width can
widen spacing a little but never loses ink. No code change; the preamble
comment says why.

## Testing Notes

- `cases_separate_their_columns_by_a_quad`: the ink gap between the columns of
  `cases` and `rcases` is within 40–60 px. On main it is 3 px (red).
- `cases_align_their_first_column_left`: the left ink edges of `1` and `1000`
  differ by at most 1 px. Green before and after.
- `renders_ported_mitex_commands` gained `cases` with one column, three
  columns, and `\text` in a cell.
- Mutations: no `h(1em)` → gap test red; `h(2em)` → gap test red (upper bound);
  override removed → gap test red.

## Future Improvements

- [ ] MiTeX's own `standard.typ` maps `cases` straight to Typst's `cases`
  (`define-env(none, kind: "is-cases", alias: "cases")`), so the MiTeX
  package has the same missing gap. Worth adding to the upstream reports.
