# Lessons Learned

Reusable fixes and non-obvious solutions discovered during development. Check here before debugging similar issues.

---

## egui_commonmark

### Link hooks reset automatically
**Context:** Implementing link navigation
**Gotcha:** Hooks registered with `cache.add_link_hook()` reset to `false` before each `show()` call automatically. No need to manually reset them.
**Files:** `src/main.rs`

### Anchor-only links cause browser errors
**Context:** Links like `#section` passed to browser open `file:///path/#section` and fail
**Fix:** Register hooks for anchor-only links too, but ignore them in navigation handler
```rust
// Register ALL local links including anchors
cache.add_link_hook("#section");
// In navigate_to_link(), skip if path starts with #
```
**Files:** `src/main.rs`

### Line height not exposed by default
**Context:** Wanted to set 1.5x line height for readability
**Problem:** egui_commonmark doesn't expose `line_height` configuration despite egui's `TextFormat.line_height` existing
**Fix:** Fork egui_commonmark and wire up the API
**Files:** `crates/egui_commonmark/`

### TextFormat::simple() ignores line height
**Context:** Setting line height on code blocks
**Problem:** `TextFormat::simple(font_id, color)` doesn't support line height
**Fix:** Create format manually:
```rust
let mut format = egui::TextFormat::simple(font_id, color);
format.line_height = Some(line_height);  // Must set explicitly
```
**Files:** `crates/egui_commonmark/egui_commonmark_backend/src/misc.rs`

### default_width overflows at narrow widths
**Context:** Text was cut off instead of wrapping when window narrowed below 600px
**Problem:** `CommonMarkOptions::max_width()` returned `default_width` even when larger than `available_width`
**Fix:** Cap the width at `available_width`:
```rust
// Before (buggy):
if default_width as f32 > max_width { default_width as f32 }

// After (fixed):
(default_width as f32).min(available_width)
```
**Files:** `crates/egui_commonmark/egui_commonmark_backend/src/misc.rs`

### Line ratio scroll calculation is inaccurate
**Context:** Clicking outline headers scrolled to wrong position
**Problem:** Using `line_number / content_lines * content_height` assumes linear relationship between line count and rendered height, but markdown has variable heights (headers, code blocks, spacing).
**Fix:** Track actual header positions during rendering by adding position tracking to `CommonMarkCache`:
```rust
// In CommonMarkCache - track positions
cache.set_scroll_offset(viewport.min.y);  // Before render
cache.record_header_position(title, y);   // During render
let pos = cache.get_header_position(title); // For scroll
```
**Key insight:** `ui.cursor().top()` inside `show_viewport` is viewport-relative, not content-relative. Add `viewport.min.y` to convert.
**Files:** `crates/egui_commonmark/`, `src/main.rs`

### SVG text requires svg_text feature
**Context:** Shields.io badges rendered as colored rectangles without text
**Problem:** `egui_extras/svg` feature only enables basic SVG rendering. Text in SVGs requires system fonts to be loaded by resvg.
**Fix:** Enable `egui_extras/svg_text` feature which calls `fontdb.load_system_fonts()`:
```toml
# In egui_commonmark Cargo.toml
svg_text = ["egui_extras/svg_text"]

# In app Cargo.toml
egui_commonmark_extended = { features = ["svg", "svg_text", ...] }
```
**Requires:** System fonts like Verdana, Arial (install `ttf-ms-fonts` on Arch)
**Files:** `Cargo.toml`, `crates/egui_commonmark/egui_commonmark/Cargo.toml`

### SVG badges with embedded logos don't render
**Context:** GitHub stars badge shows broken icon, but License MIT badge works
**Problem:** resvg doesn't fully support `<image>` elements with embedded SVG data URIs (nested SVGs)
**Example of failing badge:**
```xml
<!-- GitHub stars badge contains embedded logo -->
<image href="data:image/svg+xml;base64,..." />
```
**What works:** Simple text-only badges (no logos)
**What fails:** Badges with `?logo=` parameter or embedded images
**Workaround:** Use badges without logos, or accept the limitation
```markdown
<!-- Fails (has logo) -->
![](https://img.shields.io/badge/snap-app-blue?logo=snapcraft)

<!-- Works (no logo) -->
![](https://img.shields.io/badge/License-MIT-blue.svg)
```
**Status:** Upstream limitation in resvg - no fix available
**Files:** N/A (external limitation)

### Strong markdown needs a registered bold family for visible weight
**Context:** Issue #39 reported that `**bold**` rendered with no visible bold difference.
**Problem:** The renderer already called `RichText::strong()`, but egui can still lay the text out with the same regular font face. For md-viewer, visible bold requires selecting a distinct font family backed by a bold face.
**Fix:** Keep `RichText::strong()` for the semantic hint, add an opt-in `use_strong_font_family` option, register `STRONG_FONT_FAMILY` (`MarkdownStrong`) in `setup_fonts`, and enable the option only in md-viewer's viewer builder:
```rust
CommonMarkViewer::new()
    .use_strong_font_family(true)
    .show_scrollable(tab.id, ui, &mut tab.cache, &tab.content);
```
**Gotchas:** Default must stay `false` for generic library consumers that did not register `MarkdownStrong`. Strong inline code must skip the strong family override so `**`code`**` keeps the monospace code font.
**Files:** `src/main.rs`, `crates/egui_commonmark/egui_commonmark/src/lib.rs`, `crates/egui_commonmark/egui_commonmark_backend/src/misc.rs`

---

### Emoji shortcode expansion must preserve raw source identity
**Context:** Issue #38 — render recognized GitHub shortcodes such as `:pushpin:` as emoji glyphs.
**Problem:** Replacing shortcodes in the whole Markdown string changes byte offsets and can mutate code, links, image syntax, and heading identity before pulldown-cmark parses them. Replacing text without retaining raw spelling also breaks search ranges and duplicate-heading keys because `:pushpin:` is 9 source bytes while `📌` is 4 UTF-8 bytes.
**Fix:** Expand only eligible `Event::Text` segments inside the renderer. Each scanner segment carries rendered text, raw spelling, and its absolute original source range. Advance the cursor by raw bytes. Keep replacement segments indivisible for painting: any search-range overlap highlights the whole glyph, with Active taking precedence. Feed the rendered glyph to `current_heading_rich_texts`, but feed the raw shortcode to `current_heading_text`. Skip image alt text and code blocks; route `Event::Code` through a separate literal-only highlight path so inline code keeps both source text and search highlights. Destinations, URLs, and malformed/unknown candidates remain literal.
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`, `src/main.rs`, `docs/devlog/045-emoji-shortcodes.md`

---

## egui

### CommonMarkCache must persist across frames
**Context:** Markdown rendering was flickering/broken
**Problem:** Recreating `CommonMarkCache` every frame breaks rendering state
**Fix:** Store cache in app struct, only reset on file load (not per-frame)
```rust
struct MarkdownApp {
    cache: CommonMarkCache,  // Persist this!
}
```
**Files:** `src/main.rs`

### Typography multipliers are context-dependent
**Context:** Line height of 1.5 produced different pixel values for body vs code
**Explanation:** Multipliers resolve against the font size of that context:
- Body text (16px): 1.5x = 24px
- Code blocks (14px): 1.3x = 18.2px

**Files:** `crates/egui_commonmark/egui_commonmark_backend/src/typography.rs`

### Font fallback for Unicode support
**Context:** Red triangles appearing for emojis, CJK, and non-Latin scripts
**Problem:** egui's default fonts don't include most Unicode characters
**Fix:** Load system fonts (Noto family) as fallbacks at startup:
```rust
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // Load font file
    let font_data = fs::read("/usr/share/fonts/noto/NotoSans-Regular.ttf")?;
    fonts.font_data.insert(
        "NotoSans".to_string(),
        egui::FontData::from_owned(font_data).into(), // .into() for Arc
    );
    // Add as fallback (after default fonts)
    fonts.families.get_mut(&egui::FontFamily::Proportional)
        .unwrap().push("NotoSans".to_string());
    ctx.set_fonts(fonts);
}
```
**Files:** `src/main.rs`

### Windows CJK fallback needs Windows font paths
**Context:** Issue #40 reported Chinese text missing on Windows 11 even though Linux Noto CJK fallback paths existed.
**Problem:** `setup_fonts()` only loads files from paths listed in `SYSTEM_FONT_PATHS`. Linux Noto paths do nothing on Windows, so Windows builds can still miss CJK glyphs.
**Fix:** Add common Windows CJK font files to the same fallback list, keeping the existing loader and no manual config:
```rust
("MicrosoftYaHei", "C:/Windows/Fonts/msyh.ttc"),
("SimSun", "C:/Windows/Fonts/simsun.ttc"),
("DengXian", "C:/Windows/Fonts/Deng.ttf"),
("MicrosoftJhengHei", "C:/Windows/Fonts/msjh.ttc"),
```
**Scope:** This is automatic fallback only. User-configurable font paths are a separate feature because they need UI/persistence/design work.
**Files:** `src/main.rs`

### egui 0.33 FontData requires Arc wrapper
**Context:** Compiler error when adding fonts
**Problem:** `fonts.font_data.insert()` expects `Arc<FontData>`, not `FontData`
**Fix:** Add `.into()` to convert:
```rust
egui::FontData::from_owned(font_data).into()
```
**Files:** `src/main.rs`

### Color emojis not supported in egui
**Context:** Emojis render as monochrome outlines despite loading NotoColorEmoji
**Root cause:** egui's font renderer (ab_glyph) doesn't support color font formats (COLR/CPAL, CBDT/CBLC)
**Workaround:** None - this is an upstream limitation. Emojis will render as simple outlines.
**Files:** `src/main.rs`

### egui 0.33 Painter API changes
**Context:** Implementing minimap with custom drawing
**Changes:**
- `screen_rect()` deprecated - use `available_rect()` or `content_rect()`
- `rect_stroke()` now requires 4th argument: `StrokeKind` (Inside, Outside, Middle)
```rust
painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Outside);
```
**Files:** `src/main.rs`

### allocate_painter for custom drawing
**Context:** Drawing minimap blocks
**Pattern:** Use `ui.allocate_painter(size, sense)` to get painter and response:
```rust
let (response, painter) = ui.allocate_painter(
    egui::vec2(width, height),
    egui::Sense::click_and_drag(),
);
painter.rect_filled(rect, rounding, color);
```
**Files:** `src/main.rs`

### Text selection clears when content leaves viewport (BY DESIGN)
**Context:** Implementing scroll-while-selecting feature
**Problem:** Text selection disappears when selected content scrolls out of view
**Root cause:** egui intentionally clears selection in `label_text_selection.rs`:
```rust
if !state.has_reached_primary || !state.has_reached_secondary {
    // We didn't see both cursors this frame - deselect to avoid glitches
    let prev_selection = state.selection.take();
}
```
**Why:** egui validates selection every frame by checking if cursor endpoints were "seen" during rendering. When labels scroll out of view, they aren't rendered, endpoints aren't reached, selection is cleared.
**Workarounds:**
- Accept limitation (selection works if content stays in viewport)
- Disable selection: `ui.style_mut().interaction.selectable_labels = false;`
- Use TextEdit instead of Labels (handles own selection state)
- Implement custom selection tracking (significant effort)
**Files:** `src/main.rs`, `docs/devlog/009-drag-scroll.md`

### Scroll during selection requires post-render state modification
**Context:** Mouse wheel scroll while selecting text
**Problem:** Setting `scroll_area.vertical_scroll_offset()` BEFORE rendering breaks selection
**Fix:** Modify scroll state AFTER `show_viewport()`:
```rust
let mut scroll_output = scroll_area.show_viewport(ui, |ui, viewport| { ... });

