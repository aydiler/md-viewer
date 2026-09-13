# 066 — Copy Link Address (issue #169)

**Branch:** `feature/169-copy-link-address`
**Date:** 2026-09-07
**Status:** complete; held until v0.2.0 is tagged

## The request

@lamikr reported that a link's URL cannot be obtained without following it.
Clicking hands the URL to `open_url`, which on their GNOME desktop opens a
browser instance that does not carry their login session — so the link is
useless for a site they are authenticated to.

## What was there

`Link::end` in `egui_commonmark_backend/src/misc.rs` painted the link as an
`egui::Label` with `.selectable(false)` and `Sense::click()`. Hover showed the
URL, click called `open_url`. No context menu, and because the label is not
selectable, Ctrl+C had nothing to act on either.

The app already had the pattern: file-explorer entries carry a context menu
with *Copy Contents*, *Copy Path* and *Copy File URI*, built on
`ui.ctx().copy_text(...)`.

## What this adds

A `Copy Link Address` context-menu item on the link itself. `response` and
`destination` are both already in scope, so it is a handful of lines.

**Both link kinds copy `destination` exactly as the document spells it.** For an
external link that is the URL. For a link this viewer resolves itself, the
source spelling is the honest answer — the resolved path depends on which
document is open, so copying it would hand out something the author never wrote
and that means nothing when pasted elsewhere. I raised this as an open question
on the issue and it was not answered; the choice is recorded in a comment at
the call site so a future reader can disagree with the reasoning rather than
guess at it.

## Not included: Ctrl+C

That needs a concept the app does not have — "the link under the pointer" as
tracked state — plus a document-level key handler that does not steal Ctrl+C
from text selection. Separate change, deliberately not bundled.

## Verification

End-to-end on Xvfb, using @lamikr's own fixture (a reference-style link whose
definition puts the URL on a continuation line):

```
--- clipboard BEFORE ---
NOTHING (no clipboard owner)
--- clipboard AFTER right-click -> Copy Link Address ---
https://www.mywebshowxyzabcdefgpage.com/quote
Panics: 0
```

Read with a small `python-xlib` reader, since neither `xclip` nor `xsel` is
installed here. The before/after pair is the point: "the menu appeared" would
not have shown that anything was actually copied.

**No automated test.** The crate's harness inspects painted shapes and drives
no input, so an opening context menu is out of its reach. That is a weaker
position than the frontmatter work in #171, and worth saying plainly rather
than implying coverage that does not exist.

## Release timing

This touches the vendored renderer, whose workspace version is 0.28.0 and not
yet published. If it merges before the `v0.2.0` tag it rides along in 0.28.0;
after, it needs a bump. Held on purpose until the tag lands.

## Files

- `crates/egui_commonmark/egui_commonmark_backend/src/misc.rs`
