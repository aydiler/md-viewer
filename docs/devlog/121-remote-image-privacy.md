# Feature: Remote image privacy controls (#289)

**Status:** ✅ Complete
**Branch:** `feature/remote-image-privacy`
**Date:** 2026-11-24
**Lines Changed:** `crates/egui_commonmark` (`CommonMarkOptions::load_remote_images`, `Image::end`, `CommonMarkCache`), `src/main.rs` (`RemoteImages`, View menu, Ask bar), tests

## Summary

#289 (zHd4): opening a Markdown document fetches every remote image in it,
which tells the image servers the reader's IP address and when the document
was opened. The issue proposed blocking by default; md-viewer keeps loading by
default (README badges are the common case) and makes the other choices one
click away:

| View → Remote Images | Remote images |
|---|---|
| Load automatically (default) | fetched as before |
| Ask per document | placeholders, plus a bar: "N remote images not loaded. [Load images] [Always load]" |
| Block | placeholders only |

Local and `data:` images always load.

## Features

- [x] Renderer: `CommonMarkOptions::load_remote_images` (default `true`, so
      other users of the crate see no change) and builder
      `CommonMarkViewer::load_remote_images`.
- [x] `Image::end` is the only place a remote fetch starts (an `egui::Image`
      asks the loaders for its URI; table measurement only reads cached
      sizes). With the option off and an `http://`/`https://` URI
      (`is_remote_image_uri`, case-insensitive), it draws a small button
      "Remote image: <alt text or host>" with the URL as tooltip, records its
      size for table rows, and records the URI in `CommonMarkCache`.
- [x] `CommonMarkCache::blocked_remote_image_count` and
      `take_remote_images_request` (a click on a placeholder).
- [x] App: `RemoteImages { Load, Ask, Block }`, persisted
      (`PersistedState::remote_images`, older state files load as `Load`);
      per tab `remote_images_allowed` (session only); the Ask bar above the
      document; a placeholder click loads that document's images in Ask mode
      and does nothing in Block mode.
- [x] Tests: `tests/remote_images.rs` (renderer) checks the *request*, not
      the paint: a recording bytes loader sees every URI any loader asks for.
      With the option off no `http(s)` URI reaches it, the local image still
      does, and the placeholders show alt text or host; with it on, the same
      recorder sees the request (control). App:
      `remote_image_policy_survives_a_persistence_roundtrip`,
      `remote_image_policy_decides_per_tab`.

## Verification on Xvfb

A local `python -m http.server` on 127.0.0.1 served a PNG and an SVG badge; a
document referenced both plus a local image. Requests in the server's log:

| mode | after opening (6 s) | after clicking a placeholder |
|---|---|---|
| Load automatically | 2 | – |
| Ask per document | 0 | 2 |
| Block | 0 | – |

In Ask mode the bar read "2 remote images not loaded. [Load images] [Always
load]" with the placeholders "Remote image: remote picture" and "Remote
image: badge"; Block showed the placeholders without the bar. The local image
rendered in every mode.

## Key Discoveries

### The recording loader has to be the newest one

egui asks bytes loaders newest first and stops at the first one that does not
answer `NotSupported`. The renderer installs egui_extras' loaders on its first
frame, so a recorder added before that frame sat *behind* the HTTP and file
loaders and never saw `https://` or `file://` URIs; the first run of the test
failed its own control. Installing egui_extras' loaders first and the
recorder after makes it see everything. It answers `http(s)` URIs with an
error, so the test never reaches the network.

The mutation run confirms the test can fail: with the gate in `Image::end`
disabled, the blocked test reports both remote URIs requested.

### An `HTTP://` URI is not fetched today, but is still treated as remote

egui_extras' HTTP loader only matches lowercase schemes, so an uppercase one
fails to load instead of being fetched. `is_remote_image_uri` matches case
insensitively anyway, so a future loader change cannot turn it into a leak.

## Future Improvements

- HTML `<img>` tags are not rendered as images at all (inline HTML is shown
  as text, HTML tables strip tags), so they cannot fetch; if HTML images are
  ever rendered, they must go through the same option.
- A per-host allow list ("always load from img.shields.io") would sit between
  Ask and Load.
