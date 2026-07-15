# Fix: snapcraft `push-metadata` → `upload-metadata`

**Status:** ✅ Complete
**Branch:** `fix/snap-upload-metadata`
**Date:** 2026-07-15
**Files Changed:** `.github/workflows/release.yml` (1-line command + comment), `docs/LESSONS.md`

## Summary

The v0.1.14 release run's **Publish to Snap Store** job failed — but only on its
last step, "Push snap store listing metadata". The actual snap upload succeeded:

```
Status: ready to release!
Revision 17 created for 'md-viewer' and released to 'stable'
```

`snapcore/action-publish@v1` (the step that ships the `.snap` to the `stable`
channel) ran fine. The follow-on step then ran:

```
snapcraft push-metadata "${SNAP_FILE}" --force
# Error: no such command 'push-metadata', maybe you meant 'upload-metadata'
```

## Root cause

Snapcraft renamed its entire `push` verb family to `upload`
(`push` → `upload`, `push-metadata` → `upload-metadata`, …). The GitHub runner's
`snapcraft` (installed fresh via `sudo snap install snapcraft --classic` in this
step) auto-tracked a newer release than the one present at v0.1.13 (which
published metadata successfully ~5 weeks earlier). So this was **environmental
drift**, not a change introduced by the v0.1.14 release commit — and it would now
fail on every future release.

## Fix

One-word verb change: `push-metadata` → `upload-metadata`. Same positional
`<snap-file>` argument and same `--force` flag. Comment above the step updated to
record the rename and the fact that the snap upload is independent of this step.

## Impact / non-impact

- **No user impact for v0.1.14** — the snap is live at revision 17 on `stable`.
  Only the store *listing* metadata (summary/description from
  `snap/snapcraft.yaml`) wasn't re-synced, and it was unchanged from prior
  releases anyway.
- The overall release run shows red because of this single trailing step, even
  though crates.io, GitHub Release, AUR (`-git` + `-bin`), and the snap upload
  all succeeded.

## Future improvement

`upload-metadata` could itself be renamed/removed in a later snapcraft major.
Consider pinning the snapcraft channel used by this step (e.g.
`snap install snapcraft --classic --channel=8.x/stable`) so the verb surface
stays stable across releases instead of tracking latest.
