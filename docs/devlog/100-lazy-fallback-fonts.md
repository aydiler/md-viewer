# Fallback fonts load after the first frame when the document does not need them

**Status:** ✅ Complete
**Branch:** `perf/lazy-fonts`
**Date:** 2026-11-16

## Summary

Every launch installed the full font set before the window drew anything:
the primary sans and its bold, plus one fallback face per script (CJK, Arabic,
Hebrew, Devanagari, Thai), each with a bold face, a symbol face, and the
10 MB color emoji font. Most documents use none of them.

Startup now installs a **base set** first: the primary sans, its bold, the
preset's mono face, and egui's bundled faces including the monochrome emoji
face. It then checks the startup text against that set:
- the active document;
- tab titles and paths;
- the recent-files list.

If every character is covered, the full set is built on a background thread
and installed when it arrives. If not, the full set is installed before the
first frame, which is the old behaviour. The final font set is the same as
before in both cases.

## Measurements

Release builds with a timestamp probe (not committed), on Xvfb. main and this
branch ran alternately, run by run, so machine load (a load average of 16–25 on
16 cores) hits both equally. The numbers are the time spent in
`MarkdownApp::new`, where fonts are set up, in ms:

| case | main: fastest / median | branch: fastest / median |
|---|---|---|
| welcome page | 67 / 109 | 35 / 48 |
| 1 KB English prose | 76 / 112 | 49 / 69 |
| mixed scripts, 1.2 MB (needs fallbacks) | 99 / 126 | 102 / 129 |
| 10 restored devlog tabs (need fallbacks) | 79 / 87 | 84 / 91 |
| explorer at `$HOME` (needs fallbacks) | 59 / 81 | 62 / 81 |

With the page cache evicted first (`posix_fadvise(DONTNEED)`), the font phase
went from 137 to about 55 ms.

The time from `new` to the first content frame dropped by the same amount for
the welcome page (median 172 → 106 ms).

The total from launch is not a fair comparison here. Window and GL context
creation happen before `new`, which this change does not touch, and under load
they varied between 71 and 509 ms for the same binary.

In-process split of the old `setup_fonts`, first call in a fresh process:

| part | ms |
|---|---|
| fontique `Collection::new` | 38 |
| primary sans | 6 |
| script fallbacks (CJK 104 of it) | 121 |
| generic fallbacks (symbols) | 43 |
| color emoji read | 9 |

Later calls in the same process are much cheaper: fontconfig caches its fallback
queries, so CJK took 5 ms instead of 104.

## Key Discoveries

- **The coverage check has to stop at the first miss.** The first version
  collected every distinct non-ASCII character before checking any. On the
  1.2 MB mixed-script document that made the fallback path about 36 ms slower
  than main. Stopping at the first uncovered character brought it level with
  main, within noise.
- **Emoji faces are excluded from the coverage check on purpose.** The full set
  puts the system color emoji ahead of egui's monochrome emoji faces. A
  character only an emoji face covers would therefore change face when the full
  set lands, so documents with emoji take the full path.
- **What the primary face covers depends on the platform, and CI caught both
  assumptions I had made from this machine.**
  - DejaVu Sans (Ubuntu runners) and Segoe UI (Windows) cover Arabic and
    Hebrew. A document in those scripts therefore takes the fast path there.
    That is correct, because the full set would draw them with the same
    primary face. The test now compares against an independent per-face
    lookup instead of hard-coding the result.
  - DejaVu Sans also has a 😀 glyph. In the full set the vendored epaint sends
    emoji-presentation characters to the color face, so the base set would draw
    them differently. `base_covers` therefore treats every emoji-presentation
    character, and U+FE0F, as uncovered, using
    `epaint::text::has_emoji_presentation` (public in stock epaint 0.37 too).
    `emoji_presentation_is_uncovered_even_when_a_base_face_has_the_glyph` pins
    this independently of the machine, by giving the base set a face with emoji
    glyphs. Removing the check turns it red.
- **Two View-menu symbols, ✓ and ☾, come only from the symbol fallback**
  (DejaVu Sans). Neither Noto Sans nor egui's bundled faces have them.
  Including that fallback in the base set costs about 40 ms per launch, so it
  stays deferred. A menu opened in the first ~150 ms after the window appears
  shows them as boxes until the full set lands. The tests pin both halves:
  `chrome_symbols_render_with_the_base_set` (→ — ☀) and
  `view_menu_symbols_come_from_the_full_set` (✓ ☾).
- **The egui MCP bridge does not build against egui 0.37.** Its `Response` type
  comes from an older egui. The find-bar typing check from the plan was
  replaced by a live-reload check: open a Latin document, wait for the full set,
  rewrite the file with CJK, Arabic, Hebrew, Thai and emoji, then screenshot.

## Architecture

- `system_fonts::FontLoader` holds the fontique collection, source cache and
  locale. `definitions(preferred, preset, FontScope::{Base, Full})` builds the
  set with the existing helpers. `install_regular_fonts` takes
  `include_fallbacks`, and `install_emoji_fonts` takes `include_color`.
- `system_fonts::base_covers(defs, texts)` checks text against
  `BASE_TEXT_FACES`.
- `setup_fonts` (runtime family, preset or size changes) still builds the full
  set synchronously.
- `MarkdownApp::new` installs the base set, then `finish_startup_fonts`
  installs the full set now or spawns the `font-fallbacks` thread.
- `update()` polls `pending_full_fonts`. It installs the result only if the
  font config it was built for still matches `last_applied_font_config`. A
  family or preset change in the meantime already installed a full set, so the
  stale result is dropped.
- Info logs name the path taken: "installing the full set now" or "loading
  fallbacks in the background", then "Installing the background-built full
  font set".

## Testing

- `full_scope_matches_the_set_installed_before_the_split`: the same `font_data`
  keys and the same Proportional, Monospace and `MarkdownStrong` chains as the
  old code path, for the default and GitHub presets.
- `base_scope_leaves_out_script_symbol_and_color_emoji_faces`,
  `base_covers_latin_text_and_not_other_scripts`, plus the two chrome tests
  above.
- **Mutations:**
  - `base_covers` always true → the CJK/Arabic/emoji case goes red;
  - the Full scope skipping fallbacks → the equality test and the View-menu
    test go red.
- **Xvfb:**
  - the mixed-scripts document's first screen shows every script, emoji and
    right-aligned RTL paragraphs (synchronous path in the log);
  - the live-reload check above (background path in the log);
  - prose and frontmatter/HTML documents take the background path.

## Future Improvements

- Install only the fallbacks a document needs (for example, just the color
  emoji) synchronously, and the rest in the background. That needs care where a
  text-presentation character is covered by both the color emoji face and the
  symbol face.
- The large-document cost is the renderer's full layout pass in the first
  frame (startup measurement, 2026-11-16), not fonts.