// Apply scroll AFTER rendering
if raw_scroll.abs() > 0.0 {
    scroll_output.state.offset.y = new_offset;
    scroll_output.state.store(ui.ctx(), scroll_output.id);
}
```
**Gotcha:** Avoid `state.store()` at scroll boundaries (offset near 0 or max) - can still break selection
**Files:** `src/main.rs`

### Document-level keyboard shortcuts must not steal mode-specific keys
**Context:** Issue #29 keyboard document scrolling added Up/Down and Page Up/Page Down document movement.
**Problem:** Arrow keys already have a mode-specific meaning in the find bar: Up/Down cycle search matches. A global document-scroll handler that consumes the same keys unconditionally would make focused/mode shortcuts feel broken.
**Fix:** Gate document-level shortcut handling on UI state and modifiers. Let search consume arrows while the find bar is open, and ignore Ctrl/Alt/Command-modified keypresses so existing app and system shortcuts keep priority. Defer the chosen scroll action through `pending_scroll_offset` rather than mutating renderer-owned scroll state directly.
**Files:** `src/main.rs`, `docs/devlog/035-keyboard-scroll-keys.md`

### Resize handle and scrollbar overlap causes jitter
**Context:** Resizing outline panel caused mouse jitter when near content scrollbar
**Problem:** The SidePanel resize handle sensing area overlaps with the adjacent ScrollArea scrollbar, causing rapid switching between resize and scroll modes.
**Fix:** Combine reduced grab radius with minimal margin:
```rust
// 1. Reduce resize grab radius
style.interaction.resize_grab_radius_side = 2.0; // Default ~5.0

// 2. Add minimal right margin to content area (not visually noticeable)
egui::Frame::none()
    .inner_margin(egui::Margin { right: 3, ..Default::default() })
    .show(ui, |ui| { /* content */ });
```
**Why both?** Reduced grab radius alone isn't enough. The 3px margin provides physical separation without creating a visible gap (8px was too much and created a black gap).
**Files:** `src/main.rs`

---

## Custom Tab System

### Avoid iterating and mutating tabs simultaneously
**Context:** Rendering tab bar while handling close actions
**Problem:** Can't mutate `self.tabs` while iterating over it in a closure
**Fix:** Collect tab info (title, active state) before iterating, then process actions after:
```rust
// Collect data first
let tab_info: Vec<(String, bool)> = self.tabs.iter()
    .enumerate()
    .map(|(idx, tab)| (tab.title(), idx == self.active_tab))
    .collect();

// Then iterate over collected data
for (idx, (title, is_active)) in tab_info.iter().enumerate() {
    // Render without borrowing self.tabs
}

// Handle mutations after the loop
if let Some(idx) = tab_to_close {
    self.close_tab(idx);
}
```
**Files:** `src/main.rs`

### ui.close() replaces ui.close_menu()
**Context:** Closing context menus in egui 0.33
**Problem:** `ui.close_menu()` is deprecated
**Fix:** Use `ui.close()` instead
**Files:** `src/main.rs`

---

## notify / File Watching

### notify-debouncer-mini 0.4 requires notify 6.x
**Context:** Dependency resolution failed
**Problem:** Version mismatch between notify crate versions
**Fix:** Pin `notify = "6.1"` when using `notify-debouncer-mini = "0.4"`
**Files:** `Cargo.toml`

### File watcher can fail silently
**Context:** Live reload stopped working after system sleep
**Fix:** Implement auto-recovery with retry limit:
```rust
// Auto-recover up to 3 times on watcher failure
if watcher_error && recovery_count < 3 {
    self.setup_watcher();
    recovery_count += 1;
}
```
**Files:** `src/main.rs`

### Recursive watch of the explorer root froze startup (~6s) proportional to tree size
**Context:** Launching md-viewer hung ~6s before the first frame whenever the file explorer root was a large tree. With `explorer_root = /home/ahmet` (454,709 dirs), KDE DrKonqi recorded an "application-not-responding" marker at the exact launch time.
**Root cause:** `start_watching()` registered the explorer root with `notify::RecursiveMode::Recursive`. notify's inotify backend implements recursive watching by **walking the entire subtree and issuing one `inotify_add_watch` per directory, synchronously on the calling thread**. `start_watching()` runs inside `MarkdownApp::new()` *before* the eframe event loop, so the walk blocked the first paint. A cache-warm walk of `/home/ahmet` alone measured 6.11s; the watch also consumed ~87% of `max_user_watches` (524,288). The document being opened was irrelevant — any file triggered it.
**Two things to keep separate:** the explorer *scan* (`scan_directory_shallow`) is already lazy/one-level and fine; only the *watch* recursed everything.
**Fix:** Watch the root **plus each currently-expanded directory, non-recursively** — mirroring the lazy tree. A non-recursive inotify watch on dir `D` reports create/delete/modify of `D`'s direct entries, which is exactly what's visible. Keep watches in sync on expand/collapse via a new `reconcile_explorer_watches()` (incremental add/remove diff, same pattern as `update_watched_paths`), called after `toggle_expanded`/`expand_all`/`collapse_all`.
**Why nothing breaks:** open-tab reload uses individual per-tab watches (independent of the root watch); the `path.starts_with(root)` tree-refresh trigger still fires for all visible dirs. Bonus: eliminates spurious whole-tree refreshes from invisible deep changes.
**Result (Xvfb, isolated `XDG_DATA_HOME`):** time-to-window 6s → 0.11s; inotify watches ~455,000 → ~10 (root + tab + expanded dirs).
**General lesson:** `notify`'s `RecursiveMode::Recursive` is O(directories) synchronous work at `watch()` time — never call it on an arbitrarily large/user-chosen root on a UI-blocking path. Watch only what's visible, or watch off-thread *and* bound the set.
**Files:** `src/main.rs` (`start_watching`, `reconcile_explorer_watches`, `watched_explorer_dirs`), `docs/devlog/041-explorer-watch-nonrecursive.md`

---

## Path Resolution

### Relative links need canonicalize()
**Context:** Links like `../other/file.md` weren't resolving
**Fix:** Use `canonicalize()` to resolve `../` in paths:
```rust
let target_path = current_dir.join(path_part);
let target_path = target_path.canonicalize()?;  // Resolves ../
```
**Files:** `src/main.rs`

---

## Vendoring Dependencies

### Prefer in-repo vendoring over git dependencies
**Context:** Forking egui_commonmark for typography
**Decision:** Vendor in `crates/egui_commonmark/` rather than git dependency
**Why:**
- Easier to modify and debug
- No network dependency for builds
- Clear diff of changes vs upstream
- Simpler rebasing when upstream updates

**Files:** `Cargo.toml`, `crates/`

---

## Typography Research

### WCAG 2.1 SC 1.4.12 line height requirement
**Context:** Accessibility compliance
**Standard:** Must support 1.5x line height minimum
**Source:** Rello et al. (CHI 2016) eye-tracking study confirmed 1.5x optimal

### Optimal line length is 55-75 CPL
**Context:** Setting max content width
**Finding:** ~600px max width for 16px body text
**Source:** Dyson & Haselgrove (2001)

### Off-white reduces eye strain
**Context:** Background color selection
**Fix:** Use `#F8F8F8` instead of pure white (anti-halation)
**Source:** Material Design guidelines

---

## Git Worktrees

### Branch protection via Claude Code hook
**Context:** Preventing accidental edits to main
**Solution:** Hook in `.claude/hooks/` that blocks edits on main branch
**Files:** `.claude/hooks/`

### Bare repo structure for worktrees
**Context:** Managing multiple feature branches
**Structure:**
```
~/markdown-viewer/
├── .bare/           # Git database
└── worktrees/
    ├── main/        # Main branch
    └── feature-x/   # Feature branches
```
**Benefit:** Each worktree is fully isolated with own working directory

---

## egui MCP / Virtual Display Testing

### Wayland systems ignore DISPLAY env var
**Context:** Testing egui apps on virtual X11 display (Xvfb)
**Problem:** Setting `DISPLAY=:99` doesn't work on Wayland systems - app still opens on real screen
**Root cause:** egui/winit defaults to Wayland when available, ignoring DISPLAY
**Solution:** Use `egui_launch` MCP tool which auto-detects this and forces X11 mode:
```
egui_launch({
  applicationPath: "./target/debug/app",
  env: { "DISPLAY": ":99" }  // Tool auto-adds WINIT_UNIX_BACKEND=x11
})
```
**Manual workaround:**
```bash
DISPLAY=:99 WINIT_UNIX_BACKEND=x11 WAYLAND_DISPLAY= ./app
```
**Files:** `~/.claude/CLAUDE.md`, `~/egui-mcp/`

### Inline code in headers renders incorrectly
**Context:** Headers like `### Title (`code`)` rendered garbled text
**Problem:** Each text fragment in a heading was getting its own `allocate_ui_at_rect()` call to force left alignment, causing each fragment to reset to x=0 instead of flowing inline.
**Fix:** Accumulate all heading RichText fragments, render once at end:
```rust
// Add field to accumulate fragments
current_heading_rich_texts: Vec<egui::RichText>,

// In event_text (heading case) - accumulate, don't render
self.current_heading_rich_texts.push(rich_text);

// In end_tag(Heading) - render all at once
let rich_texts = std::mem::take(&mut self.current_heading_rich_texts);
ui.allocate_ui_at_rect(heading_rect, |ui| {
    for rt in rich_texts {
        ui.label(rt);
    }
});
```
**Key insight:** Single `allocate_ui_at_rect` for left alignment + multiple labels inside for inline flow.
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`

### High CPU on virtual displays (Xvfb)
**Context:** E2E testing on Xvfb uses 500%+ CPU
**Problem:** Virtual displays lack vsync, so egui renders at unlimited FPS
**Root cause:**
1. `ctx.request_repaint()` (without delay) triggers unlimited repaints
2. Even with `request_repaint_after()`, the rendering loop runs continuously because Xvfb doesn't block on vsync
**Fix:** Detect virtual display and add explicit sleep:
```rust
// In struct
is_virtual_display: bool,

// In constructor
let is_virtual_display = std::env::var("DISPLAY")
    .map(|d| d != ":0" && d != ":0.0" && !d.is_empty())
    .unwrap_or(false);

// In update()
if self.is_virtual_display {
    std::thread::sleep(Duration::from_millis(16)); // ~60 FPS cap
}

// For MCP polling, use delay:
ctx.request_repaint_after(Duration::from_millis(50)); // NOT request_repaint()
```
**Result:** CPU drops from 500%+ to ~220%
**Files:** `src/main.rs`

### "Connection reset by peer" during egui_launch
**Context:** Bridge logs errors during MCP connection polling
**Problem:** `egui_launch` polls every 200ms to check if bridge is ready, connecting then disconnecting
**Explanation:** This is normal behavior during startup - not an actual error
**Note:** The error can be ignored; it's just polling to detect when the app is ready
**Files:** `~/egui-mcp/crates/egui-mcp-bridge/src/server.rs`

---

## Distribution / CI / Packaging

### GitHub Actions blocks `secrets.*` AND `env.*` in job-level `if:`
**Context:** Wiring an optional `publish-aur` job gated on `AUR_SSH_PRIVATE_KEY` being set
**Problem:** `if: ${{ secrets.AUR_SSH_PRIVATE_KEY != '' }}` caused the whole workflow file to fail validation (no jobs run, "workflow file issue" with 0s duration). Switching to `if: ${{ env.AUR_SSH_KEY != '' }}` also fails — only `github`, `needs`, `vars`, and `inputs` contexts are allowed in job-level `if:`.
**Fix:** Gate at step level instead. First step always succeeds, reads the secret, writes `proceed=true|false` to `$GITHUB_OUTPUT`. Every subsequent step has `if: steps.check.outputs.proceed == 'true'`. The job shows green with a notice when the secret is unset.
```yaml
- name: Check secret
  id: check
  env:
    AUR_SSH_KEY: ${{ secrets.AUR_SSH_PRIVATE_KEY }}
  run: |
    if [ -z "$AUR_SSH_KEY" ]; then
      echo "proceed=false" >> "$GITHUB_OUTPUT"
    else
      echo "proceed=true" >> "$GITHUB_OUTPUT"
    fi
