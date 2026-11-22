# 113: Snap X11 startup (#272)

**Branch:** `fix/snap-x11-libs`
**Date:** 2026-11-22
**Status:** Complete

## Summary

Since 0.3.9 the snap aborted at startup in every X11 session:
`XNotSupported(LibraryOpenError … libXcursor.so.1: cannot open shared object file)`.
Commit f6366dd (0.3.9) dropped `libgtk-3-0` and `libglib2.0-0` from
`stage-packages` because the app never loads them. GTK had been pulling in,
as its own dependencies, the X11 libraries winit **dlopens** in
`XConnection::new` (winit 0.30.13, `platform_impl/linux/x11/xdisplay.rs`:
Xlib, Xcursor, Xlib-xcb, XInput2). snapcraft stages link-time dependencies
only, so they left with GTK. Wayland sessions were unaffected. Same class as
#55 (devlog 051).

## Reproduction without Ubuntu

- Downloaded stable rev 33 (0.3.10, broken), rev 31 (0.3.8, last good) and the
  core22 base from the store API, and `unsquashfs`ed them.
- Ran each snap's `bin/md-viewer` under `bwrap` with core22 bound as `/`, the
  snap at `/snap/md-viewer/x1`, only the X11 socket and host fonts passed
  through, and the environment from `meta/snap.yaml`
  (`LD_LIBRARY_PATH=$SNAP/usr/lib:$SNAP/usr/lib/x86_64-linux-gnu`, `XKB_CONFIG_ROOT`,
  `LIBGL_DRIVERS_PATH`) on a private Xvfb.
- Rev 33 died with the exact error from the issue. Rev 31 rendered the test
  document.
- Rev 33 lacked libXcursor, libXrender, libXi and libXrandr (rev 31 had them).
  Copying libraries from rev 31 into rev 33 one at a time showed what startup
  needs: libXcursor.so.1, then libXrender.so.1 (Xcursor's dependency), then
  libXi.so.6. With those three, rev 33 renders. libX11-xcb, libXfixes and
  libXext were already staged through Mesa.

## Fix

- `snap/snapcraft.yaml` stages `libx11-xcb1`, `libxcursor1`, `libxrender1`,
  `libxi6` and `libxrandr2` explicitly, with a comment naming #272.
- `scripts/snap-ci.sh::check_contents` now requires every library winit's X11
  backend dlopens and their links the core22 base lacks: `libX11-xcb.so.1`,
  `libXcursor.so.1`, `libXrender.so.1`, `libXfixes.so.3`, `libXi.so.6`,
  `libXext.so.6`. Run against the published snaps it fails on rev 33 (naming
  exactly libXcursor, libXrender and libXi) and passes on rev 31.
- `release.yml`: a dry run uploads the snap it built (`snap-dry-run` artifact,
  3 days), so it can be started in the sandbox before a tag.

## Key Discoveries

- The contents check existed since #55 but listed only three libraries, so it
  stayed green: a guard that checks a list protects only what is on the list.
  Removing a package from `stage-packages` can drop anything it pulled in
  transitively; nothing fails until the app dlopens it.
- The bwrap sandbox is a faithful enough stand-in for strict confinement to
  reproduce library-visibility bugs: core22 has no libX* at all, so anything
  missing from the snap cannot be found.

## Future Improvements

- Run the bwrap sandbox against the dry run's snap in CI itself (needs Xvfb and
  the core22 snap on the runner).
