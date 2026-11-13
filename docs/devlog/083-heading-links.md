# Links in headings render inline

**Status:** Complete
**Branch:** `feature/heading-autolinks`
**Date:** 2026-11-13

## Summary

Devlog 076 left bare URLs in headings unlinked because "heading text uses a
separate accumulator". Checking how an explicit `[text](url)` in a heading
renders showed that path was already broken: the link was drawn in body size,
overlapping the words after it (`See the docs for more` painted "the docs" on
top of "for more").

## Cause

Heading text is collected in `current_heading_rich_texts` and drawn once in
`end_tag(Heading)` inside a single scope at the heading's left edge (LESSONS,
"Inline code in headers"). Link text went to `self.link`, and `TagEnd::Link`
drew it immediately with `Link::end`, at wherever the cursor stood mid-heading
and before the heading's own fragments existed.

## Fix

- `current_heading_links: Vec<(usize, Link)>` records each finished link with
  the number of text fragments before it.
- `finish_link` draws a link right away outside headings and defers it inside
  one. `TagEnd::Link` and the auto-link path both use it.
- `end_tag(Heading)` interleaves links and text in source order inside the
  single heading scope.
- The `heading.is_none()` guard on auto-links is gone, so bare URLs (and emails
  once #228 lands) link in headings too.

Heading identity is unaffected: position keys come from the heading's source
start, and `current_heading_text` is only accumulated, not read.

## Tests

`links_in_headings_render_inline_with_the_heading_text` (`tests/wrapping.rs`):
both links underlined, on the heading's row, ordered between their neighbours,
at the neighbours' height.

| build | result |
|---|---|
| this branch | pass |
| `main`'s renderer | fail: bare URL not linked |
| this branch with the deferral disabled | fail: link not at heading size |

Wrapped-row labels lay their galleys out from the row's left edge, so every
fragment's rect starts at the same x; order shows in the right edges.

Xvfb screenshot before/after: overlapping body-size link → inline heading-size
links for both the explicit link and the bare URL.
