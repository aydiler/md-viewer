# Changelog

All notable changes to markdown-viewer will be documented in this file.

## [0.2.0] - 2026-09-04

The largest release so far: 52 merged pull requests, most of them contributed by [@RichardCao](https://github.com/RichardCao) and [@Pat9496](https://github.com/Pat9496). Viewport-clipped rendering is back on for long documents, tables were rebuilt around content-driven column widths, and font fallback now asks the platform instead of guessing family names.

### Features

- **Local Markdown references open in the viewer (PR #78, contributed by [@RichardCao](https://github.com/RichardCao)).** Links are discovered from CommonMark events rather than a regex, so bare paths, inline-code file references, absolute paths and `file://` URLs all navigate; percent-encoded relative paths such as `docs%20with%20spaces/guide.md` are decoded. Code blocks stay excluded. Regular click navigates in place, Ctrl+click opens a new tab.
- **Selection highlight and link colors are customizable (PR #120, contributed by [@Pat9496](https://github.com/Pat9496)).** A **View → Colors…** dialog sets the highlight color used for tabs, list entries and text selection, plus the link text color. Both persist across restarts, and "Reset to Default" returns to the theme's stock color.
- **YAML frontmatter renders as a key/value table (PR #128).** A leading `---` block was previously shown as a raw fenced blob or dropped entirely. It now renders the way VS Code does, as a flat two-column table. Deliberately not a YAML parser: anything that is not a top-level `key: value` line — nested mappings, sequence items, folded scalars — is appended to the preceding value verbatim, so no source text is lost.
- **Formula size follows typography, not UI zoom (PR #130).** **View → Formula Size** (100 / 110 / 125 / 150 %) scales rendered math independently of Ctrl+±, so formulas can be enlarged to match surrounding prose without magnifying the whole interface. The setting persists, and the math raster cache is keyed by the quantized size so switching back and forth reuses existing rasters.
- **Tables follow the Full Width setting (PR #114, contributed by [@RichardCao](https://github.com/RichardCao)).** Previously prose obeyed the reading-width cap and tables always spanned the pane. Both now respect the setting in both directions, with the visible frame kept inside the bound by deriving its padding and stroke budget from `Frame::total_margin()`. Cached resizable column widths reset when the bound changes, so tables reflow; a stable bound still preserves manual resizing.
- **Side panels show full labels and resize freely (PR #79, contributed by [@RichardCao](https://github.com/RichardCao)).** Explorer and Outline entries scroll horizontally instead of truncating, the upper width limit is gone, and each panel's width is persisted and restored — with invalid stored values (NaN, infinite, undersized) sanitized on load.
- **`--version` prints the version (PR #127).** The flag was declared but never wired to anything.

### Rendering and layout

- **Viewport clipping restored for long documents (#93, PR #96, contributed by [@RichardCao](https://github.com/RichardCao)).** Rendering resumes only after complete top-level blocks, split coordinates are stored relative to the document origin, and the visible slice is placed in an absolute child rectangle following egui's `show_rows`, so skipped height cannot inflate the document extent. The bootstrap `ScrollArea` content size is the canonical document height. Cached geometry is invalidated when image sizes change or async math and Mermaid rendering completes.
- **Viewport slices reproduce the bootstrap's layout (PR #113, contributed by [@RichardCao](https://github.com/RichardCao)).** After #96, a document with a long table stopped scrolling at the end of the table, painted everything below it blank, and shifted the table to the right. Three separate divergences between the measuring pass and the painting pass: the slice recomputed its own content column, was bounded to a fixed-height rect that inflated the reported extent, and never reset line state because it keyed the reset off event 0. Markdown table identity is now derived from document and source position so column state stays stable, and search correction is consumed as a one-frame full-render request.
- **A table reserves its own height, not everything below the cursor (PR #131).** `egui_extras` reserves the height of skipped rows only once it reaches the first *visible* row, so a table lying entirely above the viewport reserved nothing and collapsed to zero height — everything below it laid out too high, and an outline click landed about a third of the way to its target. Each table now reserves its computed height directly.
- **Fitted table columns follow content demand, and header words stay whole (#112, PR #115, contributed by [@RichardCao](https://github.com/RichardCao)).** When columns have to be squeezed into the available width, the old policy found one common cap and applied `min(desired, cap)` to every column — so `[200, 400, 300]` in 600 px all became 200, and a column with twice the content demand of its neighbour gained nothing from it. Allocation now blends max-min fairness with proportional unmet demand, distributing only the width above each column's floor by exact sorted water-filling. The floor is the widest word in that column's header, so a short label like `Required` is no longer split mid-word into `Require` / `d`. Hard minimums, natural-width bounds, compact-table behavior and horizontal overflow are unchanged, and Markdown and HTML tables get the same treatment.
- **Mixed table cell content is measured, not guessed, and cell boundaries stop taking up space (PR #116, contributed by [@RichardCao](https://github.com/RichardCao)).** Row height came from a heuristic — count the chunks a long inline-code token would wrap into, take the maximum — which saw nothing of the text, inline HTML and footnote references beside it. Cells are now laid out for real with egui galleys, accumulating across the wrapped row. Separately, `parse_row` was keeping each `End(TableCell)` marker as the first event of the next cell, and the renderer painted that structural event as a two-space label: roughly 6 px of first-line indent that the height measurement never saw, which near a wrap boundary added a line and clipped the last one. Documents get slightly shorter, because rows are no longer over-allocated.
- **Long table cell text wraps instead of scrolling away (#71, PR #98, contributed by [@RichardCao](https://github.com/RichardCao)).** Columns are sized from their natural content widths, then wide columns are fitted into the available viewport while narrow ones are preserved. Labels wrap at their actual resizable column width and heterogeneous row heights are recomputed from the current widths. Horizontal scrolling remains when minimum widths cannot fit.
- **Images reserve their height in table rows (PR #129).** A row containing an image was sized as if it held only text, so the image overlapped the rows beneath it. Row height now takes the observed image size when it is known and a bounded estimate before the image has loaded — and the observed-size cache marks layout dirty on the *first* write, which it previously skipped because `HashMap::insert` returns `None` for a new key.
- **The outline uses CommonMark heading positions (PR #83, contributed by [@RichardCao](https://github.com/RichardCao)).** Entries are built from pulldown-cmark heading events instead of a line regex, so formatted headings, inline code, links, Setext headings and every code-block form are handled. Positions are keyed by original source byte offset, which keeps duplicate and emoji-shortcode headings distinct without depending on rendered title text.
- **Table formulas keep their absolute bars (PR #75, contributed by [@RichardCao](https://github.com/RichardCao)).** Bare vertical bars inside math pairs were read as table cell delimiters, so `\(\operatorname{EW}[|\Delta OI|]\)` split into extra cells. They are protected before CommonMark table parsing and decoded before reaching the math backend, with source ranges preserved.
- **Tall formulas no longer clip in table cells (PR #76, contributed by [@RichardCao](https://github.com/RichardCao)).** Typst vector frames are expanded before rasterization so glyph ink outside the nominal layout bounds is kept, the measured inline height is cached, and rows reserve the actual formula height. Fixes fractions, integrals and bold symbols being cut off.
- **Font fallback asks the platform (#106, PR #107, contributed by [@RichardCao](https://github.com/RichardCao)).** The hardcoded list of installed family names is replaced by Fontique discovery against CoreText, fontconfig and DirectWrite, using generic and ISO 15924 script fallbacks for the system locale. Glyph coverage is verified per face — including families that split Latin, Thai or Devanagari across separate faces — collection face indices are preserved, and true bold faces are paired with each selected regular fallback. No family names are hardcoded any more.
- **CJK text inside math renders real glyphs (PR #77, contributed by [@RichardCao](https://github.com/RichardCao)).** Fontconfig is asked for a sans-serif face covering Simplified Chinese, added to the Typst font collection as a fallback only, without hardcoding distribution-specific names.

### Robustness

- **A link resolves the same way with and without Ctrl (PR #147).** Plain click and Ctrl+click went through two different resolvers: the Ctrl path percent-decoded and understood `file://` after #78, the plain path still did a raw join. So `docs%20with%20spaces/guide.md` opened in a new tab on Ctrl+click and, on a plain click, looked for a directory literally named `docs%20with%20spaces`, found nothing, and did nothing — no navigation, no error, no feedback. Both paths now share one resolver.

- **A failed read no longer destroys the open document (PR #86, contributed by [@RichardCao](https://github.com/RichardCao)).** Read failures were swallowed and an empty document opened in place of the real one, taking the navigation history with it. Content is applied only after a successful read, history is mutated only after navigation succeeds, and startup, open, reload and navigation errors all surface in the existing error bar.
- **Watcher recovery is bounded for real (PR #85, contributed by [@RichardCao](https://github.com/RichardCao)).** The retry counter reset whenever watcher *construction* succeeded, so repeated disconnects before the first event restarted forever. User-requested starts are now separated from automatic recovery, the limit is enforced, and a bridge thread that fails to start is reported instead of panicking.
- **File dialogs fall back to Tkinter on minimal Linux installs (PR #80, contributed by [@RichardCao](https://github.com/RichardCao)).** Availability of an XDG desktop portal is checked before taking the native path; where neither the portal nor Zenity can serve the desktop, a Python Tkinter dialog handles both file and folder selection, preserving non-UTF-8 path bytes. Configured desktops are unaffected. Dependencies are documented per distribution (PR #105).
- **A saturated math queue retries instead of dropping the formula (PR #103, contributed by [@RichardCao](https://github.com/RichardCao)).** Enqueue failures were silent; they are now reported and a prompt repaint is requested so the job is retried as workers drain.
- **`commonmark_str!` resolves paths from the invoking manifest (PR #102, contributed by [@RichardCao](https://github.com/RichardCao)).** The macro tried rustc's raw working directory first and baked the proc-macro crate's own manifest path into every build, which could select a same-named file from the dependency source tree.

### Performance

- **Explorer scans run off the UI thread (PR #84) and refresh only what changed (PR #87)** — both contributed by [@RichardCao](https://github.com/RichardCao). Local and GVFS root scans no longer block the UI, expansion state is restored when an async result lands, stale results whose root has changed are ignored, and there is a synchronous fallback if thread creation fails. Watcher events rescan only the affected parent directories, leaving unrelated loaded subtrees intact.
- **Math rendering uses one shared worker pool (PR #92, contributed by [@RichardCao](https://github.com/RichardCao)).** One OS thread per formula is replaced by a process-wide pool sized from the existing `math_concurrency()` policy, with queued jobs bounded at 64 and retried on a later repaint. Texture cache keys now include exact foreground and background colors.
- **SVG and data-URL caches are bounded (PRs #88, #89, #100, #101, contributed by [@RichardCao](https://github.com/RichardCao)).** MIME-detected SVG rasters are capped at 64 MiB with LRU eviction, and the trim loop — previously quadratic in cache size because it recomputed the total and rescanned for the LRU entry per eviction — now computes and sorts once. Encoded data URLs above 16 MiB are rejected before parsing, the decoded cache is bounded at 64 MiB including key memory, one-thread-per-URL decoding is replaced by two reusable workers behind a four-job queue, and cache keys and jobs share one `Arc<str>` instead of duplicating a 16 MiB string. SVG options and system-font scanning are deferred until the first MIME SVG is actually rendered.

### Build, packaging and CI

- **MSRV raised to Rust 1.89 (PR #90, contributed by [@RichardCao](https://github.com/RichardCao)).** The enabled Typst 0.14.2 dependency graph requires it, so the previous 1.80/1.76 declarations were not buildable contracts.
- **CI covers the vendored renderer workspace (PR #91, contributed by [@RichardCao](https://github.com/RichardCao)).** Root cargo commands compile the path-patched renderer as a dependency but never ran its own unit, integration, doctest, example or proc-macro tests. A dedicated job now lints and tests all three fork crates with the feature sets md-viewer uses — which immediately exposed stale example and doctest imports left over from the `*_extended` rename.
- **`rustls-webpki` bumped in the vendored lockfile (PR #97)** for GHSA-82j2-j2ch-gfr8.
- **A deep-scroll regression guard (PR #126).** `scripts/scroll-regression.sh` walks a generated table-heavy fixture in 150 steps of 3 wheel clicks and fails on an empty document pane or on scrolling that stops advancing before the bottom. It needs Xvfb, xdotool and ImageMagick, so it does not run in CI — and it found three defects that all three CI jobs passed, including #125.
- **macOS and Windows are checked on every pull request (PR #138, contributed by [@RichardCao](https://github.com/RichardCao)).** All three CI jobs ran on `ubuntu-latest`, so the first Windows compile error surfaced at tag time: v0.1.16 needed three release runs because `Build (windows-x86_64)` failed with ``error[E0433]: cannot find `unix` in `os` ``, fixed afterwards by #70. A `cargo check --locked` on `macos-latest` and `windows-latest` now runs in the PR that would introduce it.
- **The two workspaces are checked for drift (PR #137, contributed by [@RichardCao](https://github.com/RichardCao)).** `scripts/check-workspace-sync.sh` fails when the renderer workspace version, the root dependency, and the `[patch.crates-io]` entries disagree, when either lockfile is stale under `--locked`, or when Cargo reports an unused patch. A fork version bump touches five places by hand; missing one produces a failure that looks like something else entirely. Deliberately a guard for the current two-workspace layout, not a decision on #122.
- **The backend builds without default features again (PR #136, contributed by [@RichardCao](https://github.com/RichardCao)).** Two `math_cache_hash` tests were not gated behind the `math` feature that compiles the function they exercise, so `--no-default-features` failed with `E0425` even though the library supports that configuration. The renderer CI job now exercises it.
- **Compiler warnings cleared (PRs #104, #109, contributed by [@RichardCao](https://github.com/RichardCao)),** including cross-platform ones.
- **The crates.io token is validated before anything is built (PR #135).** `md-viewer` had been stuck at 0.1.15 on crates.io since 2026-07-23, and the fork crates at 0.25.0, because the release token expired: the v0.1.16 and v0.1.17 runs both failed with `403 Forbidden: authentication failed`. It went unnoticed because `publish-crates` runs last — by the time it fails, the snap, both AUR packages and the GitHub Release have already published, so the release looks delivered. A read-only `GET /api/v1/me` in the `validate` job now fails in a second, before anything is built, and names the fix. **The expired token itself still needs replacing.**
- **The release runbook no longer tells you to destroy the changelog (PR #132).** It instructed `git-cliff -o CHANGELOG.md`, which this project's non-conventional commit history turns into a sparse file with whole versions missing.

### Wording

- **The plus button says "Open File in New Tab" (PR #81, contributed by [@RichardCao](https://github.com/RichardCao)),** consistently across the File menu, MCP widget metadata, shortcut tables and README. md-viewer has no empty-tab state, so "New Tab" described something the app cannot do.

## [0.1.17] - 2026-08-22

### Features

- **LaTeX `\(...\)` and `\[...\]` math delimiters now render (#60, PR #73).** Markdown written in LaTeX/Pandoc style previously stayed literal; paired delimiters are converted on an in-memory copy before parsing and every event range is mapped back to the original text, so search highlighting stays exact. Inline code, fenced and indented code blocks, and raw HTML are never converted; escaped (`\\(`) and unmatched delimiters stay literal.

### Bug Fixes

- **GNOME dock icon matches the running window (#62, PR #72).** The window never reported a Wayland `app_id`, so GNOME could not associate it with the pinned `.desktop` entry and showed a second generic icon. The app now sets `app_id: md-viewer`, matching the shipped desktop file's `StartupWMClass`.

## [0.1.16] - 2026-08-22

### Features

- **Tables use the full content pane (#64, PR #67).** In reading mode the whole document is laid out at the prose width (~600 px), and wide tables were clipped at that column with their right side unreachable. Markdown and HTML tables now escape the reading column and span the entire content pane, horizontally scrolling within it when still wider; prose keeps the reading width and small tables continue to hug their columns.
- **LaTeX `\operatorname` / `\operatorname*` render (PR #58, contributed by [@RichardCao](https://github.com/RichardCao)).** amsmath named operators emitted by mitex map to Typst `math.op`, so formulas like `r_{\text{dir}}=\operatorname{sign}(x)` no longer fail.

### Bug Fixes

- **App starts on Ubuntu 26.04 Wayland sessions where the snap cannot reach the compositor (#65, PR #66).** winit commits to exactly one backend from the environment and never falls back, so an unusable `WAYLAND_DISPLAY` aborted startup even with working Xwayland. md-viewer now probes the Wayland socket before creating the event loop and falls back to X11 when it cannot be connected.
- **Linked SVG badges render completely (PR #63, contributed by [@RichardCao](https://github.com/RichardCao)).** Three compounding issues in badge rendering fixed: image-only links no longer reset the wrapped-row cursor (badges overlapped), SVGs served with `image/svg+xml` but without a `.svg` URL extension are decoded via a MIME-aware loader, and the `sans-serif` alias resolves through Fontconfig when fontdb maps it to an uninstalled face.
- **Fonts are discovered from the system instead of hard-coded paths (#59, PR #61, contributed by [@RichardCao](https://github.com/RichardCao)).** A paired regular/bold sans family plus CJK/script fallbacks are selected via fontdb with locale-aware SC/TC/JP/KR priority and glyph-coverage checks; TTC face indices are preserved.

## [0.1.15] - 2026-07-23

### Bug Fixes

- **Snap no longer crashes on X11 sessions (#55, diagnosed and fix verified by [@HartmutLeister](https://github.com/HartmutLeister)).** The strictly-confined snap aborted at startup on X11 (`Library libxkbcommon-x11.so could not be loaded`; Wayland was unaffected). Three gaps compounded, all on the X11-only code path: winit `dlopen`s its X11 stack at runtime so snapcraft's link-time dependency staging never included `libxkbcommon-x11.so.0` and the `libxcb-xkb`/`libX11` chain; XKB keymap data (`/usr/share/X11/xkb`) was absent from both the snap and the core22 base; and Mesa's loader searched its compiled-in absolute DRI path, which resolves to the empty base inside the mount namespace, so GLX context creation failed (`GLXBadFBConfig`) even though the drivers were staged. The snap now stages `libxkbcommon-x11-0`, `libx11-6`, `libx11-data`, and `xkb-data`, and sets `XKB_CONFIG_ROOT` and `LIBGL_DRIVERS_PATH` to the staged copies.

## [0.1.14] - 2026-07-15

### Features

- **GitHub emoji shortcodes (#38, PR #49, contributed by [@aki1ro](https://github.com/aki1ro)).** Recognized gemoji shortcodes in visible Markdown text now render as Unicode emoji (`:pushpin:` → 📌). Expansion happens on `Event::Text` only, after pulldown-cmark has parsed the document, so Markdown syntax, source offsets, search ranges, and heading identity stay authoritative (code, links, image alt-text, and URLs are left literal).

### Bug Fixes

- **`**bold**` now renders with visible weight (#39, PRs #42 + #51).** md-viewer registers a `MarkdownStrong` font family backed by a real bold face and makes **Noto Sans** the primary body face, so bold shares the body baseline instead of falling back to egui's Ubuntu-Light (which had no matching bold).
- **List markers are vertically centred on their item text (PR #50).** The `•` bullet, hollow `◦` nested bullet, and `N.` number sat above the optical centre of the item text on every bullet/ordered list; they now align to the text's line box.
- **Fenced code blocks inside list items no longer overlap adjacent text (#44, PR #48, contributed by [@aki1ro](https://github.com/aki1ro)).** The renderer ends the active wrapped list row immediately before and after each fenced code block; top-level code blocks are unaffected.
- **Inline `code` sits on the shared text baseline at body size (#46, PR #52).** It previously rendered smaller than and raised above the surrounding body text.
- **Narrow tables hug their columns (#47, PR #53).** A table narrower than the content area no longer stretches its bordered frame to full width with an empty gap after the last column; wide-table horizontal scrolling is unchanged.
- **Chinese/Japanese/Korean text renders on Windows (#40, PR #43, contributed by [@aki1ro](https://github.com/aki1ro)).** Added common Windows CJK font files (MicrosoftYaHei, SimSun, DengXian, MicrosoftJhengHei) to the automatic fallback list.
- **Mermaid diagram text with special characters renders correctly (PR #41, contributed by [@MCXCC303](https://github.com/MCXCC303)).** Handles the five double-escaped XML characters produced by the Mermaid→HTML→SVG path.

### Performance

- **Startup no longer hangs ~6 s on a large explorer root.** `start_watching()` registered the explorer root with `notify`'s recursive mode, which synchronously walks the entire subtree on the UI-blocking startup path (~455k inotify watches on `/home/ahmet`). The watcher now covers the root plus each expanded directory non-recursively, mirroring the lazy tree — time-to-window dropped from ~6 s to ~0.11 s and inotify watches from ~455k to ~10.

### Documentation

- Refreshed README notes on recent UI changes (#37).

## [0.1.13] - 2026-06-09

### Packaging

- **`cargo install` (crates.io) now ships the math-rendering fixes.** The vendored `egui_commonmark` fork had been pinned at `0.23.0` while its source changed across releases, so `cargo publish` resolved the fork from the registry and `cargo install md-viewer` built against the *older* fork code. (Source builds — snap, AUR, the GitHub binaries — use the local fork via `[patch.crates-io]` and were always current.) Bumped the fork to `0.24.0` and the root pin to match, so the publish job uploads the new fork version instead of skipping it as "already published." No functional changes versus 0.1.12.
- **Guard against this recurring (#36).** `scripts/check-fork-publishable.sh` runs in the release `validate` job: for each vendored fork crate it diffs the local source against the published crate at the pinned version and fails the release (before any build) if they drift, so a stale crates.io build can't ship silently again.

## [0.1.12] - 2026-06-09

### Features

- **LaTeX math rendering — correct, fast, and baseline-aligned (#35).** Inline `$…$` and display `$$…$$` equations render through typst + mitex. This release overhauls them across three axes:
  - *Correctness:* added `\tfrac`, `\dfrac`, `\boxed`, `\!`, and `\xrightarrow`/`\xleftarrow` to the typst preamble (mitex emits these as calls typst didn't define → red error boxes), and loosened the currency heuristic so real formulas (`$w(z)$`, `$-1.38$`, `$D>0$`, `$8.5$`) render instead of being downgraded to literal `$text`. A 405-formula physics paper now renders 405/405.
  - *Speed:* load only typst's embedded fonts (New Computer Modern) instead of scanning every system font per formula — removing a ~13 s first-formula stall — render formulas in parallel, repaint on completion instead of on a fixed tick, and composite via a 256-entry alpha LUT. A math-heavy document settles in ~3 s instead of ~30 s.
  - *Typography:* inline `$…$` now renders at inline (textstyle) size; display `$$…$$` breaks onto its own centered line even mid-paragraph; short symbols no longer carry doubled horizontal spacing; and each formula's baseline is aligned to the text baseline using egui's actual font metrics (`line_height − font_ascent`) rather than a tuned constant. Devlogs `038`–`040`.
- **Welcome / idle page with recent files (#28, PR #34).** Closing the last tab — or launching with no file — now shows a welcome page with Open File / Open Folder buttons and a recent-files list (deduped, capped, persisted). The old built-in sample document was removed.
- **File → Open Folder… to re-point the file explorer (#28, PR #33).** Repoint the explorer at any directory at runtime without restarting.
- **Keyboard document scrolling (#29, PR #32, contributed by [@aki1ro](https://github.com/aki1ro)).** Up/Down scroll by line and Page Up/Page Down by page, deferred through the renderer-owned scroll pipeline. Arrow keys stay reserved for search-result navigation while the find bar is open, and Ctrl/Alt/Command-modified keys are ignored so existing shortcuts keep priority.
- **Detached terminal launches by default (#30, PR #31, contributed by [@aki1ro](https://github.com/aki1ro)).** `md-viewer file.md` now returns the shell prompt immediately while the window stays open; pass `--foreground` to keep the blocking behavior for logs/scripts.

## [0.1.11] - 2026-05-24

### Bug Fixes

- Wide tables no longer get nudged sideways by ordinary page scrolling (#22, PR #23, contributed by [@aki1ro](https://github.com/aki1ro)). The post-render `forward_wheel_to_horizontal_scroll` helper introduced for #4 redirected any hovered-table `smooth_scroll_delta.y` into the inner `ScrollArea::horizontal` X offset. The intent was that wheel-over-table would scroll the table sideways without users having to grab the bottom scrollbar; the cost was that whenever the cursor merely crossed a wide table during normal document scrolling, the table jumped left/right. Edge pass-through reduced but did not remove the surprise. Fix: remove the helper and both call sites entirely; plain wheel input stays with the outer document scroller.

### Features

- Shift+wheel over a wide table now scrolls it horizontally (PR #24). Restores the #4 ergonomic that PR #23 had to drop, but gated on the Shift modifier so it can't collide with normal page scrolling. New helper `forward_shift_wheel_to_horizontal_scroll` in `crates/egui_commonmark/.../pulldown.rs` mirrors the prior helper's edge-passthrough logic and only acts when `ui.ctx().input(|i| i.modifiers.shift)` is true. Shift+wheel for horizontal scroll matches the browser convention; Ctrl was already taken by zoom. Documented in `docs/devlog/033-table-shift-wheel.md`.

## [0.1.10] - 2026-05-24

### Bug Fixes

- Search-active scroll lock (#19, PR #21). With the find bar open, wheel-scrolling away from the active match snapped the view back every frame, leaving the user "locked" near the result; Esc to dismiss the bar was the only workaround. Root cause: the post-render corrective scroll in `render_tab_content` was designed as stage 2 of a two-stage scroll (line-ratio estimate → snap to renderer-recorded `active_search_y`). After the disable-virtualization change in this release, the renderer started walking the full event stream every paint, and `record_active_search_y_viewport` fires unconditionally per Active highlight segment (egui's clip rect culls painting but not widget layout). So `active_search_y` became perpetually fresh, and the corrective block — which had no guard for "user just scrolled" — re-fired every frame the match was off-screen. Fix: one-shot `correct_active_search_pending: bool` on `Tab`, set by `scroll_to_active_match` and cleared after the corrective block runs once. Two-stage scroll still converges in 1–2 frames; subsequent wheel input is no longer overridden. Detail in `docs/devlog/031-search-scroll-lock.md`.
- Nested-list rendering crash + scroll lag (4b13e25). Long docs with deeply nested lists could panic in `delayed_events_list_item` because that helper stopped at the first `TagEnd::Item` regardless of nesting depth, leaking inner-list events into the outer `show()` loop where they were registered as split-points. Fix: depth-track nested items/lists, return only when the outer item closes. Also addressed a math-feature parser-options mismatch between `show()` and `show_scrollable()` that produced inconsistent event streams whenever the doc contained `$…$` (currency, env vars, regex).
- Outline-click scroll precision regression (#1, #2; 6eb8001). Click-on-far-heading landed each header progressively further below the viewport top on layout-changed builds. Root cause: `record_header_content_y_if_absent` pinned the first paint's value, which was computed before async font fallbacks settled. Fix: drop the `_if_absent` semantic so every paint refreshes the recorded y.
- Scroll jitter on slow CPUs (9f02fdc). On T470-class machines, scrolling image-heavy 3 800-line docs felt extremely janky for ~30 s before settling. Root cause: `compute_layout_signature` hashed raw `f32.to_bits()` of widths and font heights; sub-pixel jitter from async image/font loading flipped the hash every frame and forced ~32 full re-bootstraps. Fix: quantize the float inputs (pixel for width, 0.1 px for font heights). Bootstraps during a 30 s scroll: 32 → 1.
- Async-load content shift staleness (d356cc9, 809b761). When images finished loading mid-session, stored split-point y-coords went stale and viewport-skip painted wrong content. First attempt (bucket the previous content_h into layout_signature) entered a perpetual bootstrap loop because the two paint paths report content_h differing by ~44 px. Final fix: absolute-drift hysteresis with a 1 024 px threshold — only invalidate when content height shifts by more than the egui-internal oscillation amplitude.
- Deep-scroll rendering regression revert (5fb3b71). The content-y conversion attempted in 4b13e25 fixed outline-click precision but broke deep-scroll rendering in `full_width_content=true` mode on docs with mixed tables/code blocks. Reverted to screen-y storage; outline-click instead uses the `pending_scroll_offset` non-clear pattern (see entry above).

### Performance / Stability

- Disabled `show_scrollable` virtualization in favor of always-bootstrap (21d43c5). The skip-paint virtualization had three independent unfixable-in-band bugs (orphan `Start` events, `content_size.y` inflation, container-state mid-slice). Symptoms were flicker, blank patches, wrong code-block indentation during scroll. Trade-off: docs ≤ ~3 k events stay smooth, ≤ ~10 k borderline, ≥ ~20 k laggy. Acceptable for typical personal use. A re-virtualization design is tracked in `docs/devlog/030-skip-paint-investigation.md`.

### Internal / CI

- Release pipeline hardened (044d872). `validate` job runs upfront so syntax/typo errors fail fast instead of after the long matrix build. Step-level secret gating for the optional `publish-aur` / `publish-aur-bin` / `publish-snap` / `publish-crates` jobs (GitHub Actions blocks `secrets.*` in job-level `if:`, so the pattern is a first step that writes `proceed=true|false` to `$GITHUB_OUTPUT` and every subsequent step gates on it). MCP-strip transform anchors at start-of-line so it doesn't rewrite commented lines and trigger `cargo publish`'s dirty-tree check (9b59101 adds `--allow-dirty` as belt-and-suspenders for local-MCP testers who forget to re-comment).

## [0.1.9] - 2026-05-16

### Internal / CI

- Restored crates.io auto-publish that was removed in PR #11. Fork crates publish under `_extended` renamed identifiers (no upstream conflict) with feature parity vs the registry; publish order is backend → macros → extended → md-viewer with a 45 s sparse-index settle delay between hops. "Already uploaded" treated as success → idempotent on re-tags.

## [0.1.8] - 2026-05-16

### Packaging

- New `md-viewer-bin` AUR package ships the prebuilt linux-x86_64 binary from GitHub Releases instead of compiling from source. `yay -S md-viewer-bin` is a ~5 s install (vs ~2-3 min compile via `md-viewer-git`), no Rust toolchain required. The two packages `conflict` with each other; pacman picks one. PKGBUILD pulls the `.desktop`, icon, and `LICENSE` from raw GitHub URLs pinned to the tagged commit since the release tarball is binary-only. CI: new `publish-aur-bin` job in `release.yml` mirrors `publish-aur` but rewrites both `pkgver=` *and* the four-element `sha256sums=( ... )` array on every tag. Same `AUR_SSH_PRIVATE_KEY` secret powers both publish jobs.

## [0.1.7] - 2026-05-16

### Bug Fixes

- Outline-click on duplicate-titled headers (#17). Two `## Installation` sections used to both resolve to the same y because `CommonMarkCache::header_positions` is keyed by lowercased title; the second occurrence's `insert()` clobbered the first. Fix: composite key `(normalized_title, nth_with_same_text)` rendered as `"installation"` for the 0th occurrence and `"installation#N"` for the Nth duplicate. Parser assigns the index, renderer mirrors it under the same scheme. Includes a corrective two-stage scroll (`pending_header_click_key`) modeled on the existing search-jump corrective so the bootstrap full paint's precise y wins over the line-ratio first-frame estimate.
- Bootstrap branch in `show_scrollable` corrupted recorded positions when triggered by a non-zero `pending_scroll_offset` (search-jump, outline-click landing deep in doc). Root cause: `cache.set_scroll_offset(0.0)` was unconditional, but the inner `.show()` runs inside a ScrollArea that has already been scrolled to the pending offset, so `ui.cursor().top()` is viewport-relative. Every `record_header_position` / `record_active_search_y_viewport` got shifted by the negative scroll offset, then the corrective scroll snapped to those wrong values. Fix: pass `pending_scroll_offset.unwrap_or(0.0)` instead. This is the missing piece that makes the duplicate-headers disambiguation work end-to-end.

### Features

- Click-to-enlarge lightbox now works for regular markdown images too, not just mermaid diagrams (#17). `![alt](url)` images get `Sense::click()` + a `cache.clicked_image` slot that the main app consumes alongside `take_clicked_mermaid` to open the existing lightbox overlay. Pointer cursor on hover, X close button, escape closes.

## [0.1.6] - 2026-05-16

### Features

- Full-width content toggle (#16, contributed by [@aki1ro](https://github.com/aki1ro)). New `View → Full Width` menu item flips between the default 600 px reading-cap (optimal line length per Dyson & Haselgrove 2001) and using the full available content pane. Persisted to `~/.local/share/md-viewer/app.ron` as `full_width_content: bool` so the choice survives restarts. Default remains capped.

### Bug Fixes

- Wide table horizontal scroll now responds to mouse wheel over the table body (#15, closes the second half of #4). egui 0.33's `ScrollArea::horizontal()` only consumes the X delta of `smooth_scroll_delta` and plain wheel only emits Y, so without intervention the page scrolled instead of the table — users had to drag the bottom scrollbar. The vendored fork now calls a post-render `forward_wheel_to_horizontal_scroll` that redirects Y delta into the inner area's X offset when the cursor is hovered, with edge pass-through (at left/right boundary the delta falls back through to the outer vertical area so the page can still scroll past the table).

### Documentation

- README + all 7 screenshots refreshed for v0.1.5 visuals (new `screenshots/search.png` and `screenshots/resizable-tables.png`, plus refreshed `dark-mode.png` / `light-mode.png` / `syntax-highlighting*.png` / `tables.png`). Features section now mentions search (Ctrl+F), resizable table columns, and viewport virtualization; new Search keyboard-shortcuts table.
- New `docs/devlog/022-table-wheel-scroll.md` and `docs/devlog/023-full-width-toggle.md`.

### Internal

- All View menu items now register with the MCP bridge under `Menu: View → …` names with state-value tags (`"on"`/`"off"`, `"dark"`/`"light"`). The View menu button itself is registered as `Menu: View`. This closes the previously-documented "menus aren't in AccessKit" coverage gap — future E2E tests can drive theme/sidebar/zoom/full-width toggles through `egui_click` instead of state-file injection.

## [0.1.5] - 2026-05-16

### Features

- Resizable table columns (closes the column-width side of issue #4). Both markdown `|...|` tables and HTML `<table>` blocks now render via `egui_extras::TableBuilder` instead of `egui::Grid`. Drag the divider between any two columns to resize; cells re-wrap their content to fit. Striping and the outer border are preserved. Wide tables exceeding the panel width get a horizontal scrollbar instead of clipping. Long inline-code paths inside cells wrap to multiple visual lines per row (per-row height auto-computed). See `docs/devlog/021-table-columns-resizable.md` for the verification matrix and the known edge case (tables with many narrow columns at ≤800 px windows can drop right-side columns).

### Performance

- End-to-end virtualization of the markdown renderer. Scroll frame time at 100 k lines drops from ~101 ms to below the 1-tick measurement floor (effectively 60+ FPS); first-paint settle on a 100 k-line / 6 MB doc drops from ~15 s to ~7 s. Achieved via the vendored `egui_commonmark` fork: dense `split_points` at every block-level event end (root cause of the upstream "buggy in scenarios more complex than the example" warning on `show_scrollable`), binary-search viewport range over split_points, parsed-events cache keyed by a per-`Tab` `content_version`, `layout_signature` invalidation that includes zoom and theme (not just width). The app's `render_tab_content` switches to the renderer-owned `ScrollArea` via the new `CommonMarkViewer::show_scrollable` builder that returns `ScrollAreaOutput<()>` so the selection-preserving wheel hack still works.
- Lazy syntect highlighting. `CodeBlock::end` now hits a `(content, lang, theme, font_size)`-keyed `LayoutJob` cache before running syntect, so only visible code blocks pay the highlight cost on first paint and re-highlight is a hash-lookup after that.
- Outline panel virtualized via `egui::ScrollArea::show_rows`. On a 100 k-line doc with ~15 k headers the outline cost drops from O(headers) to O(visible_rows).

### Bug Fixes

- Search-jump and outline-click on off-viewport targets no longer leave the viewport at the line-ratio estimate. When `pending_scroll_offset` is set, the renderer forces a one-frame full paint so `cache.active_search_y` / `header_position` get recorded; the two-stage corrective scroll then snaps precisely. Cost: one ~100 ms frame per jump action (steady-state scroll is unaffected).

### Documentation

- New `docs/devlog/020-virtualize-large-docs.md` with the implementation walk, perf measurements, and the full MCP test pass (T-A through T-J: outline click, wheel scroll, search, zoom, theme, multi-tab isolation, file-explorer click, live reload, outline fold, selection-during-scroll).
- New `docs/devlog/021-table-columns-resizable.md` for the TableBuilder refactor and verification matrix.
- New `docs/LESSONS.md` entries covering virtualization gotchas (sparse split_points, layout_signature scope, selection-preserving wheel hack needs `ScrollAreaOutput`, lazy-syntect cache key) and TableBuilder gotchas (fixed row heights clip multi-line cells, outer `ScrollArea::horizontal` required, header/body Y alignment needs `ui.vertical()`).

## [0.1.4] - 2026-05-15

### Features

- Search (Ctrl+F) with inline highlights and precise scroll-to-match (#14, closes #4). Find bar above the tab bar; case-insensitive matches in the active tab get an inline yellow highlight, the active match gets a brighter orange. Enter / Shift+Enter / ↑ / ↓ cycle matches; Esc closes the bar. Matches inside image alt-text and image/link URLs are skipped so cycling only lands on visibly-rendered text. Two-stage scroll lands the active match in viewport even in image-heavy documents.

### Bug Fixes

- Wide inline-code tokens (long file paths, fully-qualified identifiers) overflowed the content column at narrow widths and overlapped adjacent text at wide widths. Long tokens are now split into fixed-size chunks separated by row breaks (#5).

### Documentation

- Document snap `--destructive-mode` glibc trap (Ubuntu 22.04 compatibility), inline-code wrap segmentation choice, and the open feature-request priority order in LESSONS.md and TARGET_METRICS.md.

### Miscellaneous

- Replace placeholder app icon with a generated document icon.
- Tighten Flatpak `finish-args` for Flathub linter; prep Flatpak manifest for Flathub submission.

## [0.1.3] - 2026-05-15

### Bug Fixes

- Resolve clippy warnings for CI
- Prevent dollar amounts from rendering as math formulas

### Features

- Add LaTeX math rendering via typst + mitex

### Performance

- Eliminate per-frame allocations, syscalls, and re-parsing

### Styling

- Apply rustfmt formatting
## [0.1.2] - 2026-03-04

### Bug Fixes

- File watcher recovery when watcher fails to start
- Properly apply underline and color to markdown links
- Bring link underline closer to text by removing extra line height
- Enable HTTP image loading and add DejaVu Sans font fallback
- Directory expansion now works with single click
- Canonicalize tab paths for consistent file watcher matching
- Re-check expansion state after click for immediate child rendering
- Toggle directory expansion after row render for immediate children
- Load children for expanded directories on session restore
- Use floor_char_boundary for outline header truncation
- Resolve relative image paths against markdown file directory
- Improve HTML table readability with vertical separators and cell padding
- Use unique IDs for code block horizontal ScrollAreas
- Truncate long file paths in menu bar to prevent overlap with buttons
- Re-enable content zoom (disabled during MCP testing)

### Documentation

- Update README screenshots with nav arrows
- Update README with latest features and refresh screenshots
- Document SVG text rendering requirements and limitations
- Update README with new features and refresh screenshots

### Features

- Add context menu to copy file contents from explorer
- Increase link visibility with underline and hyperlink color
- Add system font fallbacks for Unicode support
- Add navigation buttons and virtual display CPU fix
- Lazy load file explorer directories on expand
- Enable SVG text rendering for shields.io badges
- Enable file watching by default
- Add middle-click to close tabs from file explorer
- Add file explorer sorting options
- Render HTML tables as grids instead of raw text
- Add mermaid diagram rendering support
- Switch mermaid renderer from mermaid-rs-renderer to merman
- Add mermaid diagram click-to-enlarge lightbox
- Async mermaid rendering + lightbox zoom-to-cursor

### Miscellaneous

- Add snap artifacts to gitignore, update deps
- Upgrade merman from 0.1 to 0.3
- Bump version to 0.1.2

### Performance

- Fix file explorer O(n×m) scan and lazy-load session restore
- Eliminate idle CPU usage with event-driven file watcher repaints
- Disable egui memory persistence and clear stale data on startup

### Styling

- Apply rustfmt to font fallback tuples
## [0.1.1] - 2026-01-30

### Bug Fixes

- **ci:** Correct rust-toolchain action name
- **ci:** Correct Ubuntu package names for libxcb
- **ci:** Remove local-only MCP dependency before build
- **ci:** Fix clippy warnings and MCP feature handling
- **ci:** Fix sed order to preserve mcp feature
- **ci:** Use cargo test instead of cargo test --lib
- **release:** Use rust plugin for snap, allow-dirty for crates.io

### Miscellaneous

- Release v0.1.1

### Styling

- Apply cargo fmt

### Ci

- Add GitHub Actions for CI and releases

