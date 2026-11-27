# Fix: MiTeX commands that failed with `unknown variable`

**Status:** ✅ Complete
**Branch:** `fix/mitex-commands`
**Date:** 2026-11-25
**Lines Changed:** renderer `egui_commonmark_backend/src/misc.rs` (`MITEX_PREAMBLE`, `MITEX_NAME_FIXES`, `fix_mitex_names`, tests), `egui_commonmark_backend/Cargo.toml` (dev-dependency)

## Summary

md-viewer converts LaTeX math to Typst with the `mitex` crate (0.2.4) and
compiles the result with its own preamble, `MITEX_PREAMBLE`. MiTeX emits calls
such as `pmod(b)` for `\pmod{b}`. In the MiTeX Typst package those functions
live in its `standard.typ` scope, which md-viewer does not load, so every one
the preamble left out failed the whole formula with `unknown variable: pmod`.
#298 (RichardCao) fixed the `\big` family that way; reviewing it turned up 44
more by hand. A test over MiTeX's whole spec found 124.

## What was measured

`every_mitex_command_resolves` renders one invocation of every command and
environment in `mitex_spec_gen::DEFAULT_SPEC` (the spec `convert_math(_, None)`
uses), with placeholder arguments of the shape the spec asks for. On main's
preamble 124 failed with `unknown variable`:

- **Handlers in `standard.typ`:** `Bra Braket Ket Set bra braket ket set`,
  `bcancel xcancel sout`, `cfrac dbinom tbinom`, `colorbox fbox hbox raisebox`,
  `hphantom phantom vphantom mathclap`, `hspace vspace pmod pod middle`,
  `mathbin mathclose mathinner mathop mathopen mathord mathpunct mathrel`,
  `mathring overgroup undergroup overleftharpoon overrightharpoon
  overleftrightarrow overlinesegment underbar`, `substack underset`.
- **Symbols and operators:** `TeX LaTeX KaTeX`, `arctg arcctg ch cosec cotg cth
  sh th`, `argmax argmin injlim projlim plim`, `smallint thickapprox thicksim`,
  `enspace negmedspace negthickspace negthinmedspace`, the eleven `\var…`
  Greek capitals, and `mho` (MiTeX emits `ohm.inv`, which Typst 0.14 renamed
  `Omega.inv`).
- **Greedy and style commands:** `cal frak tt color textcolor`, `textsf texttt
  textup textmd textnormal`, `overbracket underbracket`.
- **Extensible arrows:** the 17 `\x…arrow` commands other than `\xrightarrow`
  and `\xleftarrow`, and `xlongequal`.
- **Infix:** `atop brack`.
- **Environments:** `array subarray matrix smallmatrix rcases alignedat`.
- **Not meaningful in one formula:** `ref eqref cite`, `toprule midrule
  bottomrule textwidth`.

Three more failed with other errors: `\begin{Vmatrix}` (the preamble's
`delim: "||"` is two characters; Typst wants `‖`) and `\dashleftarrow` /
`\dashrightarrow` (`arrow.l.dash` is `arrow.l.dashed` in Typst 0.14). And
`\brace` compiled but was wrong: `brace` is a Typst symbol, so `{n \brace k}`
drew a stray `{` followed by `(n, k)`.

## Fix

- **The preamble** defines each missing function, ported from its handler in
  `standard.typ` and grouped by kind. Deviations from MiTeX are commented where
  they are made:
  - `\mathop` takes the `large` class (LaTeX's large operator, limits in
    display style), not MiTeX's `unary`.
  - `\xcancel` is a cross (`cancel(cross: true)`), as in LaTeX; MiTeX draws one
    stroke.
  - `\fbox` pads by 3pt, LaTeX's `\fboxsep`.
  - Colour commands keep their content and drop the colour. A formula is drawn
    as one-colour ink in the theme's text colour (`render_math_formula`
    composites by alpha), so no colour can show, and a filled `\colorbox`
    would cover its own content.
  - `\ref`/`\eqref` show `??` and `\cite` shows `[?]`, which is what LaTeX
    prints for an undefined label or citation; a single formula has none.
  - Extensible arrows stretch to their label, 1em longer, as in LaTeX. MiTeX
    uses the `@preview/xarrow` package, which this engine cannot download. The
    existing `\xrightarrow`/`\xleftarrow` (a fixed-length arrow with the label
    above) now stretch too.
  - `\vphantom` gets a wide hidden box, and `\mathclap` keeps its content at
    full width; see Key Discoveries.
- **`MITEX_NAME_FIXES` / `fix_mitex_names`** rewrite five names in MiTeX's
  output that no preamble can serve: `set(` → `mitexset(` (keyword), `brace(` →
  `mitexbrace(` (symbol), the two dashed arrows and `ohm.inv`. The rewrite
  skips `#name[…]` text blocks, so a `\text{}` that happens to contain one of
  the names is left alone.
