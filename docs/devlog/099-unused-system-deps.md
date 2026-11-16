# Chore: drop unused system dependencies

**Status:** ✅ Complete
**Branch:** `chore/deps-cleanup`
**Date:** 2026-11-16

## Summary

The AUR packages, the CI/release apt lines and the snap all asked for
openssl, gtk3 and the dbus headers. The app uses none of them at build time
and only libdbus at runtime.

| library | used? | evidence |
|---|---|---|
| openssl | no | no openssl/native-tls crate in `Cargo.lock`, no `libssl` string in the binary |
| gtk3 / glib | no | `rfd` 0.17 is built without its `gtk3` feature (defaults: `xdg-portal`, `wayland`); no gtk/glib crates or strings |
| libdbus | **runtime only** | rfd's portal backend dlopens `libdbus-1.so.3` (`rfd-0.17.2/src/backend/xdg_desktop_portal/portal/ffi.rs`); nothing links it, so `libdbus-1-dev` is not needed |

`ldd target/release/md-viewer` lists only libc, libm and libgcc_s; the rest
is dlopened, which is why the old PKGBUILD comment had guessed instead of
measured.

## Changes

- `aur/PKGBUILD`, `aur-bin/PKGBUILD`: `depends` loses `openssl` and `gtk3`.
  `dbus` stays (Arch's `dbus` provides `libdbus-1.so.3`).
- `ci.yml`, `release.yml`: `libssl-dev`, `libgtk-3-dev`, `libdbus-1-dev`
  removed from all six apt lines.
- `snap/snapcraft.yaml`: same three build-packages removed; `libgtk-3-0` and
  `libglib2.0-0` no longer staged.
- `scripts/snap-ci.sh`: after the build, `check_contents` fails unless the
  snap holds the libraries the app dlopens (`libxkbcommon-x11.so.0`,
  `libX11.so.6`, `libfontconfig.so.1`), the #55 blind spot.

## Key Discoveries

### libdbus comes from the base, not the snap

The published snap (rev 31) does not contain `libdbus-1.so.3`, even with gtk
staged. The core22 base does (`unsquashfs -l core22.snap`), and a strict snap
sees its base's `/usr/lib`, so the portal dialog works. A first draft staged
`libdbus-1-3` and listed it in the guard; run against rev 31 the guard failed
on a snap that works. It is not listed now.

### The guard was observed both ways

Against rev 31: exit 0 with all three libraries found. With a nonexistent
library added to the list: exit 1 and an `::error::` line naming it.