- uses: actions/checkout@v4
  if: steps.check.outputs.proceed == 'true'
```
**Files:** `.github/workflows/release.yml`

### Docker bind-mount `chown` breaks host runner ownership
**Context:** Regenerating `.SRCINFO` inside a containerized `archlinux/archlinux:base-devel` because `makepkg` refuses to run as root
**Problem:** After `docker run --rm -v "$PWD/aur-repo:/pkg" ... bash -c 'useradd -m builder && chown -R builder /pkg && sudo -u builder makepkg --printsrcinfo'`, the next host step failed with `error: could not lock config file .git/config: Permission denied`. The container's `chown -R builder` propagates through the bind mount, so the host runner can no longer write inside `aur-repo`.
**Fix:** Restore ownership to the runner immediately after the container exits:
```yaml
- name: Restore aur-repo ownership to runner
  run: sudo chown -R "$(id -u):$(id -g)" aur-repo
```
**Files:** `.github/workflows/release.yml`

### GitHub macos-13 (Intel) runners have multi-minute queue waits
**Context:** Cross-platform release matrix including `macos-13` for Intel Mac users
**Problem:** v0.1.3 release run sat queued 24+ minutes for an Intel Mac runner while linux/macos-arm64/windows finished. The free macOS-13 runner pool is heavily contended.
**Fix:** Drop Intel Mac from the matrix. Modern Macs are all Apple Silicon — `aarch64-apple-darwin` covers the bulk. Direct Intel Mac users to `cargo install`.
**Files:** `.github/workflows/release.yml`

### Vendored forks must publish to crates.io with feature parity, under renamed identifiers
**Context:** Restoring crates.io auto-publish after it was removed in PR #11.
**Problem:** `cargo publish` ignores `[patch.crates-io]` during its verify step (resolves deps against the registry directly). If the registry version of a patched crate lacks a feature the consumer asks for, publish fails:
```
package `md-viewer` depends on `egui_commonmark_extended` with feature `math`
but `egui_commonmark_extended` does not have that feature
```
**Fix:** Publish the vendored fork under renamed identifiers (`*_extended` suffix here) so they don't conflict with upstream `lampsitter/egui_commonmark`, and *keep registry feature parity*. When you add a feature to the local fork, bump the fork's workspace version and publish the new version *before* tagging md-viewer — otherwise md-viewer's publish-verify will fail against the older registry version.
**Operational details (see `scripts/publish-crates.sh` + `publish-crates` job in `release.yml`):**
- Publish order: backend → macros → extended → md-viewer.
- Sparse-index settle delay (45 s) between publishes — otherwise dependents fail to resolve the new version.
- "Already uploaded" treated as success → idempotent on re-tags.
- `[patch.crates-io]` stays in root `Cargo.toml`. It's neutral for publish (ignored) and keeps local dev fast when iterating between fork bumps.
- Pitfall: git URLs are NOT a workaround — `cargo publish` rejects any dep not on crates.io.
**Files:** `Cargo.toml`, `crates/egui_commonmark/Cargo.toml`, `.github/workflows/release.yml`, `scripts/publish-crates.sh`, `PUBLISHING.md`

### Flathub linter rejects `--filesystem=home:ro` (exception pattern)
**Context:** Tightening sandbox permissions for the Flatpak manifest
**Problem:** `flatpak-builder-lint` flags `finish-args-home-ro-filesystem-access` as an error for `--filesystem=home:ro`. Stricter alternatives (`xdg-documents` only) break CLI invocation outside `~/Documents` and live reload for arbitrary paths. `--filesystem=host:ro` is also flagged (worse — exposes `/etc`, `/usr`).
**Fix:** Keep `--filesystem=home:ro` (functional priority) and document the exception in the Flathub PR body. For a read-only markdown viewer that needs CLI invocation + `notify` watcher access, this is a defensible exception — reviewers grant it for similar markdown editors/viewers. Document in `PUBLISHING.md` so the next person knows the lint error is expected.
**Files:** `flatpak/io.github.aydiler.md-viewer.yaml`, `PUBLISHING.md`

### Default-detached GUI CLIs still need a foreground escape hatch
**Context:** Issue #30 — launching `md-viewer README.md` from a terminal kept the shell occupied until the GUI closed.
**Root cause:** `eframe::run_native` runs in the foreground process. Desktop launch was unaffected because the `.desktop` file uses `Terminal=false`, but direct CLI launch behaved like any foreground command.
**Fix:** Detect terminal launch, respawn the same executable with a hidden child marker (`--no-detach`) and null stdio, run Unix children in a new session with `setsid()`, then let the parent exit. Keep a documented `--foreground` flag so startup errors, logs, and scripts can still use blocking behavior. Insert the hidden marker before `--` so clap does not treat it as a positional file argument.
**Files:** `src/main.rs`, `README.md`

### `--talk-name=*portal*` is unnecessary for Flatpak XDG portals
**Context:** First-pass Flatpak manifest included `--talk-name=org.freedesktop.portal.FileChooser` and `.Desktop`
**Problem:** `flatpak-builder-lint` flags `finish-args-portal-talk-name`. Flatpak apps reach XDG portals through the standard sandbox mechanism without explicit D-Bus talk-name permissions.
**Fix:** Remove the `--talk-name=*portal*` lines entirely. Native file dialogs (via `rfd` crate) still work through the portal mechanism.
**Files:** `flatpak/io.github.aydiler.md-viewer.yaml`

### Pollinations.ai is the keyless image-gen fallback
**Context:** Replacing a placeholder app icon without a Gemini/OpenAI API key
**Problem:** The `gemini` CLI's OAuth-personal auth (free tier) only grants access to text models. Image-gen models (`gemini-3.1-flash-image-preview`, `gemini-2.5-flash-image`, etc.) return `ModelNotFoundError: Requested entity was not found` (HTTP 404). Setting `GEMINI_API_KEY` in `~/.gemini/.env` would fix it but requires user action.
**Fix:** Pollinations.ai requires no key:
```bash
PROMPT="..."
ENC=$(python3 -c "import urllib.parse,sys; print(urllib.parse.quote(sys.argv[1]))" "$PROMPT")
curl -fsSL --max-time 90 -o /tmp/icon.png \
    "https://image.pollinations.ai/prompt/${ENC}?width=1024&height=1024&model=flux&nologo=true&seed=42"
```
Returns JPEG despite `.png` extension; convert via ImageMagick. Flux is default model. Image often has a margin around any "rounded card" subject — crop to remove:
```bash
magick X.png -gravity center -crop 540x540+0+0 +repage -resize 256x256 -strip out.png
```
**Files:** N/A (one-off icon generation pattern)

### GitHub user-level block stops Flathub PRs entirely
**Context:** Attempting to open a PR against `flathub/flathub` from the `aydiler` account
**Problem:** `gh pr create` returned HTTP 422 with `{"resource":"Issue","code":"custom","field":"user","message":"user is blocked"}`. Direct `curl` against the GitHub API confirms the block; org-block check (`/orgs/flathub/blocks/aydiler`) returns 404 (not org-level) so the block is at the GitHub user level — likely auto-triggered by previous activity (the account had 3 close/reopen PRs for `io.github.aydiler.msigd-gui` in January 2026).
**Workaround:** Open a topic on Flathub Discourse (https://discourse.flathub.org/) asking for unblock; reference the closed prior PRs and the new app. The push to `aydiler/flathub:io.github.aydiler.md-viewer` branch succeeds before the PR step, so once unblocked the PR can be opened from the GitHub web UI without redoing branch work.
**Files:** N/A (account-level state)

### Never `snapcraft --destructive-mode` for releases on a glibc-newer-than-base host
**Context:** Issue #3 — v0.1.2 snap (revision 4) failed to start on Ubuntu 24.04 with `GLIBC_2.43 not found` errors. The snapcraft.yaml declared `base: core22` (glibc 2.35), but the actual binary required GLIBC_2.43.
**Root cause:** v0.1.2 release CI run failed; the snap was manually uploaded with `snapcraft --destructive-mode` from this Arch host (glibc 2.43). Destructive mode builds directly on the host without LXD/multipass isolation, so cargo links against host glibc — `atan2f@GLIBC_2.43`, `acosf@GLIBC_2.43` etc. get baked into the binary regardless of the declared `base:`.
**Fix:** Only publish snaps via CI (`snapcore/action-build@v1` uses LXD with the declared base). If CI fails, fix CI — never fall back to destructive-mode upload. The v0.1.3 CI run produced revision 6, which only requires up to `GLIBC_2.35` and works on Ubuntu 22.04+.
**Verify before upload:**
```bash
objdump -T target/release/md-viewer | grep -oE 'GLIBC_[0-9.]+' | sort -u | tail
# Must not exceed the glibc of the declared `base:` in snapcraft.yaml
# core22 → 2.35, core24 → 2.39
```
**Files:** `snap/snapcraft.yaml`, `.github/workflows/release.yml`

### snapcraft renamed `push-metadata` → `upload-metadata` (the whole `push` verb family)
**Context:** v0.1.14 release. The **Publish to Snap Store** job went red, but the snap itself published fine — `snapcore/action-publish@v1` logged `Revision 17 created for 'md-viewer' and released to 'stable'`. The failure was the *next* step, "Push snap store listing metadata", which runs `snapcraft push-metadata "${SNAP_FILE}" --force`:
```
Error: no such command 'push-metadata', maybe you meant 'upload-metadata'
```
**Root cause:** the step does `sudo snap install snapcraft --classic` (tracks *latest*), and newer snapcraft renamed the entire `push` verb family to `upload` (`push` → `upload`, `push-metadata` → `upload-metadata`). This worked at v0.1.13 (~5 weeks earlier) because the runner's snapcraft was older — **environmental drift**, not anything the release commit changed. It would now fail on every release.
**Fix:** `snapcraft upload-metadata "${SNAP_FILE}" --force` — identical positional `<snap-file>` and `--force` flag, just the renamed verb.
**Non-impact:** the snap upload is a *separate* step (`action-publish`) that already succeeded, so users got the new revision regardless; only the store-listing description sync (unchanged from prior releases anyway) was skipped. The overall run still shows red because of the one trailing step.
**Guard for next time:** consider pinning `snap install snapcraft --classic --channel=8.x/stable` in this step so the verb surface can't drift out from under the release again.
**Files:** `.github/workflows/release.yml` (Push snap store listing metadata step), `docs/devlog/050-snap-upload-metadata-verb.md`

### `to_ascii_lowercase` preserves byte offsets; `to_lowercase` doesn't
**Context:** Implementing case-insensitive substring search (`find_matches`) that returns byte ranges into the original content.
**Problem:** `str::to_lowercase` does Unicode case folding which can *change byte length* (e.g. `"İ".to_lowercase() == "i̇"` — adds a combining mark). Any match offsets computed against the folded string would point at the wrong bytes in the original.
**Fix:** Use `str::to_ascii_lowercase` on both sides. It only rewrites A–Z; non-ASCII bytes pass through untouched, so byte offsets stay 1:1 with the original.
```rust
let content_lc = content.to_ascii_lowercase();
let query_lc = query.to_ascii_lowercase();
for (byte_start, _) in content_lc.match_indices(&query_lc) {
    // byte_start is a valid offset into the original `content`
}
```
**Trade-off:** Non-ASCII case folding doesn't work (`É` won't match `é`). For a v1 simple search this was the right cut; the case-toggle and full Unicode folding are tracked in `docs/devlog/019-search-find.md` Future Improvements.
**Files:** `src/main.rs` (`find_matches`)

### Search-highlight in headings must go through `current_heading_rich_texts`
**Context:** Search-match highlighting in `pulldown.rs` `Event::Text`
**Gotcha:** The existing "Inline code in headers renders incorrectly" lesson taught that heading text fragments must accumulate into `current_heading_rich_texts` and render in `end_tag(Heading)` inside a single `allocate_ui_at_rect`. The new `emit_text` helper that applies highlight background colors MUST preserve this routing, otherwise highlighted segments in headings reset to x=0 individually and the header is rendered as fragmented overlapping text.
**Fix:** `emit_text` keeps the same `if self.text_style.heading.is_some()` branch the original `event_text` had — only the per-segment `RichText` gets a `.background_color(...)` applied. The accumulator routing is identical.
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`

