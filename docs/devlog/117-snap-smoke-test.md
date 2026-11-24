# 117: Start the snap before uploading it

**Branch:** `ci/snap-smoke`
**Date:** 2026-11-24
**Status:** Complete

## Summary

The release workflow built the snap, checked its file list
(`scripts/snap-ci.sh` `check_contents`) and uploaded it, without ever starting
it. Two regressions reached snap users that way, and both built cleanly:

- **#272 (0.3.9–0.3.11):** md-viewer aborted on X11 because the snap no longer
  staged libraries winit dlopens. `check_contents` listed only three libraries
  and stayed green.
- **#279 (up to 0.3.11):** md-viewer aborted when fontconfig found no fonts
  (fontique 0.11.1 unwrapped `NoMatch`).

`scripts/snap-smoke.sh <file.snap>` now starts the built snap in its base
before any upload. It runs in `release.yml`'s snap job and in
`snap-publish.yml`.

## How it works

1. Unpack the snap; read `name` and `base` from `meta/snap.yaml`.
2. Get the base: `SNAP_SMOKE_BASE` (a directory or a `.snap`), otherwise
   download it from the Snap Store API (stable, amd64), retried.
3. Generate a launcher from `meta/snap.yaml`: the global `environment`, then
   the app's, each value expanded by the base's own `/bin/sh` as snapd does
   (`$SNAP`, `${LD_LIBRARY_PATH:+…}`). A snap that drops one of those variables
   fails the test the way it fails for users.
4. Start Xvfb on a free display from :90 to :119.
5. Run the launcher under bubblewrap with the base as `/`, the snap at
   `/snap/<name>/x1`, only the X11 socket shared, a private home, twice: with
   the host's `/usr/share/fonts` and with an empty one.
6. A run passes if md-viewer is still running after `SNAP_SMOKE_WAIT` (10 s),
   its log has no `panicked`, and the screenshot has at least 32 colours (an
   empty X root has 1; the rendered window about 650).

Under `CI=true` it installs bubblewrap, Xvfb, ImageMagick, squashfs-tools and
PyYAML, and sets `kernel.apparmor_restrict_unprivileged_userns=0`: Ubuntu
24.04 denies unprivileged user namespaces by default, and bwrap needs one.
`SNAP_SMOKE_OUT` keeps each run's log and screenshot; CI uploads them as the
`snap-smoke` artifact when the step fails.

## Proof, both ways (local, against published revisions)

| snap | with fonts | no fonts | result |
|---|---|---|---|
| rev 33 (0.3.10) | exits: `XNotSupported(LibraryOpenError …)` (#272) | exits, same | FAIL |
| rev 34 (0.3.11) | renders | exits: `fontique-0.11.1 … fontconfig.rs:685 … NoMatch` (#279) | FAIL |
| rev 35 (0.3.12) | renders (656 colours) | renders (680 colours) | PASS |

The rev 35 screenshot shows the test document with its heading, paragraph and
highlighted code block.

**On GitHub's runner** (release dry run 45292688040 on this branch): the step
passed on the branch's own snap (710 and 680 colours, 193 s including the tool
install and the core22 download), and a temporary second step running it on
rev 33 failed it with #272's `XNotSupported(LibraryOpenError …)`, not with a
sandbox error. So the AppArmor workaround works there and the check goes red
on a real regression. The temporary step was dropped before the PR.

## Key Discoveries

- **unsquashfs exits 2 as a normal user on a base snap:** it cannot create
  `/dev/null` and friends or write `security.capability` xattrs. bwrap mounts
  its own `/dev`, so exit 2 is accepted (with `-no-xattrs`) and exit 1 or a
  missing key file (`meta/snap.yaml`, `bin/sh`) fails.
- **`retry curl … | parser` mixes the retry messages into the data.** The first
  local run hit a TLS error from the store, and the retry's warning went into
  the JSON parser. Fetch into a file, then parse.
- **`Xvfb -displayfd` picks the lowest free display, which can be `:0`.** On a
  desktop that is the real display's number. The script probes :90–:119.
- **core22 has no `/etc/fonts` and only `Plymouth.ttf`.** The no-fonts run
  masks `/usr/share/fonts`, which leaves fontconfig with nothing, the exact
  condition of #279.

## Future Improvements

- A Wayland run (weston headless) would cover the other backend; #55 and #272
  were both X11-only, which is why X11 comes first.
- Install the snap's desktop entry the way snapd would and validate the
  rewritten `Icon=` path, beyond `check_contents`' presence check.
