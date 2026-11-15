# Docs: README checked against v0.3.6, screenshots retaken

**Status:** ✅ Complete
**Branch:** `docs/readme-refresh`
**Date:** 2026-11-15

## Summary

Every claim in the README was checked against the code at 711a87a (v0.3.6),
the v0.3.6 release assets and the Snap Store API. The screenshots were retaken
with a fresh profile on Xvfb against a neutral demo folder. The old math
screenshot showed a private research document. Devlog 093 (`fix/code-copy-icon`)
fixed the code-block copy button, and the screenshots were taken from a build
with that fix.

## What changed in the text

- **Wrong:** binary ~35 MB / snap ~7 MB. Measured: the Linux binary is 50.4 MB
  (23 MB `.tar.gz`), and the stable snap download is 90 MB. Also fixed in
  `CLAUDE.md` and `docs/TARGET_METRICS.md`.
- **Wrong:** "Platform: Linux" and "Cross-Platform: X11 and Wayland". Releases
  also ship Windows x86_64 and macOS arm64.
- **Wrong:** Arch build deps listed `openssl gtk3 dbus`. `Cargo.lock` has no
  `openssl-sys`, `gtk-sys` or `libdbus-sys`, D-Bus goes through pure-Rust
  zbus, and fontconfig is loaded at runtime. The release binary's `ldd` lists
  only libc, libm and libgcc_s. Also fixed in
  `.claude/rules/system-dependencies.md`. The same three are still in both
  PKGBUILDs, the CI/release apt lines and the snap build-packages; that is a
  packaging change for its own PR.
- Ctrl+S moved from Navigation to File Operations; Ctrl+E (Preview/Edit) added,
  matching `docs/KEYBOARD_SHORTCUTS.md`.
- Added: Edit mode, right-to-left text (#108), View → Show Tab Bar (#208),
  explorer sort orders, the `md-viewer-bin` AUR package, and why
  `cargo install` builds have no bidi layout.
- "60 FPS on 100k+ line docs" was not measured for this version, so it now says
  what the renderer does (paints only the visible part).
- `scripts/install.sh` header said Intel Macs are supported; the script exits
  for them.

## Screenshots

All 1280×800, dark unless noted, explorer rooted at `/tmp/mdv-demo`:
`math-rendering`, `syntax-highlighting`, `tables`, `frontmatter`, `search`,
`view-menu`, `dark-mode`, `light-mode`, plus a new `rtl.png`.

## Key Discoveries

### Bugs found while capturing (not fixed here)

- **Copy button glyph** — fixed in devlog 093.
- **Table cells mixing text and inline math reserve far too much height.**
  `table_cell_height` adds every formula's height on top of the text height
  when a cell has both (deliberately conservative, see its comment). A header
  row `| model | $\chi^2$ (60 pts) | $\gamma$ (s$^{-1}$) |` came out about four
  lines tall. A cell holding only a formula gets `exact + 0.5 line`, also
  visibly taller. The demo uses Unicode in table cells for that reason.
- **Inline code in a narrow fitted column breaks mid-identifier** (`budget_` /
  `mb`). The column fitter does not treat an inline-code span as unbreakable.
- **Startup time could not be confirmed.** Launch-to-visible-window on Xvfb measured
  1.3–4 s, but with a host load average of ~36 from unrelated work, so it neither
  confirms nor refutes the README's "< 200 ms". The line was removed until it is
  measured on an idle machine.

### Capture harness

- No window manager on Xvfb: wheel events over the document did not scroll.
  Clicking an outline entry did. Menus open with one click if the pointer does
  not leave them.
- `xdotool search --name` sometimes returned nothing right after a click while
  the window existed; the shot step retries.
- Typing into the find bar works after `xdotool windowfocus --sync`.
