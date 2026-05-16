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

### Nested horizontal ScrollArea doesn't auto-redirect vertical wheel
**Context:** Issue #4 — wide markdown/HTML tables are wrapped in `egui::ScrollArea::horizontal()` inside the outer document `ScrollArea::vertical()`. Mouse wheel over the table body scrolled the page, never the table. Users could only scroll a wide table sideways by dragging the bottom scrollbar.
**Root cause:** egui 0.33's `ScrollArea::horizontal()` only consumes the X component of `smooth_scroll_delta` during its `.show()`. Plain mouse wheel emits Y delta only, so the inner area sees nothing; the unconsumed Y delta then reaches the outer vertical area and scrolls the page. Shift+wheel has the same broken behavior — egui doesn't auto-convert Y→X for nested horizontal areas.
**Fix:** After the nested `ScrollArea::horizontal().show(...)` returns, check `ui.rect_contains_pointer(out.inner_rect)`. If hovered AND the area still has room in the wheel direction, redirect Y delta into `out.state.offset.x`, `state.store(ctx, out.id)`, then zero `i.smooth_scroll_delta.y` so the outer area doesn't double-consume.
**Edge pass-through:** If `offset.x == 0` and wheel-up, OR `offset.x >= max_x` and wheel-down, return early without touching the delta. The outer area then scrolls normally — avoids "stuck table swallows my wheel" feel.
```rust
fn forward_wheel_to_horizontal_scroll<R>(
    ui: &Ui,
    out: &mut egui::containers::scroll_area::ScrollAreaOutput<R>,
) {
    if !ui.rect_contains_pointer(out.inner_rect) { return; }
    let dy = ui.ctx().input(|i| i.smooth_scroll_delta.y);
    if dy.abs() < 0.1 { return; }
    let max_x = (out.content_size.x - out.inner_rect.width()).max(0.0);
    if max_x <= 0.0 { return; }
    let at_left  = out.state.offset.x <= 0.0   && dy > 0.0;
    let at_right = out.state.offset.x >= max_x && dy < 0.0;
    if at_left || at_right { return; }
    out.state.offset.x = (out.state.offset.x - dy).clamp(0.0, max_x);
    out.state.store(ui.ctx(), out.id);
    ui.ctx().input_mut(|i| i.smooth_scroll_delta.y = 0.0);
    ui.ctx().request_repaint();
}
```
**Why `State` is `Copy`:** egui's `ScrollArea::State` is `#[derive(Copy)]`, so `out.state.store(ctx, id)` copies and stores — the caller's `out.state` stays accessible after.
**Files:** `crates/egui_commonmark/egui_commonmark/src/parsers/pulldown.rs` (`forward_wheel_to_horizontal_scroll`, both table call sites)
