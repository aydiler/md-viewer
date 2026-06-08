# 038 — Math rendering: missing mitex commands + over-aggressive currency filter

**Branch:** `feature/math-rendering`
**Date:** 2026-06-08
**Status:** Implemented, verifying

## Problem

A math-heavy document (`res/quintom/MODEL.md`, a DESI dark-energy writeup) rendered with
visible LaTeX errors (red "error" fallback boxes) and many formulas that "looked off"
(stray leading `$`, unstyled plain text). Diagnosed by running all **399** math spans of
the doc through the real `mitex → typst` pipeline.

Two independent failure classes:

### Class 1 — hard render failures (16 spans → error boxes)

md-viewer renders math by converting LaTeX → Typst with `mitex`, then compiling with a
hand-written `MITEX_PREAMBLE` (`egui_commonmark_backend/src/misc.rs`) that defines the
helper functions mitex's output references. Four commands the doc uses are emitted by
mitex as calls/identifiers the preamble never defined, so typst aborts with
*"unknown variable"*:

| LaTeX | mitex output | fix |
|---|---|---|
| `\tfrac12` | `tfrac(1, 2)` | `#let tfrac(num, denom) = math.inline(math.frac(num, denom))` |
| `\dfrac{a}{b}` | `dfrac(a, b)` | `#let dfrac(num, denom) = math.display(math.frac(num, denom))` |
| `\boxed{…}` | `boxed(…)` | `#let boxed(it) = box(stroke: 0.6pt, inset: (x: 4pt, y: 3pt), $it$)` |
| `\!` | `negthinspace` | `#let negthinspace = h(-0.16667em)` |
| `\xrightarrow{…}` | `xrightarrow(…)` | `#let xrightarrow(label) = $attach(arrow.r.long, t: #label)$` (+ `xleftarrow`) |

(`\,`→`thin`, `\;`→`thick`, `\:`→`med`, `\quad`→`quad`, `\qquad`→`wide` already resolve to
typst builtins — only `\!` had no mapping.)

**Secondary-error lesson:** `\xrightarrow` (§13 boxed display) was initially **masked** —
the same display block also used `\boxed`, which failed *first*, so the diagnostic only
reported `boxed`. After `boxed` was defined, that block compiled further and hit
`xrightarrow`. Takeaway: re-run the **full** diagnostic after every preamble addition,
since fixing the first unknown command in a formula can expose a later one. Final state
verified end-to-end: **405 math spans, 0 failures** on the current MODEL.md.

One span is **not** a preamble gap: `F(\rm today){=}1` (a brace-less `\rm`). mitex
mis-serializes it to `mitexupright(today\)=,1)` (a 2-arg call to a 1-arg helper →
*"unexpected argument"*). Every other `\rm` in the doc is brace-scoped (`_{\rm eff}`,
`_{\rm CPL}`) and converts correctly. Fixed at the source: `\rm today` → `\mathrm{today}`.

### Class 2 — currency false positives (105 inline spans → plain text)

`is_likely_currency()` (added in `c34fffd` to stop `$5`-style amounts rendering as math)
downgraded **any** inline `$…$` lacking `\ { } ^ _` to literal `$text`. That over-fires on
real math: `$w(z)$`, `$-1.38$`, `$D>0$`, `$p=P$`, `$[-1.1,-1.0]$`, `$G$` all rendered as
`$w(z)`, `$-1.38`, … with a stray `$` and no math styling.

## Fixes

1. **Preamble** (`egui_commonmark_backend/src/misc.rs`): four `#let` definitions for
   `tfrac`, `dfrac`, `boxed`, `negthinspace`.
2. **Heuristic** (`egui_commonmark/src/parsers/pulldown.rs`, `is_likely_currency`): before
   falling through to "currency", also return *not-currency* when the span contains a
   relational/grouping operator (`= < > ( ) [ ]`), is a signed number (`-1.38`, `+2.6`),
   is a short all-letters token (`G`, `w`, `D`), or is a **clean numeric literal with no
   internal whitespace** (`8.5`, `0.35`, `10` — an intentional `$number$`). Currency only
   reaches `InlineMath` by spanning two `$` across prose, so its mis-parsed content carries
   spaces or dashes (`"8.5 to "`, `"3,000–"`) and still classifies as currency.
3. **Source** (`res/quintom/MODEL.md` L369): `\rm today` → `\mathrm{today}`.

The clean-number rule (added after live review caught `$8.5$` and `$0.35$` — χ² values in
§8/§13 — rendering as stray `$8.5`/`$0.35`) is low-risk: the original `c34fffd` bug was
currency *spanning prose* (always has spaces/dashes), which this rule does not match.

## Key discoveries

- The math pipeline is `mitex::convert_math` → typst compile of `$ {math} $` with a fixed
  preamble; failures surface as `MathState::Error` and a styled fallback, never a crash.
- mitex maps most amsmath spacing/font commands but **not** `\tfrac`/`\dfrac`/`\boxed`/`\!`.
  The right place to absorb these is the preamble, not the source docs.
- A diagnostic that re-parses a markdown file with `Options::ENABLE_MATH` and pushes each
  `InlineMath`/`DisplayMath` event through mitex+typst reproduces exactly what the renderer
  does — the fast way to audit a doc without launching the GUI.

## Currency heuristic trade-off

The new operator check makes a closed `$…$` containing `( ) [ ] = < >` render as math even
if it was a currency-with-prose misparse (e.g. `$5 (sale) $3`). For a *markdown viewer with
a math feature*, real function/relation notation is far more common than currency that both
spans a `$…$` pair and contains those operators, so the trade favors rendering. Bare amounts
remain protected.

## Future

- Consider making `mitexupright` variadic so a brace-less `\rm` degrades instead of erroring.
- Non-ASCII / fuller amsmath coverage in the preamble as docs demand it.
