# Perf: parse a document once

**Status:** ✅ Complete
**Branch:** `perf/parse-once`
**Date:** 2026-11-21
**Lines Changed:** renderer `parsers/pulldown.rs` (`parsed_events`, `show_measuring`, `show_scrollable`, `measure_from_frontier`, a test), backend `pulldown.rs` (`ParsedEvents`, `ScrollableCache::events`), backend `misc.rs` (`cached_events` and accessors); `src/main.rs` (`parse_document`, `HeaderCollector`, `LinkCollector`, `is_local_markdown_link`, tests)

## Summary

Opening a document parsed its full text four times:

| # | where | for |
|---|---|---|
| 1 | `parse_headers` (app) | outline and title |
| 2 | `parse_local_links` (app) | link hooks |
| 3 | renderer `show()` | `CommonMarkCache::cached_events` |
| 4 | renderer `show_scrollable` | `ScrollableCache::events`, a second copy kept identical to #3 by hand (devlog 027) |

Now the renderer parses once and both caches hold the same allocation. The app
parses once for its outline and links together, and the link scan no longer
classifies every word of plain text.

## Step 1: measurement before the change

A temporary probe timed each site (`MDV_DIAG_PARSE`, never committed), plus the
full-text hash in `show()`. Release builds on Xvfb, 5 runs per document,
median, time to first content (TTFC) from the startup probe of devlog 100.

| document | TTFC | #1 | #2 | #3 | #4 | hash |
|---|---|---|---|---|---|---|
| README | 192 ms | 0.1 | 0.3 | 0.2 | 5.3 | 0.0 |
| code-heavy 2.7 MB | 235 ms | 23.6 | 2.0 | 6.3 | 6.6 | 0.5 |
| tables 1 MB | 468 ms | 8.3 | 22.9 | 169.1 | 97.7 | 0.2 |
| prose 5 MB | 307 ms | 6.7 | 93.1 | 21.6 | 29.7 | 1.0 |
| GitLab changelog 6 MB | 838 ms | 107.6 | 90.4 | 173.0 | 268.7 | 1.2 |

Decisions, by the plan's rule (act on ≥ 5 % of TTFC):

- **Renderer double parse:** acted on. #4 is up to a third of TTFC.
- **Full-text hash:** dropped. It runs once per load, not per frame (progressive
  measurement calls `show()` once), and costs ~1 ms.
- **Link pass:** acted on, but the measurement changed the fix. On 5 MB of prose
  the link pass took 93 ms while the outline pass over the same parse took 7 ms.
  The cost was not the parse but `classify_destination`, run on every
  whitespace-separated word to see whether it names a Markdown file.

Order matters in these numbers. Whichever pass runs first on a fresh document
pays for first-touching its memory: #4 runs before #3, and #1 before #2. Read
the columns as upper bounds, not exact per-site costs.

## Architecture

### Renderer

- `ParsedEvents = Arc<Vec<(Event<'static>, Range<usize>)>>` (backend
  `pulldown.rs`). It is the type of `ScrollableCache::events` and of the stream
  in `CommonMarkCache::cached_events`.
- `CommonMarkViewerInternal::parsed_events(cache, options, text)` returns the
  cached stream when the text hash matches. Otherwise it parses, stores the
  stream, and stores the right-to-left block starts from the same parse.
  - `show_measuring` calls it.
  - `show_scrollable` calls it when the content version changes, and assigns
    the result to `sc.events`.
  - The devlog 027 invariant (split points index the same stream the slice
    consumes) now holds by construction: it is one allocation.
- `show_measuring` iterates the `Arc` directly. The take/put-back of the vector
  is gone, and so is `mem::take` in `measure_from_frontier`.
- `CommonMarkCache` keeps its public API:
  - `get_cached_events` and `set_cached_events` (which wraps in an `Arc`);
  - `take_cached_events` (which unwraps or clones);
  - new: `cached_events_shared` / `set_cached_events_shared`.
- `ScrollableCache::events` is a public field whose type changed. The next
  release therefore bumps the renderer's minor version.

### App

