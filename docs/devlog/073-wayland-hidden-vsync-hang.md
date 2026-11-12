# Fix: "Application Not Responding" when a hidden window's file changes

**Status:** ✅ Complete
**Branch:** `fix/wayland-hidden-vsync-hang`
**Date:** 2026-10-22

## Summary

On Hyprland, md-viewer sitting on a non-visible workspace froze for as long as
it stayed hidden whenever a file it had open changed on disk. After five missed
`xdg_wm_base` pings (~8 s) Hyprland showed its ANR dialog. Driver vsync is now
off on Wayland.

## Root cause

The file watcher reloads the tab and requests a repaint. egui paints and calls
`eglSwapBuffers`. With swap interval 1, Mesa's Wayland EGL blocks inside the
swap until the compositor's frame callback for the previous frame arrives.
Hyprland only sends frame callbacks to visible workspaces, so the main thread
sat in `wl_display_dispatch_queue` indefinitely and could not answer pings.

Captured with gdb during the freeze:

```
#1 ppoll
#3 wl_display_dispatch_queue_timeout   libwayland-client
#5..#8                                 libEGL_mesa
```

## Fix

`NativeOptions::vsync` is `false` when `WAYLAND_DISPLAY` or `WAYLAND_SOCKET` is
set (after the dead-socket X11 fallback has run).

Nothing is lost on Wayland: the compositor latches complete buffers, so there
is no tearing without the tearing-control protocol, and winit 0.30 already
withholds `RedrawRequested` while a frame callback is pending, so pacing moves
from a blocking wait to a non-blocking one. X11 keeps vsync because the driver
wait is its only pacing.

## Verification

Hidden test instance on `special:anrtest`, then append a line to its file:

| build | longest main-thread stall | ANR dialog |
|---|---|---|
| 0.3.0 stock | 8 s | yes |
| stock + `vblank_mode=0` | 0 s | no |
| this branch | 0 s | no |

### Maintainer re-check on KWin + NVIDIA (2026-11-12)

The freeze is not Hyprland- or Mesa-specific. Nested `kwin_wayland --virtual`
under a private `dbus-run-session`, window minimized via a KWin script, one
line appended to the open file, then the app (run as a gdb child) interrupted
3 s later:

| build | idle CPU (10 s) | main thread 3 s after the hidden reload |
|---|---|---|
| main | 0.0 % | `eglSwapBuffers` → `libEGL_nvidia` → `wl_display_dispatch_queue` (stuck) |
| this branch | 0.0 % | winit `calloop` event loop (responsive) |

Harness traps: Yama `ptrace_scope=1` blocks `gdb -p`, so run the app *under*
gdb; and the nested KWin D-Bus-activates `knighttimed` on the private bus,
which inherits stdout and keeps a `| tail` pipe open forever.

## Key Discoveries

- The window must actually be hidden for the test to mean anything. Two early
  runs used legacy `hyprctl dispatch` syntax, which this Lua-configured Hyprland
  rejects, so the window stayed visible and "no stall" proved nothing.
- `vblank_mode=0` is honoured by Mesa's Wayland EGL platform and is a usable
  workaround for installed builds.