### MCP bridge has no keystroke injection; menus aren't in AccessKit
**Context:** Verifying Ctrl+F end-to-end via the egui MCP bridge on Xvfb
**Problem:** `xdotool key --window <id> ctrl+f` doesn't route to the egui app on Xvfb because there's no window manager to handle focus, AND the egui MCP bridge only exposes `egui_click` / `egui_type` (no `egui_keystroke`). Egui's `MenuBar::menu_button("File", ...)` widgets also don't surface their sub-buttons in the AccessKit tree, so triggering search via `File → Find...` click also fails.
**Workaround:** For E2E testing of keyboard-shortcut-only features, either run on a real X11/Wayland session, or add a debug CLI flag that pre-opens the relevant UI state. For PRs, lean harder on unit tests + a no-regression AccessKit snapshot (count nodes before/after).
**Files:** N/A (external limitation)

### Search matches must skip non-renderable markdown spans (alt-text + URLs)
**Context:** Find-feature cycling appears "stuck" — user presses Enter but the active highlight doesn't visibly move; sometimes the view scrolls to a position where the matched text isn't visible.
**Root cause:** `find_matches` scans raw byte content. A query like "syntax" against the README returned 10 matches, but **several were inside `![alt-text](url)` markdown** — alt text is hover/screen-reader only, URLs are never visible. Cycling to those matches:
- Painted the active highlight at bytes that don't appear on screen → no visible color change
- Scrolled to the markdown source line of the `![...](...)` (which is where the *image* renders, not where the matched bytes are) → wrong position
The user perceives "highlight stuck on first result" because the visible (non-active) matches all wear the dim color while the actual active match is invisible.
**Fix:** In `find_matches`, regex-match `(!?)\[([^\]]*)\]\(([^)]*)\)` and filter out matches whose byte range falls in:
- The URL portion (group 3) — always, for both images and links
- The alt portion (group 2) — only when group 1 is `!` (image)
Link *text* stays in scope because it IS rendered.
**Files:** `src/main.rs` (`find_matches`)

### Line-ratio scroll is unreliable in image-heavy docs — record actual y during paint
**Context:** Auto-scroll to the active search match lands the wrong content on screen for matches past several images.
**Root cause:** `(line_number / total_lines) * content_height` assumes uniform per-line height. In a README with multiple 400 px+ images, the actual rendered y of a text line is far below what the ratio estimates. Even subtracting 35% of viewport height as a margin isn't enough — match 4 of "syntax" in the README is past three large images and ends up off-screen below the viewport.
**Fix:** Two-stage scroll.
1. `scroll_to_active_match()` (called from `jump_match` and `maybe_rebuild_search`) sets `pending_scroll_offset` from the line-ratio estimate — gets the view roughly in the right area.
2. During render, when `emit_text` paints an `Active` highlight segment, the renderer calls `cache.record_active_search_y_viewport(ui.cursor().top())`. The cache stores `current_scroll_offset + viewport_y` as content-relative y.
3. After `show_viewport()` returns, `render_tab_content` reads `cache.active_search_y()`. If the recorded y is outside the visible viewport, schedule a corrective `pending_scroll_offset` for next frame.
Visual effect: one frame lands close (line-ratio), the next frame snaps precise (recorded y). The active match is always in viewport after at most two frames.
**Files:** `crates/egui_commonmark/egui_commonmark_backend/src/misc.rs` (`record_active_search_y_viewport`, `active_search_y`), `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`event_text_with_highlights`), `src/main.rs` (`scroll_to_active_match`, `render_tab_content` corrective block)

### Heading bg-color rendering wasn't the bug — `allocate_ui_at_rect` is fine
**Context:** Active highlight in h3 heading rendered as `HL_DARK` instead of `HL_ACTIVE_DARK`. Spent ~30 min suspecting `allocate_ui_at_rect` in `end_tag(Heading)` suppressed `RichText.background_color`.
**What was actually happening:** The "active" match's byte range pointed inside an image alt-text — so the active *was* applied but to invisible bytes; the visible heading "Syntax" got the regular (Match) color because it was a *different* search range. Pixel sampling showed only `HL_DARK` pixels because there were no visible Active spans on that frame.
**Diagnostic that would have caught it sooner:** log the active range's byte position AND a content-context window around it before chasing renderer-side hypotheses. If `active = Some(2823..2829)` and `content[2790..2870]` is `![Syntax Highlighting](...)`, you know the match is in alt text *before* touching the renderer.
**Files:** N/A (debugging discipline)

### TableBuilder fixed-height rows clip multi-line cell content
**Context:** Refactor of markdown-table renderer from `egui::Grid` to `egui_extras::TableBuilder`.
**Symptom:** Long inline-code paths in table cells render only their first 56-char chunk. `inline_code_wrap_segments` still produces multiple chunks and `ui.end_row()` inside the cell still advances the cursor, but chunks 2+ are invisible.
**Root cause:** `body.row(h, ...)` declares a *fixed* row height. The row's bounding clip rect is `h` tall. Cell content rendered at y > clip rect bottom is hidden. Grid had variable-height rows that grew to content; TableBuilder doesn't.
**Fix:** Pre-compute per-row height from content, pass per-row to `body.row(h, ...)`. Helper:
```rust
fn cell_visual_lines(cell: &[(Event, Range<usize>)]) -> usize {
    let mut max_lines = 1usize;
    for (event, _) in cell {
        if let Event::Code(text) = event {
            let chunks = inline_code_wrap_segments(text).len();
            if chunks > max_lines { max_lines = chunks; }
        }
    }
    max_lines
}

let body_heights: Vec<f32> = rows.iter().map(|row| {
    let max_lines = row.iter().map(|c| cell_visual_lines(c)).max().unwrap_or(1);
    cell_h * max_lines as f32
}).collect();

table.body(|mut body| {
    for (idx, row) in rows.into_iter().enumerate() {
        let h = body_heights.get(idx).copied().unwrap_or(cell_h);
        body.row(h, |mut row_ui| { /* render */ });
    }
});
```
**Edge case:** Cells with multiple wrapping codes — use `max(chunks)` not `sum(chunks)`. The rare case of two separate wrapping codes in one cell renders the second below the first; `sum` would over-allocate, `max` may under-allocate by 1-2 chunks. Pragmatic trade-off for the common case.
**HTML tables:** No event stream, just plain strings. Use a heuristic: `cell.lines().count() + cell.len()/60`. Over-estimates → extra row height (safe). Documented approximation.
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`cell_visual_lines`, `html_cell_visual_lines`, `fn table`, `fn render_html_table`)

### TableBuilder columns clip on narrow window without outer ScrollArea::horizontal
**Context:** Same refactor.
**Symptom:** A wide 10-column table renders only C1-C8 when the window narrows; C9, C10 are clipped at the panel edge with no scrollbar.
**Root cause:** Two factors compound:
1. `Column::auto()` stores measured widths in egui's `TableState` keyed by `id_salt`. On subsequent frames the widths are reloaded from state — **they don't re-shrink when the parent narrows.**
2. TableBuilder's `body()` has its own internal `ScrollArea::new([false, vscroll])` — `false` on horizontal. No horizontal scroll mechanism inside TableBuilder.

The original Grid code was wrapped in `egui::ScrollArea::horizontal()`. Grid let columns grow without bound → outer scroll area handled overflow.
**Fix:** Restore the outer `ScrollArea::horizontal()` wrapper around the TableBuilder chain:
```rust
egui::ScrollArea::horizontal()
    .id_salt(id.with("_scroll"))
    .max_width(max_width)
    .auto_shrink([false, true])
    .show(ui, |ui| {
        ui.vertical(|ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                egui_extras::TableBuilder::new(ui).columns(...)...;
            });
        });
    });
```
`auto_shrink([false, true])`: horizontal=false → ScrollArea takes max_width; vertical=true → shrinks to content. Keep `ui.vertical()` for the body/header alignment fix.
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`fn table`, `fn render_html_table`)

### TableBuilder body overlaps header when the parent ui isn't vertical-flow
**Context:** Swapping `egui::Grid` for `egui_extras::TableBuilder` in the vendored markdown renderer.
**Symptom:** Header row and first body row rendered on the same Y position; subsequent body rows correctly stacked below. With 2-column markdown tables this looked like a 4-column row at the top of the table.
**Root cause:** `TableBuilder::header()` returns `Table<'_>`; `Table::body()` then captures `cursor_position = ui.cursor().min` for the body's internal `ScrollArea`. If the parent `Ui` isn't a clean vertical-flow scope (the markdown renderer's parent is a multi-purpose `Ui` from pulldown_cmark event processing where line/blockquote/etc. machinery has its own cursor logic), the cursor after the header doesn't advance vertically and the body's `ScrollArea` starts at the same Y as the header.
**Fix:** Wrap the whole TableBuilder chain in `ui.vertical(|ui| { TableBuilder::new(ui)... })`. The vertical scope forces a fresh vertical-flow Ui whose cursor advances as expected between header and body.
```rust
ui.vertical(|ui| {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let table = egui_extras::TableBuilder::new(ui)
            .columns(Column::auto().resizable(true), num_cols)
            .header(row_h, |row| { /* ... */ });
        table.body(|body| { /* ... */ });
    });
});
```
**Diagnostic discipline:** Replace recursive cell rendering with stub labels (`ui.label(format!("R{}C{}", ri, ci))`) when debugging TableBuilder layout. Rich content hides whether the problem is in the data path or the layout path.
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`table`, `render_html_table`)

### parse_table's `header` is Vec<Cell>, not Vec<Row>
**Context:** Computing the column count from a parsed markdown table.
**Pitfall:** `parse_table(events).header` is a `Vec<Cell>` representing the cells of a *single* header row — not multiple rows. `header.first().map(|h| h.len())` therefore returns the **event count of the first cell** (e.g., `[Text("Status")]` has 1 event), NOT the column count. Using that as the TableBuilder column count panics with `Added more Table columns than were pre-allocated` as soon as `row.col()` is called more times than the wrong-counted columns.
**Fix:** `num_cols = if !header.is_empty() { header.len() } else { rows.first().map(|r| r.len()).unwrap_or(0) }`.
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`table`)

