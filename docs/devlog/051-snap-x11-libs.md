# Fix: Snap crashes on X11 sessions (missing dlopen'd libs, XKB data, DRI path)

**Status:** 🚧 In Progress
**Branch:** `fix/snap-x11-libs`
**Date:** 2026-07-23
**Lines Changed:** +8 / -1 in `snap/snapcraft.yaml` (plus release version bumps)

## Summary

Issue #55 (reported and fully root-caused by @HartmutLeister): the strictly-confined
snap aborts on startup on any **X11** session (`Library libxkbcommon-x11.so could not
be loaded` → after that, `XKBNotFound` → after that, `GLXBadFBConfig`). Wayland
sessions were unaffected, which is why no prior release caught it — all three
failures live on the X11-only code path.

Three independent gaps, all fixed in `snap/snapcraft.yaml`:

1. **dlopen'd X11 libraries not staged.** winit's X11 backend loads
   `libxkbcommon-x11.so.0` (and through it `libxcb-xkb1`/`libX11`/`libXau`/`libXdmcp`)
   via `dlopen` at runtime instead of dynamic linking. snapcraft's automatic
   dependency staging only follows link-time dependencies, so none of these were in
   the snap. `stage-packages` gains `libxkbcommon-x11-0` + `libx11-6`.
2. **XKB keymap data missing.** Neither the snap nor the core22 base contains
   `/usr/share/X11/xkb`; winit panics `XKBNotFound` once the libraries exist.
   `stage-packages` gains `xkb-data`, and `XKB_CONFIG_ROOT=$SNAP/usr/share/X11/xkb`
   points xkbcommon at it. `libx11-data` is also staged to provide Compose files
   (silences a cosmetic `couldn't find a Compose file for locale` warning).
3. **Mesa DRI drivers unreachable.** The DRI drivers *are* staged (dep of libgl1),
   but Mesa's loader searches its compiled-in absolute path
   `/usr/lib/x86_64-linux-gnu/dri`, which inside the mount namespace is the (empty)
   core22 base → GLX `BadConfig`. Fixed with
   `LIBGL_DRIVERS_PATH=$SNAP/usr/lib/x86_64-linux-gnu/dri`.

The exact combination was verified end-to-end by the reporter on Ubuntu 24.04/X11
(copying matching libs + XKB data into `$SNAP_USER_DATA` inside the confinement via
`snap run --shell`). The alternative — `extensions: [gnome]` — would also work but
drags in the whole gnome-42-2204 content snap; the targeted staging keeps the snap
lean (~7 MB compressed).

## Features

- [x] Stage `libxkbcommon-x11-0`, `libx11-6`, `libx11-data`, `xkb-data`
- [x] Set `XKB_CONFIG_ROOT` and `LIBGL_DRIVERS_PATH` in app environment
- [ ] Release v0.1.15 so the fixed snap revision ships
- [ ] Verify shipped snap contents (unsquashfs of the store revision)
- [ ] Confirm with reporter on real Ubuntu/X11 hardware, close #55

## Key Discoveries

### snapcraft only stages link-time dependencies — dlopen is invisible to it

`stage-packages` dependency resolution walks the ELF `DT_NEEDED` graph of staged
binaries. Libraries the app opens with `dlopen()` at runtime (winit does this for
most of its X11 stack) are never discovered. Any dlopen'd library must be listed
explicitly. This is the same class of problem for every winit/egui app packaged as
a strict snap without a desktop extension.

### Staged files ≠ reachable files: compiled-in absolute paths resolve against the base

Mesa's DRI drivers were *in* the snap all along, at
`$SNAP/usr/lib/x86_64-linux-gnu/dri/`. The loader still failed because it searches
the absolute path baked in at Mesa build time, and inside the snap's mount
namespace `/usr/lib/...` is the core22 base, not `$SNAP`. Anything with a
compiled-in search path (GL drivers, XKB data) needs its env-var override
(`LIBGL_DRIVERS_PATH`, `XKB_CONFIG_ROOT`) pointed into `$SNAP`.

### Wayland-only testing hides the entire X11 dlopen chain

All three failures sat on the X11-only path. Local dev (Wayland) and presumably
every prior user report exercised Wayland. When packaging a toolkit that has
per-display-server backends, a smoke test on *each* backend
(`WAYLAND_DISPLAY= DISPLAY=:0`-style forcing) is the only way to catch this class.

## Architecture

No code changes. `snap/snapcraft.yaml` only:

- `parts.md-viewer.stage-packages`: + `libxkbcommon-x11-0`, `libx11-6`,
  `libx11-data`, `xkb-data`
- `apps.md-viewer.environment`: + `XKB_CONFIG_ROOT`, `LIBGL_DRIVERS_PATH`

## Testing Notes

- Cannot build the snap locally (Arch host; `--destructive-mode` is banned per
  LESSONS "Never snapcraft --destructive-mode"). Verification path:
  1. CI `publish-snap` (LXD, core22) builds and publishes the revision
  2. Download the published .snap from the store, `unsquashfs -l`: confirm
     `libxkbcommon-x11.so.0`, `usr/share/X11/xkb/`, and the env vars in
     `meta/snap.yaml`
  3. Reporter confirms on their Ubuntu 24.04 X11 machine (exact repro env)
- Residual risk: `LIBGL_DRIVERS_PATH` also applies on Wayland, pointing EGL at the
  staged core22 Mesa drivers. Staged libGL + staged drivers are version-consistent
  (same core22 archive), so this should be neutral; if a Wayland-snap regression
  report appears, this is the first suspect.

## Future Improvements

- [ ] Consider a CI smoke test that boots the built snap under Xvfb in an Ubuntu
      container (would have caught all three gaps before release)
