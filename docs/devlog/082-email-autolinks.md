# Email addresses become links

**Status:** Complete
**Branch:** `feature/email-autolinks`
**Date:** 2026-11-13

## Summary

GitHub links a plain `name@example.com` in prose; devlog 076 left this out when
bare URLs became links. `email_autolink_ranges` (`parsers/pulldown.rs`) now
finds them and gives each a `mailto:` destination.

## Rules (GFM extended email autolink)

- **Local part:** `[A-Za-z0-9.+_-]`, not preceded by another such character or
  by `/` (so `https://x.org/u@y.com` is not an email).
- **Domain:** labels of `[A-Za-z0-9_-]` joined by `.`, at least one `.`, no
  empty label.
- **End:** a trailing `.` is dropped; an address ending in `-` or `_` is not
  linked.

The ranges join `registered_auto_link_ranges` and `bare_url_ranges` before
`dedupe_link_ranges`, so a URL that contains an `@` stays one URL link. Search
highlights, emoji expansion, click and right-click copy come from the existing
auto-link path. md-viewer already classifies `mailto:` as external
(`#141` table in `src/main.rs`), so a click goes to the system handler through
egui's `open_url`.

## Tests

- Unit: accepts (`+tag`, subdomains, `_`/`-` in the local part, UTF-8
  neighbours, trailing `.`), rejects (`@example.com`, `user@localhost`,
  `user@example.`, `ex..com`, trailing `-`/`_`, `a@b@c`, `@` after `/`), and a
  URL containing `@` staying one link.
- Render: `email_addresses_in_prose_render_as_links`, underlined text is
  exactly the address, not the inline-code one. Observed red with the scanner
  unwired (`left: []`).
- Xvfb screenshot: address linked without the trailing `.`, URL with `@`
  linked whole, inline code and `user@localhost` plain.

## Key Discovery

`cargo fmt` cannot be pointed at the renderer (`--manifest-path` on a virtual
workspace reports "Failed to find targets"), and running `rustfmt` on its
files directly reformats hundreds of lines: the renderer has never been
rustfmt-clean and CI only checks formatting for the app crate. Format new
renderer code by hand.
