# Snap job survives Snap Store flakiness

**Status:** In progress
**Branch:** `ci/snap-store-retry`
**Date:** 2026-11-12

## Summary

v0.3.2 published to crates.io, both AUR packages and the GitHub Release. The
snap did not. Its job failed six times in a row (three in the dry run, three on
the tag), always inside `snapcore/action-build@v1` at `sudo snap install lxd`,
before any of our code was built:

- `error: unable to contact snap store`
- `got unexpected HTTP status code 408 via POST to "https://api.snapcraft.io/v2/snaps/refresh"`
- `Post "https://api.snapcraft.io/v2/snaps/refresh": EOF`

The store was flaky, not just the runners: probing each of the six
`api.snapcraft.io` servers four times from this machine dropped two of 24
requests (`.55` and `.57`, no answer within 15 s), and one plain info request
returned an empty body. A fresh runner makes several store calls in a row to
install LXD, then snapcraft, then the base snaps, and the actions retry none of
them, so one dropped request ends the job.

## Change

- **`scripts/snap-ci.sh`** with `setup`, `build` and `publish`. It mirrors
  `action-build` (`src/tools.ts`, `src/build.ts`) step for step and wraps every
  store call in `retry N` with linear backoff (15 s, 30 s, …):
  - `setup`: root ownership fix, remove deb LXD, `lxd` group,
    `snap install lxd` (6 tries), `lxd init --auto`, `iptables -P FORWARD
    ACCEPT` (Docker's DROP policy cuts LXD containers off),
    `snap install snapcraft --classic` (6 tries).
  - `build`: `snapcraft` in LXD as the runner user with the same
    `SNAPCRAFT_BUILD_ENVIRONMENT`, `SNAPCRAFT_BUILD_INFO` and
    `SNAPCRAFT_IMAGE_INFO` the action set (3 tries); writes `snap=` to
    `$GITHUB_OUTPUT`.
  - `publish`: `snapcraft upload --release=stable` and
    `snapcraft upload-metadata --force` (4 tries each). Replaces
    `action-publish` plus the separate metadata step.
- **`release.yml` `publish-snap`** calls the script; timeout 30 → 60 min to fit
  the backoff.
- **`snap-publish.yml`**, new, `workflow_dispatch` with `tag` and `upload`.
  Runs from main, checks out the tag's sources, keeps main's script. Needed
  because `gh run rerun` replays a tag's workflow as it was at the tag, so a fix
  on main never reaches a re-run of v0.3.2's failed job.

## Key Discoveries

- `action-build` only *refreshes* LXD if it is already installed, and swallows a
  failed refresh. It does not swallow a failed snapcraft refresh.
- `snap install X` for an installed snap exits 0 without contacting the store,
  so retrying after a partial success is safe.
- A re-run of a tag's workflow uses the tag's workflow file (already in
  LESSONS for v0.1.8). Recovery needs a workflow that runs from main.

## Verification

- Release dry run on this branch: the new `setup`/`build` steps on a real
  runner, no upload. Result recorded below.
- After merge: `snap-publish.yml -f tag=v0.3.2 -f upload=true`, then the Snap
  Store info API shows 0.3.2 on stable.

## Future Improvements

- If upstream adds retries to `snapcore/action-build`, switching back would
  drop this script.
