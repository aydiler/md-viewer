# Bare URLs become links (GFM extended autolinks)

**Status:** ✅ Complete
**Branch:** `feature/bare-url-autolinks`
**Date:** 2026-11-12
**Issue:** #209

## Summary

GitHub turns a bare `https://…`, `http://…` or `www.…` in prose into a link.
md-viewer showed it as plain text. pulldown-cmark 0.13 only recognizes the
angle-bracket form `<https://…>` and has no option for GFM's "extended
autolinks", so the renderer now detects them itself.

## How

- `bare_url_ranges` (`parsers/pulldown.rs`) scans one text event using the GFM
  spec rules:
  - **Start:** the start of the text, whitespace, or one of `* _ ~ (`.
  - **Domain:** at least one `.`, and no `_` in the last two labels.
  - **End:** the link runs to whitespace or `<`.
  - **Trimming:** trailing `?!.,:*_~'"`, an unmatched `)` and an `&entity;`
    suffix are not part of the link.
  - **`www.` links** get an `http://` destination.
- `event_text_with_highlights` already split prose by
  `registered_auto_link_ranges` (local `.md` paths) and opened a `Link` for each
  range. The URL ranges join that list. `dedupe_link_ranges` (the old sort and
  overlap filter, pulled out of `registered_auto_link_ranges`) keeps a span from
  being claimed twice. Search highlighting, emoji expansion, click-to-open and
  right-click copy (#169) all come from the existing path, unchanged.
- Links, images, code, inline code and headings are excluded by the existing
  eligibility guards.

## Key discovery: pulldown-cmark splits prose into several text events

pulldown-cmark ends a `Text` event at any character that *could* start inline
syntax: `_`, `~`, `[`, `*` and others. `https://en.wikipedia.org/wiki/Foo_(bar)`
arrives as `…/Foo`, `_`, `(bar) …`, so a per-event scanner links only `…/Foo`.

`latex_delimiters::parse_events` is the single parse used by both `show` and
`show_scrollable`. It now runs `coalesce_adjacent_text`, which merges
consecutive `Text` events when:

- their source ranges touch, and
- each piece is the source text verbatim (`text.len() == range.len()`).

So the merged event still maps byte-for-byte onto the source. A backslash escape
leaves a gap in the ranges, and entity-decoded text differs in length, so
neither is merged. Because the merge happens in the one place both render paths
read from, split-point event indices stay consistent between them.

## Tests

- **Unit, `bare_url_ranges`:**
  - the reporter's sentence
  - trailing punctuation, `&amp;`, balanced and unbalanced parens
  - `www.` destinations
  - rejects (`xhttps://`, a lone `https://`, `www.`, `_` in the domain)
  - UTF-8 neighbours, and overlap with a registered path
- **Render (`tests/wrapping.rs`):**
  - `bare_urls_in_prose_render_as_links` checks that exactly the URL text is
    underlined, without the trailing `.`, and the `Foo_(bar)` link is whole.
  - `bare_urls_are_not_linked_inside_code_or_existing_links`
- **Each guard observed red:**

  | mutation | result |
  |---|---|
  | stub scanner | 5 unit tests red |
  | URL merge removed | render test red |
  | `coalesce_adjacent_text` bypassed | render test red, linked text stops at `…/Foo` |
  | domain validation removed | reject test red |

## Not done

- **Email autolinks** (`user@example.com`), which GFM also links.
- **URLs in headings.** Heading text uses a separate accumulator (see LESSONS,
  "Inline code in headers"). Linking inside it needs a heading-aware link path.
- **URLs split by a backslash escape** (`https://a.com/p\_q`): only the part
  before the escape is linked.