### parse_table trailing empty row from pulldown_cmark
**Context:** Debug-printing `rows.len()` after `parse_table(events)`.
**Observation:** A markdown table with 3 data rows is returned as 4 rows from `parse_table` — the last one has 0 cells. Likely from how pulldown_cmark emits the closing `TableEnd`/blank-line events.
**Mitigation:** Filter empty rows before rendering: `rows.into_iter().filter(|r| !r.is_empty()).collect::<Vec<_>>()`. Without this, TableBuilder's `body.row()` runs with no cells and may render a 0-cell phantom row (no visible effect, but wasted work).
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`table`)

### Inline-code wrap segmentation: blind char-count cut, not break-friendly chars
**Context:** Issue #5 — long inline-code tokens (file paths) overflowed the content column, clipping leading characters and overlapping adjacent text. Fixed by splitting long tokens into chunks separated by `ui.end_row()` in `Event::Code` handling.
**First attempt that regressed:** Splitting at break-friendly characters (`/ \ - _ . :`) past 56 chars to keep paths readable. At narrow window widths the variable-length segments (60-120 chars) still exceeded the column width, and egui's intra-widget wrap re-introduced the original clipping bug.
**Fix:** Blind fixed-size char-count cut. Each chunk has a known upper bound (56 chars) so it always fits the column. Mid-identifier breaks are a cosmetic cost, but functionally correct at every width.
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`inline_code_wrap_segments`)

### `publish-aur-bin` races `create-release` — pull tarball sha256 from build artifact, not Release URL
**Context:** v0.1.8 first run. The new `publish-aur-bin` job was wired as `needs: build` so it could parallelise with `publish-snap` and `publish-aur`. Its first step `curl -fsSL ".../releases/download/v${VERSION}/md-viewer-${VERSION}-linux-x86_64.tar.gz.sha256"` failed with curl exit 22 (HTTP 4xx) in 12 seconds.
**Root cause:** The GitHub Release isn't *created* until `create-release` runs, which is `needs: [build, publish-snap]`. publish-aur-bin had no dependency on either snap or create-release, so it raced ahead of the Release URL existing. publish-aur doesn't hit this because the source-build PKGBUILD has `sha256sums=('SKIP')` — it never fetches the release asset.
**Fix:** Use `actions/download-artifact@v4` to pull the `release-linux-x86_64` artifact (uploaded by the `build` matrix) directly inside `publish-aur-bin`. The artifact already contains `md-viewer-VERSION-linux-x86_64.tar.gz.sha256` — the same file that later ends up on the Release page. No ordering dependency on snap or create-release.
**Alternative considered:** `needs: create-release` — adds ~20 min serial latency (snap is slow); rejected.
**Aux files** (`.desktop`, icon, LICENSE) still come from `raw.githubusercontent.com/aydiler/md-viewer/v${VERSION}/...` — those URLs are valid as soon as the tag is pushed, no race.
**Recovery for the failed v0.1.8 run:** rerunning the failed job from GitHub doesn't help — `gh run rerun` uses the workflow code as it existed at tag time. Either re-tag (destructive) or push the package manually. I did the manual push; CI fix landed as `c552f10` on main for v0.1.9+.
**Files:** `.github/workflows/release.yml` (`publish-aur-bin`)

### `sed 's/^version = "0.1.7"$/.../' Cargo.lock` bumps unrelated crates
**Context:** Releasing v0.1.8 — needed to bump `md-viewer`'s entry in `Cargo.lock` to match the new `Cargo.toml` version.
**Pitfall:** `sed -i 's/^version = "0.1.7"$/version = "0.1.8"/' Cargo.lock` rewrote 5 lines, not 1. Four other crates (`crypto-common`, etc.) happened to be at `0.1.7` and silently bumped to `0.1.8` — invalid versions, would have broken builds.
**Fix:** Use `Edit` with surrounding context to scope to the `md-viewer` block:
```
old: name = "md-viewer"\nversion = "0.1.7"
new: name = "md-viewer"\nversion = "0.1.8"
```
Or run `cargo update -p md-viewer --precise 0.1.8` from a clean tree (slower but unambiguous).
**Recovery:** `git checkout Cargo.lock` and redo surgically. Always `git diff Cargo.lock` after touching it — anything other than one line under `[[package]] name = "md-viewer"` is a mistake.
**Files:** `Cargo.lock`

### MCP-strip Python transform must anchor regex to start-of-line
**Context:** v0.1.9 release. After publishing the 3 fork crates, `publish-crates` failed at md-viewer's `cargo publish` with `1 files in the working directory contain changes that were not yet committed into git: Cargo.toml`.
**Root cause:** the "Remove local-only MCP dependency" CI step used plain `str.replace`:
```python
t = t.replace('mcp = ["dep:egui-mcp-bridge"]', 'mcp = []')
```
The target string is a *substring* of the commented line `# mcp = ["dep:egui-mcp-bridge"]`, so the replace rewrites it to `# mcp = []`. The line is still a comment (identical effect on the build), but git sees it as a change → `cargo publish` dirty-check aborts the upload.
**Fix:** anchor at start-of-line with a regex so commented lines (starting with `#`) are skipped:
```python
t = re.sub(r'(?m)^mcp\s*=\s*\["dep:egui-mcp-bridge"\]', 'mcp = []', t)
t = re.sub(r'(?m)^egui-mcp-bridge\s*=\s*.*\n', '', t)
```
Belt-and-suspenders: keep `cargo publish --allow-dirty` in `scripts/publish-crates.sh` so a future maintainer who DOES uncomment the dep for local MCP testing (and forgets to recomment before tagging) still gets a clean publish — the transform strips the uncommented lines, working dir becomes dirty, `--allow-dirty` lets the publish through.
**Recovery for v0.1.9:** the fork crates DID publish (their working dirs were unaffected by the root Cargo.toml mutation); only md-viewer's publish failed. Manual `cargo publish` from a clean local checkout shipped 0.1.9.
**Files:** `.github/workflows/release.yml` (build job + publish-crates job — both have the transform), `scripts/publish-crates.sh`

### `CHANGELOG.md` is hand-curated — do NOT `git-cliff -o CHANGELOG.md`
**Context:** `.claude/rules/release-workflow.md` suggests running `git-cliff -o CHANGELOG.md` to generate changelog entries before tagging.
**Problem:** git-cliff parses conventional commits. This repo's commit history doesn't conform (171/N commits skipped on the v0.1.8 attempt), so the generated CHANGELOG is sparse and drops entire versions (e.g. v0.1.4, v0.1.6, v0.1.7 vanished). Running `-o` overwrites the existing rich hand-written prose with the degraded version.
**Fix:** For new versions, prepend a `## [X.Y.Z] - YYYY-MM-DD` section manually using the existing entry style (rich prose with PR refs, root-cause + fix structure). git-cliff can be used as a *starting point* (`git-cliff --tag vX.Y.Z` to stdout) but the output requires heavy editing and shouldn't overwrite the file.
**Recovery:** `git checkout CHANGELOG.md` if you accidentally regenerated.
**Files:** `CHANGELOG.md`, `.claude/rules/release-workflow.md` (rule could be tightened with a "git-cliff for inspiration only" note).

### Renderer transformations need borrowed unchanged fast paths
**Context:** GitHub emoji shortcode expansion originally built a `Vec<EmojiTextSegment>` plus owned raw/rendered `String` values for every eligible `Event::Text`, then built another owned highlight vector.
**Problem:** Most text events contain no recognized shortcode, so paint-time allocation churn paid transformation costs for unchanged content.
**Fix:** Use direct visitor callbacks. Plain/raw slices borrow parser input, recognized emoji borrow static `Emoji::as_str()` values, and highlight splitting emits borrowed slices directly. Capture active-match Y during emission, then mutate cache only after immutable search-range borrows end. Tests collect owned snapshots only at the test boundary and use pointer identity to prove no-colon and unknown-only paths borrow original input.
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`

---

## Virtualization

### show_scrollable was "buggy" because split_points were list-only
**Context:** Adopting `CommonMarkViewer::show_scrollable` to fix scroll jank on large docs. Upstream the API was marked `#[doc(hidden)] // Buggy in scenarios more complex than the example application`.
**Root cause:** The split_points-population gate at `parsers/pulldown.rs:347` was `self.list.is_inside_a_list() && is_element_end` — waypoints were only added for events ending *inside lists*. A doc that's mostly headings + paragraphs + code blocks produced *no* split_points. The viewport-skip math then fell back to `Pos2::ZERO` and rendered content overlapped its own tail.
**Fix:** Add a split point at every block-level `Event::End` (Paragraph, Heading, BlockQuote, CodeBlock, List, Item, FootnoteDefinition, Table, HtmlBlock, MetadataBlock, DefinitionList*). Inline ends (Emphasis/Strong/Link/Image/Sup/Sub) stay rejected — splitting mid-paragraph would orphan inline formatting state. Table-internal ends (TableHead/Row/Cell) also rejected since tables are pre-parsed atomically.
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`is_block_end_tag`)

### Cache invalidation must include zoom and theme, not just width
**Context:** `show_scrollable`'s split_points y-coords are layout-dependent. The original invalidator only watched `available_size`.
**Problem:** Zoom (Ctrl+/-) and dark-mode toggle leave stale split_points; viewport-skip then picks the wrong event range and renders garbage.
**Fix:** `compute_layout_signature(ui, options)` hashes width, body font height, monospace font height, `dark_mode`, `default_width`, and `indentation_spaces`. ScrollableCache stores `layout_signature: u64`; mismatch clears `split_points` and `page_size`. The body-font-height term captures egui's zoom factor implicitly — no need to read `pixels_per_point` separately.
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`compute_layout_signature`)