- **`Vmatrix`** uses `‖`.

## Key Discoveries

### `convert_math`'s spec argument does not rename commands

The first fix for `set`/`brace` passed a patched spec (same commands, new
aliases) to `mitex::convert_math`. Nothing changed: `convert_inner` (mitex
0.2.4, `converter.rs`) parses with the spec it is given but builds its
`TypstRepr` with `DEFAULT_SPEC`, which is where command names come from. The
alias test passed, because the patched spec did hold the new aliases; only
rendering showed the call was still `set(x )`. Hence the output rewrite.

### Only the short arrows stretch

`stretch(arrow.r.long)` stays 22pt under a 96pt label: New Computer Modern Math
has an extensible construction for `→` but not for `⟶`. The arrow test measures
the arrow itself (the lowest band of ink rows against the label's band); a
formula-width check passed with the arrows unstretched, because the label sets
the width either way. All 19 arrows now span their label plus about 1em.

### A zero-width box wraps its content

MiTeX's `\vphantom` and `\mathclap` are `box(width: 0pt, …)`. The content gets
no room, so an inline equation inside breaks at every relation:
`\vphantom{a = b = c = d = e = f = g}` was 279pt tall against 27pt for the same
content on one line, and `\sum_{\mathclap{1 \le i \le n}}` put its subscript on
three lines. A wide box inside (`box(width: 0pt, hide(box(width: 10000pt,
it)))`) keeps `\vphantom` on one line; its overflow is hidden anyway. A box lays
math out in text style, so in a display formula `\vphantom{\frac{a}{b}}` takes
the height of an inline fraction. Centring visible content over a zero width
failed both ways tried (`align(center)` and `h(1fr)` inside the wide box), so
`\mathclap` is its content.

## Tests

In `misc.rs`, all behind the `math` feature:

| test | what it pins |
|---|---|
| `every_mitex_command_resolves` | every command and environment of MiTeX's spec renders, except ten listed with their reason (`hspace`, `vspace`, `raisebox` need a real length; `includegraphics`, `item`, `tabular`, the booktabs rules and `textwidth` have no meaning in one formula). Each listed one must still fail, so the list cannot go stale. Red on main with the 124 above. |
| `renders_ported_mitex_commands` | each ported command and environment with real arguments |
| `ported_mitex_commands_have_their_latex_sizes` | `\hspace{2em}` is 16pt wider than `{1em}`; `\phantom` takes space and paints nothing; `\vphantom` stays one line tall and still adds height; `\dbinom` is taller than `\tbinom`; `\atop`, `\brace`, `\brack`, `\choose` stack; all 19 arrows span their label; `\texttt` is monospace |
| `mitex_name_fixes_match_what_mitex_emits` | each name fix still matches what mitex emits for its command, so a mitex upgrade that fixes one shows up |
| `fix_mitex_names_leaves_text_and_longer_names_alone` | text blocks, `subset(`, `arrow.r.dashed` untouched |

Mutations, each run against the whole backend test binary:

| mutation | caught by |
|---|---|
| drop the eight class definitions | spec test, ported test |
| `\hspace` ignores its argument | sizes, spec test (the exception renders) |
| `\dbinom` in inline style | sizes |
| name rewrite off | five tests |
| arrows unstretched | sizes |
| `\texttt` as identity | sizes |
| `\phantom` shows its content | sizes |
| MiTeX's `\vphantom` | sizes |

Suites: renderer with the app's features, backend with no default and with all
features, renderer defaults, app tests; clippy (no warning in changed lines;
app `-D warnings` clean); fmt. Test inventories: backend 33 → 38 (the five
above), app unchanged. `cargo publish --dry-run` of the backend passes with the
new dev-dependency.

## In the app

Xvfb, three fixture pages covering every group, dark and light, this branch
against main (#298 included): main shows the `unknown variable` box for the
first formula of each kind; the branch renders all of them, the dashed arrows
and `\mho` included, with the arrows stretched under "long label".

## Future Improvements

- [x] `cases`/`rcases` put no gap between `a & x > 0`'s columns (Typst's
  `cases` treats `&` as an alignment point only); LaTeX puts a quad there.
  Fixed in devlog 126.
- [ ] Display-style `\vphantom`, if Typst gets a way to lay math out in a
  zero-width box without wrapping. `\mathclap` stays at full width even then:
  zero width would put a clapped subscript's ink outside the formula's frame
  at its edges, and the rasterized image cuts that off (devlog 126).
- [ ] Report upstream (mitex 0.2.4): the ignored spec argument, `\set` as a
  keyword, `\xcancel` as a single stroke, and the symbols Typst 0.14 renamed.