- `HeaderCollector` and `LinkCollector` hold the per-event logic of the old
  `parse_headers` and `parse_local_links`.
- `parse_document(content, path)` drives one `Parser::new_ext(.., Options::all())`
  through both. `Tab::new` and the reload path use it.
- The two old functions are kept under `#[cfg(test)]` as single-pass references.
- `is_local_markdown_link` returns false for a destination without a `.` before
  classifying it.
  - `has_markdown_extension` tests the source spelling (`raw`), which has no
    extension without a literal dot, so the shortcut never changes an answer.
  - A case-insensitive `.md` substring test would not be safe: a non-ASCII
    letter can lowercase into ASCII (U+212A KELVIN SIGN → `k`, as in
    `.mar\u{212A}down`).
  - Most prose words lose any trailing dot to the punctuation trim in
    `register_bare_markdown_paths`, so they skip classification.

## Tests

- `scrollable_and_bootstrap_share_one_parsed_stream` (renderer, `--lib`).
  - After `show_scrollable`, `cached_events_shared(hash)` and
    `ScrollableCache::events` are `Arc::ptr_eq`.
  - After a content change, both hold a new stream containing the new text.
  - Mutant: `show_scrollable` parsing on its own → red, "the scroll cache holds
    a second parse of the document".
- `the_dot_shortcut_never_rejects_a_markdown_destination`.
  - Corpus: a set of tricky destinations (`file://`, `%2Emd`, query and
    fragment, Kelvin sign, Windows path) plus every whitespace-separated word of
    the README, CHANGELOG and LESSONS.
  - It checks that every classified-Markdown destination contains a dot, and
    that the shortcut gives the same answer as classification.
  - Mutant: shortcut narrowed to `contains(".md")` → red on `.markdown`.
- `one_document_parse_matches_the_separate_outline_and_link_passes`.
  - On a fixture (duplicate headings, inline code, math and footnote in a
    heading, code block, explicit, bare and anchor links) and on the README,
    `parse_document` equals the two reference passes.
  - Mutant: `parse_document` skipping the link collector → red.
- Suites:
  - renderer with CI features: 123 lib tests plus all integration targets;
  - backend with all features; renderer with defaults;
  - root: 98 passed, 10 ignored;
  - clippy: no new warnings; fmt: clean.
- Xvfb guards on the release build, run one after the other:
  - `scroll-regression.sh` PASS (bottom reached at frame 63 of 150);
  - `visual-regression.sh` PASS (no drift).

## Measurement after

The startup probe only, release builds, main (efdd940) and the branch launched
alternately, 9 runs each, median TTFC.

| document | first run: main → branch | second run (busier machine): main → branch | peak RSS, main → branch |
|---|---|---|---|
| README | 145 → 144 ms | 279 → 281 ms | 206 → 207 MB |
| code-heavy 2.7 MB | 165 → 159 ms | 303 → 274 ms | 209 → 201 MB |
| tables 1 MB | 208 → 180 ms | 417 → 389 ms | 244 → 232 MB |
| prose 5 MB | 276 → 210 ms | 484 → 367 ms | 216 → 214 MB |
| GitLab changelog 6 MB | 624 → 421 ms | 1018 → 719 ms | 338 → 304 MB |

- The first run is before the app-side merge (renderer change and dot shortcut
  only); the second is the final branch.
- On the changelog the gain is 29–33 % and 34 MB of peak memory, the second
  copy of the event stream.
- In the first run's probe, the prose link pass fell from 89 to 38 ms. The
  bootstrap's `parsed_events` call became a cache hit (0–1.5 ms).

## Future Improvements

- The remaining 38 ms of the prose link pass is `split_whitespace` plus the
  punctuation trim over 5 MB. Scanning only text events that contain a `.` at
  all would skip most of it.
- The app parse (`Options::all()`) and the renderer parse (math, LaTeX
  delimiter rewrite, frontmatter) still differ, so they cannot share a stream.
  Making the outline read the renderer's events would save the last duplicate
  parse, but it changes which headings the outline sees in math-heavy
  documents.