### show_scrollable parsed pulldown on every frame
**Context:** Naive switch to `show_scrollable` would have made scroll *slower* than `show()` did.
**Root cause:** `show()` caches parsed events in `CommonMarkCache::cached_events` keyed by content hash (parsers/pulldown.rs:318-327). `show_scrollable` did not — it ran `Parser::new_ext(text).into_offset_iter().collect()` at line 410-413 every paint, ~52 ms at 100k lines.
**Fix:** Extend `ScrollableCache` with `events`, `content_version`, `layout_signature`. The caller (md-viewer's `Tab`) provides a monotonic `content_version: u64` bumped on every load/reload via the new `CommonMarkViewer::content_version(v)` builder. The renderer reads it via `content_version: Option<u64>` and falls back to `hash_content(text)` when omitted. Either way, parsing happens at most once per content change — clone is ~11 ms at 100k.
**Files:** `crates/egui_commonmark/egui_commonmark_backend/src/pulldown.rs` (`ScrollableCache`), `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`show_scrollable` cache-population branch)

### Selection-preserving wheel hack needs ScrollAreaOutput
**Context:** The post-render `state.offset.y = …; state.store(...)` workaround documented above ("Scroll during selection requires post-render state modification") needs access to the underlying `ScrollAreaOutput`. After switching to `show_scrollable` the renderer owns the ScrollArea internally, so the caller can no longer build one.
**Fix:** `CommonMarkViewer::show_scrollable` now returns `egui::scroll_area::ScrollAreaOutput<()>`. New builder methods `pending_scroll_offset(Option<f32>)` and `scroll_source(ScrollSource)` let the caller drive the renderer-owned ScrollArea without giving up the post-render hook. `tab.scroll_offset` / `tab.last_viewport_height` / `tab.last_content_height` now read from `scroll_output.state.offset.y` / `inner_rect.height()` / `content_size.y`.
**Files:** `crates/egui_commonmark/egui_commonmark/src/lib.rs` (builder + return type), `src/main.rs` (`render_tab_content`)

### Outline scroll-to: virtualization breaks the corrective y-record loop
**Context:** Outline-click and search-jump used to be a two-stage scroll: frame 1 lands close (line-ratio estimate via `pending_scroll_offset`), frame 2 snaps precise (via `cache.active_search_y()` recorded during paint). Post-virtualization, blocks *off-screen* don't paint and don't record their y, so the corrective scroll can't fire for far targets.
**Fix idea (not yet implemented):** When `pending_scroll_offset` is set for a target outside the viewport, also clear `ScrollableCache::split_points` for that frame so the renderer does one full pass, populates positions, then the *next* frame snaps precisely. Same pattern the existing width-change invalidation uses.
**Files:** TODO if it shows up in practice — flagged in `docs/devlog/020-virtualize-large-docs.md`

### show_rows for the outline drops O(headers) per frame to O(visible)
**Context:** The outline `ScrollArea::show` iterated all of `tab.outline_headers` every frame; on a 100k-line doc with ~15k headers this dominated the right-panel cost.
**Fix:** Pre-compute `visible_indices: Vec<usize>` (skip collapsed-ancestor rows) once per frame, then use `ScrollArea::show_rows(ui, row_height, visible_indices.len(), |ui, range| ...)`. Outline rows have uniform height (fold indicator is 20px, others use `interact_size.y` which is also ~20px), so the row-height assumption holds.
**Gotcha:** MCP widget registration now only fires for actually-rendered rows. That's correct — off-screen widgets aren't testable via MCP anyway — but means a "register every fold indicator up-front" approach won't work if you ever need a registration of every row regardless of visibility.
**Files:** `src/main.rs` (`render_outline`)

### Lazy syntect: cache LayoutJob, key by content + theme + font size
**Context:** Syntect re-highlighted every code block on every paint (`egui_commonmark_backend/src/misc.rs:1209`). On a 100k-line doc with ~1500 code blocks this dominated first paint (~15 s of CPU) and continued to dominate during scroll.
**Fix:** `CommonMarkCache::syntax_layouts: HashMap<u64, LayoutJob>` keyed by hash of `(content, lang, theme_is_dark, mono_font_size, code_line_height)`. `CodeBlock::end` checks the cache before running syntect. Cache hit clones the stored LayoutJob (cheap — text + format ranges).
**Theme/zoom changes** key in new entries naturally because they're part of the key; old entries become dead weight until cache reset on file load. Bounded by unique-code-block count, typically <5 MB for real docs. LRU eviction is deferred until measured pathology.
**Files:** `crates/egui_commonmark/egui_commonmark_backend/src/misc.rs` (`CommonMarkCache::syntax_layouts`, `CodeBlock::end`)

### Vertical-wheel → horizontal table offset must be modifier-gated
**Context:** Issue #4 wanted wheel-over-table to scroll wide tables horizontally without grabbing the bottom scrollbar. The first shipped fix (`forward_wheel_to_horizontal_scroll`) redirected any hovered-table `smooth_scroll_delta.y` into `out.state.offset.x`. Issue #22 then reported that ordinary document scrolling nudged wide tables left/right whenever the cursor crossed one — the cost of the unconditional redirect was higher than the benefit. PR #23 removed the helper entirely.
**Current state (`forward_shift_wheel_to_horizontal_scroll`):** the helper is back but only acts when `ui.ctx().input(|i| i.modifiers.shift)` is true. Plain wheel always goes to the outer document scroller; Shift+wheel is the explicit opt-in for sideways table scrolling. Edge pass-through is preserved so Shift+wheel at the table's left/right edge keeps moving the page.
**Why the gate works:** Shift is unused elsewhere in the document scroller's wheel handling (Ctrl is taken by zoom), and aligns with the common browser convention of Shift+wheel for horizontal scroll. Users who don't know about it still get correct default behavior (#22); those who want #4's UX have a discoverable opt-in.
**Lesson:** when adding a "redirect" of an input that already has a default consumer, gate it on a modifier or another explicit signal — otherwise the redirect competes with the default for every event and the user perceives "input was eaten" or "the thing under my cursor moves when I didn't ask it to."
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`forward_shift_wheel_to_horizontal_scroll`, both table call sites)

### `delayed_events_list_item` must depth-track nested items
**Context:** v0.1.8 — `panicked at lib.rs:566 unreachable!()` in `List::start_item` when scrolling past a nested list in `show_scrollable`. Reproduced on `Recent-Changes.md` (7481 lines, 272 nested-list items).
**Root cause:** `delayed_events_list_item` at `egui_commonmark_backend/src/pulldown.rs:52-74` stopped at the **first** `TagEnd::Item`. For an outer item containing a nested sub-list, that's the *inner* item's close — the rest of the outer item (more inner items, the inner `TagEnd::List`, and the outer `TagEnd::Item`) leaked back to the outer `show()` event loop, where they were registered as `show_scrollable` split-points. On later paints the viewport-skip path landed iteration at one of those mid-list events; `CommonMarkViewerInternal::new()` produced a fresh `self.list` per frame, so `Tag::Item` hit `start_item` with an empty stack and panicked.
**Half-mitigation that didn't work:** the existing line 64-67 early-exit on inner `Tag::List` start; by the time it fired the renderer already had the inner level pushed and the leak was set up.
**Fix:** depth-track. Start depth at 1 (caller already consumed the outer `Tag::Item`), `Tag::Item` increments, `TagEnd::Item` decrements, return when depth ≤ 0:
```rust
let mut depth: i32 = 1;
let mut total_events = Vec::new();
for (_, (event, range)) in events {
    let is_item_start = matches!(&event, Event::Start(Tag::Item));
    let is_item_end   = matches!(&event, Event::End(TagEnd::Item));
    total_events.push((event, range));
    if is_item_start { depth += 1; }
    else if is_item_end { depth -= 1; if depth <= 0 { return total_events; } }
}
total_events  // EOF before balance — defensive, can't crash
```
**Apply to `delayed_events` (generic) too:** the same structural bug exists for nested blockquotes and nested def-list-definitions; it produces visible rendering glitches rather than panics (those containers don't have a stack that can be popped past zero). Deferred — track in `docs/devlog/027`.
**Files:** `crates/egui_commonmark/egui_commonmark_backend/src/pulldown.rs` (`delayed_events_list_item`)

### Math-feature parser-options mismatch between `show()` and `show_scrollable()`
**Context:** Same panic class as the nested-list bug above. Even after depth-tracking was applied, a second independent path produced the same `lib.rs:566 unreachable!()` crash.
**Root cause:** `show()` parses with `parser_options_math(options.math_fn.is_some() || cfg!(feature = "math"))` (parsers/pulldown.rs:461). `show_scrollable()` parses with `parser_options_math(options.math_fn.is_some())` (parsers/pulldown.rs:565). md-viewer enables the `math` cargo feature, so the two parses produced *different* event streams for any document containing `$…$` (currency `$0.02`, env vars `${ENV}`, regex). Split-points were registered with indices into `cache.cached_events` (with-math, from `show()`); the viewport-skip path consumes `sc.events` (without-math, from `show_scrollable()`). The two indices diverge on real docs; iteration jumps to an unrelated event — often `Tag::Item` with no matching `Tag::List` start — panic.
**Fix:** mirror `show()`'s derivation in `show_scrollable()`'s parse so the two event streams are identical:
```rust
let math_enabled = options.math_fn.is_some() || cfg!(feature = "math");
sc.events = Parser::new_ext(text, parser_options_math(math_enabled)).into_offset_iter()...;
```
**Diagnostic that would have caught it sooner:** dump `sc.events[ev_idx]` for each entry in `sc.split_points`. If the kind is `Tag::*` (Start) rather than `Event::End`, the two parses have diverged — they were populated against different event streams.
**Better long-term:** consolidate the two parses into one source so the divergence can't reappear. An `Arc<Vec<…>>` shared field on `CommonMarkCache` would also save the duplicated event storage (~tens of MB on large docs).
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`show_scrollable` cache-population branch)

### Split-points must be gated on renderer container state
**Context:** Defense-in-depth for the two bugs above. Even with `delayed_events_list_item` correctly depth-tracking and the math options aligned, if a future helper leaks events back to the outer loop or a similar discrepancy appears, `show_scrollable`'s viewport-skip path can still land iteration mid-container — and the renderer state isn't replayed.
**Root cause:** `is_block_end_tag` accepts `TagEnd::Item`, `TagEnd::List`, etc. unconditionally as block-end sites. The push gate didn't check whether the renderer (after `process_event` ran) was *currently inside* a list/table/blockquote.
**Fix:** AND in the container-state check **after** `process_event` runs (since that's where the state updates):
```rust
let is_block_end = matches!(&e, Event::End(end) if is_block_end_tag(end));
self.process_event(ui, &mut events, e, src_span, cache, options, max_width);
let safe_for_split = is_block_end
    && !self.list.is_inside_a_list()
    && !self.is_table
    && !self.is_blockquote;
if safe_for_split && let Some(source_id) = split_points_id { /* push */ }
```
**Trade-off:** very long top-level lists (10k+ items) lose their internal split-points and fall back to bootstrap-full-paint for every viewport. Slow but correct — strictly preferable to a `unreachable!()` panic. Long-list virtualization (allow outer-`TagEnd::List` with paired `Tag::List` replay) is tracked as a future improvement in `docs/devlog/027`.
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`show`'s split-point push block)

### Slice-clone, not Vec-clone-then-skip
**Context:** `show_scrollable`'s viewport-clipped branch was cloning the entire parsed-events Vec every paint, then iterator-`skip`ping all but ~100 visible events. On a 29 676-event doc that was ~1565 µs/frame of allocation churn — ~9.4 % of a 60 fps frame budget — and the dominant component of "scroll feels laggy".
**Misleading initial framing:** an `Arc<Vec<…>>` wrap looks attractive — Arc::clone is O(1). But iteration still needs to yield owned events to `process_event` (which takes `Event` by value), so each event gets cloned during iteration anyway. Total clone work is unchanged unless `process_event` is refactored to take `&Event` (which cascades through `start_tag`, `end_tag`, `event_text`, etc. — a large refactor).
**Better insight:** `first_event_index` and `last_event_index` are computed *inside* the `show_viewport` closure from the binary-searched split-points. Move the clone inside the closure too and clone only the slice:
```rust
let range_end = last_event_index.min(scroll_cache.events.len());
let events_range: Vec<_> = if first_event_index < range_end {
    scroll_cache.events[first_event_index..range_end].to_vec()
} else {
    Vec::new()
};
// re-attach original indices for downstream consumers:
let iter = events_range.into_iter().enumerate()
    .map(|(offset, ev)| (offset + first_event_index, ev));
```
**Result:** 1565 µs → 8 µs (~196× speedup) on Recent-Changes.md. The slice-clone still clones the visible events but skips the ~29 500 we'd have thrown away. NLL releases the `scroll_cache` borrow before `process_event` re-borrows the cache mutably inside the loop.
**Gotcha:** enumerate's counter starts at 0; the original `skip(first_event_index)` preserved absolute indices. Need `.map(|(offset, ev)| (offset + first_event_index, ev))` to keep the `if i == 0 { ... }` bootstrap-newline gate working.
**Lesson:** before reaching for a refactor (Arc, RefCell, splitting cache layout), look at whether existing code is doing work that gets immediately discarded. *"Clone-then-skip"* is a smell.
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`show_scrollable` viewport-clipped branch)

### `split_points` coord system: screen-y works, content-y conversion broke deep scrolling (REGRESSION TRACKING)
**Context:** Outline-click on a far heading lands the scroll target ~hundreds of px off. `pulldown.rs:486` and `:528` push split_points using `ui.next_widget_position()`, which returns **screen-y** coordinates. The skip-paint at `:701` and `:713` then compares those screen-y values against `viewport.min.y` (a content-y). At scroll=0 the two coord systems differ only by panel chrome height (~44px — small enough that outline-click is *slightly* imprecise but works); when the bootstrap is *forced* at non-zero scroll via `pending_scroll_offset.is_some()` clearing split_points, the new storage is at the scroll-shifted screen-y, off from content-y by the scroll amount. Subsequent skip-paints land at the wrong anchor.

**Attempted fix that regressed**: converting both `start_position` and `end_position` to content-y by subtracting `ui.min_rect().top()` at storage. This made outline-click land correctly. **BUT** it caused blank-content rendering at deep scroll positions in `full_width_content=true` mode on docs with mixed tables / code blocks / ASCII art. The cursor math after `allocate_space(first_end_position.to_vec2())` landed differently — apparently the consumer side wasn't fully content-y-aware, or there's a width-dependent layout interaction the fix didn't account for. Build at commit 4b13e25 had the content-y conversion; commit reverted in a follow-up. Lesson: the screen-y storage was a load-bearing pseudo-constant that other code paths depended on — full audit of all consumers needed before changing.

**Empirical evidence** (in-renderer DIAG_RENDER log on Recent-Changes.md):

| Path | Scroll | Title screen_y | Title content_y per fmla | Visible? |
|------|--------|----------------|--------------------------|----------|
| First bootstrap | 0 | 323 | 279 | ✓ mid-viewport |
| Wheel-scroll to 230 | 230 | 117 | 303 | ✓ visible |
| Forced bootstrap after outline-click | 229 | 118 | 303 | ✓ (this frame) |
| **Subsequent skip-paint** | 229 | **-67** | **118** | **off-screen above** |

Same scroll=229, but the skip-paint after an outline-click renders title 185px higher than the same scroll position via wheel — because the split_points used by the skip-paint were stored at scroll=229 with screen-y values.

**Pragmatic fix shipped (the one-line removal):**
The pending_scroll_offset invalidation block (lines 630-634 area) previously cleared `sc.split_points` in addition to `sc.page_size = None`. The page_size clear forces bootstrap to fire; the split_points clear was THERE to re-populate them with the new scroll's positions. Removing the split_points.clear() means:
- `page_size = None` still forces bootstrap → all events painted → header / search positions recorded.
- The push-site dedup-by-event-index in `show()`'s loop keeps the existing (good, scroll=0) split_points intact during the forced re-bootstrap.
- Subsequent skip-paints use the consistent original values, so partition_point and allocate_space math stays correct.

Verified: outline-click on far heading lands the heading at viewport top; scroll-up after outline-click no longer leaves blank space at top; search Ctrl+F + Enter still works (bootstrap still records active_search_y).

**Known remaining edge case** (acceptable, documented):
When the user changes layout_signature (Ctrl+/-, theme toggle, window resize) while scrolled deep, the *layout_signature change* branch (above) clears split_points (correctly — they're for the old layout). The next paint is a bootstrap at the user's current non-zero scroll, repopulating with new bad screen-y values. Outline-click after that scenario will be off until the user scrolls back to top and the original bootstrap-style split_points are re-established. Workaround: scroll to top before zooming/theming. Real fix requires the deeper coord-system work tracked below.

**Open future work:**
1. Audit ALL split_points consumers to confirm whether they interpret values as screen-y or content-y. The `allocate_space(first_end_position.to_vec2())` call at `:737` advances the cursor by Y; the right value depends on whether the cursor's reference frame uses screen-y or content-y semantics. Compile-time enforcement of the coord system (newtype wrapper around `f32`) would prevent future regressions.
2. Alternative outline-click fix: avoid forced bootstraps at non-zero scroll. E.g., scroll-to-zero, run bootstrap, then snap back to target scroll. Costs two extra frames but the split_points stay valid forever, including across layout_signature changes.
3. Alternative fix: store the bootstrap scroll alongside each split_point (4-tuple instead of 3-tuple), have skip-paint compensate via `end.y - bootstrap_scroll + viewport.scroll`. More complex but doesn't change semantic.

**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (the pending_scroll_offset invalidation block + the `show` split-point push at lines 486 + 528)

### `layout_signature` must quantize floats — sub-pixel jitter caused massive jitter on slow CPUs
**Context:** On T470 (i5-7200U, 2 cores), scrolling a 3800-line doc with embedded images felt extremely janky — visible stuttering, dropped frames throughout the first ~30 seconds. Measured with per-frame timing eprintln: **32 full bootstraps** fired during 30s of scrolling, each taking 100-800 ms on T470 (vs 5 ms for skip-paint). After ~30s, things settled and steady-state was sub-millisecond per frame.

**Root cause:** `compute_layout_signature` at `pulldown.rs:336` hashed `f32.to_bits()` of `ui.available_width()`, body font height, and monospace font height. Async image loading and font fallback resolution caused these floats to fluctuate by *sub-pixel amounts* every frame. Since `to_bits()` exposes every bit, even a 0.0001-px change flipped the hash, invalidating `split_points` + `page_size` → next paint routed to bootstrap branch → ~14k events re-iterated → 100-800 ms wasted. After all images finished decoding (~30s on slow hardware), the floats stopped fluctuating, signature stabilized, skip-paint resumed.

**Empirical evidence:** with quantized signature, same workload on same T470:
- Bootstraps during 30s scroll: 32 → **1**
- SLOW frames: 35 → **1** (just the unavoidable initial-load frame)
- Steady-state frame time: 100-800 ms → **0.4-1.1 ms**

**Fix:** quantize the float inputs before hashing — round width to nearest pixel (`(w.round() as i32).hash(&mut h)`), round font heights to 0.1 px (`((h*10.0).round() as i32).hash(&mut h)`). Real changes (window resize, zoom, theme toggle) shift the int bucket by enough to invalidate; sub-pixel async-load jitter stays in one bucket.

**Lesson:** `f32.to_bits()` is a footgun for cache-keys in immediate-mode UI. Float values participating in layout will fluctuate sub-pixel between paints whenever ANY async resource loads (images, fonts, possibly even GPU pipeline first-frame compilation). Quantize to a granularity that matches the layout's actual sensitivity.

**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`compute_layout_signature` at line 336)

### content_h-drift detection needs HYSTERESIS, not bucketing
**Context:** After the quantize-layout_signature fix above, scrolling still showed empty viewport edges. Diagnostic logs revealed split_points stored at the initial bootstrap had stale y values — async image loading after the initial paint had shifted blocks down (~1100 px by the time the user scrolled to mid-doc on a 14k-event changelog).

**First attempt that didn't work**: fold the previous frame's `content_size.y` into `layout_signature`, bucketed at 1024 px. The intent was that when async loads grew content significantly, a bucket boundary crossing would trigger ONE re-bootstrap with refreshed positions.

**Why it failed**: egui's `ScrollArea::show()` (bootstrap path) and `ScrollArea::show_viewport()` (skip-paint path) report `content_size.y` differing by ~44 px (panel chrome offset) for the same content. Any quantization that puts those two values in adjacent buckets enters a perpetual bootstrap loop: bootstrap reports 146916 → bucket A; next skip-paint reports 146960 → bucket B; bucket mismatch → invalidate → bootstrap reports 146916 → bucket A; ... forever. Measured 76 bootstraps in a 30-second test on T470.

**Fix**: replace bucketing with **absolute-drift hysteresis**. Track `bootstrap_content_h` on `ScrollableCache` — the content height captured at the most recent bootstrap. Each paint, compare `|last_content_h - bootstrap_content_h|` against `CONTENT_H_DRIFT_THRESHOLD = 1024.0`. Only when the drift EXCEEDS the threshold do we invalidate split_points. The 44-px egui oscillation falls under the threshold; real image-load growth (typically thousands of px on a content-rich doc) exceeds it once, triggers ONE re-bootstrap, then the new baseline stabilizes.

**Empirical evidence** (same T470 + same 7-step scroll test, comparing the two approaches):

| Approach | Bootstraps | Slow frames | Visual artifacts |
|---|---|---|---|
| Quantize-bucket only (round to 1024) | 76 | 77 | empty viewport edges |
| Quantize + abs-drift hysteresis | **2** | **3** | none across 7 screenshots |

The 2 bootstraps are the initial paint + one image-load convergence. After that, drift stays under threshold for the remainder of the session.

**General lesson**: when a fluctuating value drives cache invalidation, NEVER use round/floor bucketing if the fluctuation amplitude is comparable to the bucket size. Use a Schmitt-trigger-style threshold (only flip state if change exceeds a hysteresis band wider than the noise). Bucketing fails on values that sit exactly at a boundary; hysteresis is unconditionally stable.

**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (drift check in show_scrollable's invalidation block, `CONTENT_H_DRIFT_THRESHOLD` constant), `crates/egui_commonmark/egui_commonmark_backend/src/pulldown.rs` (`bootstrap_content_h` field on ScrollableCache)

### Skip-paint's `out.content_size.y` is unreliable — don't feed it to drift detection, AND clamp scroll against it
**Context:** With hysteresis-drift detection working for the 44-px egui-internal oscillation (entry above), a separate failure mode appeared on the Dockerfile Deep Dive doc (1574 lines, mixed code blocks + tables): panel flickered between blank and rendered with weird styling when scrolling deep. The hysteresis fix prevented the bucketing oscillation but didn't address THIS pattern.

**DIAG data** (619 bootstraps in 30 s of scroll, vs the previous fix's 2-3):
```
[SKIP_END] scroll=30911 content_h=62527 drift=27966    ← skip-paint reports double the real content_h
[BOOT]     scroll=30924 content_h=34539 first_sp_y=-30846  ← invalidation fires → bootstrap at non-zero scroll
[SKIP]     vp=[30924,31648] evt=[2503,2514]/2514 sp_y=[3612,0]  ← new split_points have garbage screen-y
[SKIP_END] scroll=2948 content_h=3672 drift=-30867     ← scroll bounced back, content_h collapsed
[BOOT]     scroll=2954 ...
... 600+ more ...
```

**Why content_size.y inflates to ~2×:** `show_viewport`'s closure does `ui.set_height(page_size.y)` (sets minimum) then `ui.allocate_space(first_end_position.to_vec2())` (advances cursor by `first_end_position.y` which is the stored split_point y, e.g. 30775 at deep scroll). The two together extend the inner UI's content size past `page_size.y`. egui reports `out.content_size.y = first_end_position.y + rendered_events_height + chrome ≈ 68246` when real content is ~34604.

**Two cascading failures from that one inflation:**
1. **Drift-detection death spiral.** The inflated 68246 vs the bootstrap 34608 produces drift ≈ +33000, way past the 1024 hysteresis threshold → invalidate split_points + page_size → re-bootstrap fires at the current non-zero scroll → bootstrap repopulates split_points with screen-y values that include the negative scroll offset (e.g., `first_sp_y=-30846`) → the next skip-paint's `partition_point` against these garbage y-coords picks events from the END of the doc instead of the middle → renders ~10 events when it should render ~100 → content_size.y now collapses to a few thousand → drift spikes the other way → re-bootstrap fires again. Steady-state of ~20 BOOT cycles/sec. User sees flicker + scrambled rendering.
2. **Scroll-overshoot blank panel.** Even without invalidation firing, the inflated content_size.y lets egui's ScrollArea accept scroll offsets up to `content_size.y - viewport_height ≈ 67500` — much larger than the real maximum (real_content - viewport ≈ 33880). User wheel-scrolls past 33880, the viewport falls into the "phantom" lower half of the inflated content where no events render → blank middle panel.

**Fix (two complementary one-liners; both are required):**
```rust
// Skip-paint exit:
// 1. Do NOT update last_content_h from out.content_size.y.
//    last_content_h is set only by bootstrap. Drift detection then
//    compares bootstrap-to-bootstrap content_h only, never false-positives
//    on skip-paint's inflated value.
// (the previous `scroll_cache(cache, &source_id).last_content_h = out.content_size.y;` line is removed)

// 2. Clamp scroll against the bootstrap-authoritative `page_size.y`.
let real_max_scroll = (page_size.y - out.inner_rect.height()).max(0.0);
if out.state.offset.y > real_max_scroll {
    let mut state = out.state;
    state.offset.y = real_max_scroll;
    state.store(ui.ctx(), out.id);
    ui.ctx().request_repaint();
}
```

**Empirical verification** (T470, Dockerfile Deep Dive, 350-wheel-deep scroll test):

| Build | BOOT count | Max content_h seen | Max scroll | Flicker |
|---|---|---|---|---|
| Pre-fix (hysteresis only) | **619** | 62527 | 34030 (overshoot) | Yes, ~20 cycles/sec |
| Fix 1 only (no drift signal from skip) | 3 | 68246 | 34030 (still overshoots) | No, but blank at scroll>33880 |
| Fix 1 + Fix 2 (clamp) | 3 | 68246 | 34022→clamped to 33880 | No |

**General lesson:** when one stored quantity is the authoritative source of truth (here: `page_size.y` from bootstrap), don't let derived/observed values from a different code path (here: `out.content_size.y` from skip-paint) feed back into anything that affects state. Treat the skip-paint output as a paint-only artifact, not a measurement.

**Why simpler alternatives don't work:**
- Removing `ui.allocate_space(first_end_position.to_vec2())` — events would render at content-y=0 in the inner UI, but the scroll position math expects them at first_end_position.y. Content would appear pinned to the top regardless of scroll.
- Replacing `set_height` with `set_max_size` — egui doesn't enforce strict max on Ui sizing; widgets that exceed allocation just overflow.
- Setting `state.offset.y` clamp BEFORE rendering — would clamp every frame even when content_size.y is accurate, breaking edge cases (e.g., during a real layout_signature change). Post-render clamp keeps the per-frame invariant correct.

**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (skip-paint exit block in `show_scrollable`)

### Skip-paint virtualization had unfixable-in-band layout bugs; disabled in favor of always-bootstrap
**Context:** Session 029 — user reported persistent flicker + wrong spacing + code-block indentation shifted right during scroll on T470, even after fixes B+C+D from the previous LESSONS entry. DIAG showed the renderer was in steady state (no death spiral, no clamp firing, drift = 0) yet the visual output was wrong.

**Root cause** (three compounding bugs):
1. **Orphaned `Start` events**: `show_scrollable`'s slice often started at an `End(SomeBlock)` event whose matching `Start` was outside the slice. That block then didn't render. `allocate_space` advanced the cursor past where the block *would have been*, so subsequent events still landed at their bootstrap-relative y — but the missing block left a visual hole. When the hole intersected the viewport: blank patches.
2. **`content_size.y` inflation** (already covered in prior LESSONS entry but persists): skip-paint reports content height up to 2× the real value. Even with the post-render clamp from session 028, ScrollArea's internal max-scroll math accepts overshoot before the clamp can fire next frame.
3. **Container state at slice boundary**: existing gate excluded list/table/blockquote split-points, but not code-block end split-points or def-lists. Slicing mid-container left renderer state empty for that block.

**Why patches B/C/D didn't fully resolve it:** they addressed symptoms (drift false-positives, scroll overshoot, horizontal allocate_space leak) without touching the underlying slice-orphan-Start mechanic. Visual rendering was correct at *steady-state* scroll positions where the slice happened to start at a clean boundary, but the boundary shifted with every wheel event during motion, hitting bad positions frequently enough to be perceived as continuous flicker.

**Fix shipped:** force `show_scrollable` to always run the bootstrap path (`ScrollArea::show(...)` rendering the full event stream in order). Skip-paint code preserved as `unreachable!` for future restoration. Performance measured on T470:

| Doc | Events | avg paint | FPS |
|-----|--------|-----------|-----|
| Tiny | 348 | 1.2 ms | 800+ |
| Dockerfile (1574 lines) | 2,514 | 5.7 ms | 175 |
| Medium synth | 3,453 | 11.4 ms | 88 |
| Large synth (7k lines) | 20,703 | 39.0 ms | 26 |
| Huge synth (36k lines) | 103,503 | 229 ms | 4 |

Smooth ≤ ~3k events; borderline ~5–10k; laggy ~20k; near-unusable at 100k. Acceptable for typical personal use; future skip-paint rewrite needed for docs over ~10k events. Design plan in `docs/devlog/030-skip-paint-investigation.md` — three options ranked by cost.

**General lesson:** when adding a "fast path" that depends on stateful renderer behavior, make sure the fast path either replays the state or starts only at strict state boundaries. The original `show_scrollable` did neither: it iterated events from an arbitrary index assuming the renderer was "fresh," but the renderer's state (heading accumulators, list-depth, code-block accumulation) requires Start events to make End events meaningful. The slicing assumption was wrong from the start; the bandaids stacked up across multiple LESSONS entries before this one didn't address it.

**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`show_scrollable` early return to bootstrap path), `docs/devlog/030-skip-paint-investigation.md`

### record_header_content_y_if_absent caused outline-click overshoot on disable-virtualization builds
**Context:** Right after the disable-virtualization fix above, outline-click landed each header progressively further below the viewport top — the deeper the header in the doc, the larger the overshoot. Scrolling itself was now perfect; only the click target was wrong.

**Root cause:** Headings record their content-y position via `cache.record_header_content_y_if_absent(&key, content_y)`. The `_if_absent` semantic pins the first paint's value. The FIRST paint happens before async font fallbacks (Noto family for non-ASCII glyphs) finish loading. Slightly different font metrics on first paint → slightly different text widths → slightly different line wrapping → cumulative content_y values diverge from the post-settle layout. Each heading's stored y is *less* than the actual y by an amount proportional to how many wrap-affected lines preceded it. Outline click reads the stored y and scrolls to `y - 50`; the actual heading is at the LATER, larger y, so the scroll target is too small and the heading appears below the viewport top.

**Fix:** change to `cache.record_header_content_y` (no `_if_absent`). Every paint refreshes the recorded y with the current layout. Once fonts settle, subsequent paints write the correct values. Click reads the current correct value.

**Why `_if_absent` existed:** the prior author intended "pin the first sighting so click targets are stable." Stable but stale. The right trade-off here is "always fresh," especially since the disable-virtualization fix means every paint records every heading anyway — cost is negligible.

**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`TagEnd::Heading` handler — line ~1587)

### `record_active_search_y_viewport` keeps firing for off-screen matches once virtualization is disabled
**Context:** Issue #19 — with the find bar open, wheel-scrolling away from the active match snapped back to it every frame. Esc to close the find bar restored scrolling. The corrective scroll block in `render_tab_content` (the one that uses `cache.active_search_y()` to snap precisely to the active match) was re-firing every frame.

**Root cause:** Two facts compounded.
1. `record_active_search_y_viewport` is called by the renderer for every Active highlight segment it walks past (`crates/egui_commonmark/.../pulldown.rs:1372`). egui's clip rect culls *painting*, not widget layout — the renderer's event loop runs `record_*` for off-screen matches just like on-screen ones. So `cache.active_search_y()` returns a fresh, accurate value every frame for as long as the active match exists.
2. With virtualization disabled (commit `21d43c5`), the renderer walks the entire event stream every paint. The pre-virtualization version only walked the viewport slice, so off-screen matches didn't re-record — `active_search_y` went stale once you scrolled past, and the corrective block effectively self-disabled.
3. The corrective block in `render_tab_content` had no guard for "user just scrolled" — it only checked `if let Some(actual_y) = tab.cache.active_search_y()`. Permanent snap-back loop as soon as the user's wheel moved the match out of viewport.

**Fix:** one-shot `correct_active_search_pending: bool` on `Tab`. Set by `scroll_to_active_match` (called from `jump_match` / `maybe_rebuild_search` / search-open). Gates the corrective block, which clears the flag after running once. Two-stage scroll still converges in 1–2 frames after a jump; subsequent frames have flag=false so the block no-ops on user wheel.

**Pre-existing pattern this mirrors:** the outline-click corrective uses `tab.pending_header_click_key.take()` — the `Option<String>` is consumed on first read for the same one-shot semantics. The search-corrective could have used `Option<()>` but a named `bool` reads better.

**Empirical evidence** (Xvfb + `xdotool` 500 wheel-down events on `/tmp/search-repro.md`):

| Build | Net scroll (500 wheel-downs) | FIRING events |
|---|---|---|
| Pre-fix (`active_search_y` always fresh) | 16 px | 213 |
| Fix (one-shot guard) | ~2 800 px | 0 (after initial paint) |

**General lesson:** when a "side path" caches values mid-render for downstream consumers (here, `active_search_y` for the corrective scroll), audit ALL the code paths that *populate* the cache when you change rendering behavior. The disable-virtualization fix in `21d43c5` was correctness-first for paint, but it silently changed the semantics of `active_search_y` from "valid while visible" to "valid forever," and the corrective block was implicitly assuming the old semantics.

**Files:** `src/main.rs` (`Tab.correct_active_search_pending`, `scroll_to_active_match`, `render_tab_content` corrective block), `docs/devlog/031-search-scroll-lock.md`

### List-item block widgets need explicit wrapped-row boundaries
**Context:** Issue #44 — fenced code blocks inside unordered, ordered, and nested list items overlapped text immediately before or after the block.

**Root cause:** List-item content renders in an egui horizontal wrapped row. A fenced code block is a block widget, but entering and leaving `Tag::CodeBlock` did not end that active row. Adjacent text and the block could therefore occupy the same layout row.

**Fix:** Call `ui.end_row()` immediately before starting and after finishing a code block, gated by `self.list.is_inside_a_list()`. The gate leaves top-level code-block layout unchanged.

**Fixture gotcha:** Markdown indentation does not guarantee a sibling block. This ending:
```markdown
    NESTED_AFTER
  OUTER_AFTER
```
parses as one nested paragraph with a `SoftBreak`, so same-row placement is valid. To test return to outer list depth, use an actual outer sibling item:
```markdown
    NESTED_AFTER
- OUTER_AFTER
```
Do not change renderer soft-break behavior to satisfy a fixture whose syntax expresses one paragraph.

**Testing technique:** Inspect final-pass painted `Shape::Text` rectangles and assert strict top-to-bottom ordering. Response height alone can miss overlap. Keep one `CommonMarkCache` across render passes so tests match production cache lifetime and egui layout can settle.

**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs`, `crates/egui_commonmark/egui_commonmark/tests/wrapping.rs`, `docs/devlog/043-list-code-block-layout.md`
