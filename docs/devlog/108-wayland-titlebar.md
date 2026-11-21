# Fix: double-click on the Wayland title bar maximizes (#266)

**Status:** ✅ Complete
**Branch:** `fix/wayland-titlebar-double-click`
**Date:** 2026-11-21
**Lines Changed:** `Cargo.toml` (Linux-only `winit` dependency), `Cargo.lock` (4 packages added), `src/main.rs` (one test)

## Summary

Issue #266: on GNOME, double-clicking md-viewer's title bar did not maximize or restore the window; only the maximize button did.

GNOME's compositor (mutter) leaves window decorations to the client on Wayland, so winit draws the title bar. md-viewer now builds winit with its Adwaita frame, which handles the double-click, instead of the minimal fallback frame.

## Root cause

winit picks the frame at compile time (`winit-0.30.13/src/platform_impl/linux/wayland/window/state.rs`):

```rust
#[cfg(feature = "sctk-adwaita")]
pub type WinitFrame = sctk_adwaita::AdwaitaFrame<WinitState>;
#[cfg(not(feature = "sctk-adwaita"))]
pub type WinitFrame = sctk::shell::xdg::fallback_frame::FallbackFrame<WinitState>;
```

- `FallbackFrame::on_click` (smithay-client-toolkit 0.19.2) takes the click timestamp as `_timestamp` and ignores it: a press on the header is always `FrameAction::Move`. It draws no title text either.
- `AdwaitaFrame` (sctk-adwaita 0.10.1, `pointer.rs`) turns two header presses within 400 ms into `Maximize` or `UnMaximize`.

eframe enables the Adwaita frame only through `winit/default`, part of eframe's own default features. md-viewer has declared `eframe = { default-features = false, ... }` since its first commit, and `sctk-adwaita` was never in `Cargo.lock`. Every release so far had the fallback frame on GNOME/Wayland.

Nothing in md-viewer explains the report's "no longer": winit (0.30.13), the eframe features and the snap's plugs are the same since 0.2.0. Before, the reporter probably used an X11 session or XWayland, where mutter draws the frame itself.

## Fix

```toml
[target.'cfg(target_os = "linux")'.dependencies]
winit = { version = "0.30", default-features = false, features = ["wayland-csd-adwaita"] }
```

Cargo feature unification applies the feature to the winit eframe uses (`cargo tree -i sctk-adwaita` shows it under winit 0.30.13 → eframe). Nothing else changes:

- `Cargo.lock` only gains `sctk-adwaita`, `ab_glyph`, `ab_glyph_rasterizer` and `owned_ttf_parser`;
- the release binary grows by 85,632 bytes (50,423,768 → 50,509,400).

Compositors that draw decorations themselves (KDE), or none at all (Hyprland, Sway), and X11 sessions are unaffected. They never show winit's frame.

## Verification

- **Test:** `wayland_title_bar_uses_the_adwaita_frame` checks `Cargo.lock` for `sctk-adwaita`. It was red before the `Cargo.toml` change ("sctk-adwaita is not in the dependency graph") and green after.
- **E2E, nested weston (15.0.1, X11 backend) on Xvfb.** weston, like mutter, leaves decorations to the client. md-viewer ran as a Wayland client (`WAYLAND_DISPLAY` set, `DISPLAY` unset), driven with xdotool. The results were the same for both release builds:

| | main (fallback frame) | branch (Adwaita frame) |
|---|---|---|
| title text in the bar | none | "README.md - Markdown Viewer" |
| double-click on the bar | window unchanged | maximized |
| second double-click | — | restored |
| drag on the bar | — | moves the window |
| maximize button | maximizes (control: input reaches the frame) | works |

## Gotchas

- **Wayland socket paths are limited to 108 bytes.** weston failed with "socket path … exceeds 108 bytes" in the session scratchpad. A short `XDG_RUNTIME_DIR` (`/run/user/1000/mdv266`) fixed it.
- **A no-op double-click proves nothing on its own.** The maximize-button click on the main build was the control showing that xdotool input reached the frame.
- **Shut down weston before its X server.** Killing Xvfb first left weston running with defunct children, so it had to be stopped by PID.
