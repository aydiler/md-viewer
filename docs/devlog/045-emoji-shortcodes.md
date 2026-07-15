# Feature 045: GitHub emoji shortcode rendering

**Status:** Complete
**Branch:** `fix/38-emoji-shortcodes`
**Date:** 2026-07-12
**Issue:** #38

## Summary

Recognized GitHub/gemoji shortcodes in visible Markdown text now render as Unicode emoji. The renderer expands `Event::Text` only, after pulldown-cmark has parsed the document, so Markdown syntax and source offsets remain authoritative.

## Scope

- Expands recognized ordinary text and link-label shortcodes such as `:pushpin:`.
- Leaves inline code, fenced code, image alt text, destinations, URLs, unknown names, and malformed candidates literal.
- Preserves raw source byte ranges for search highlighting and raw heading identity for outline navigation.
- Adds the publishable `emojis` 0.9.0 dependency and bumps the vendored fork workspace from 0.24.0 to 0.25.0.

## Architecture

The UTF-8-safe scanner visits borrowed segments containing rendered text, raw spelling, absolute original source range, and replacement identity. Unchanged slices borrow parser input; recognized replacements borrow static `Emoji::as_str()` values. Its cursor advances through raw event bytes. A `:pushpin:` replacement therefore renders `📌` while retaining its original nine-byte source range.

Plain segments continue through borrowed, fine-grained search splitting without intermediate vectors or strings. Replacement segments are indivisible: any source overlap highlights the whole glyph, Active wins over Match, and active-match Y is captured during direct emission then written after immutable cache borrows end. Heading rendering receives the glyph, while heading identity receives raw shortcode spelling.

This keeps the implementation in the renderer's text-event boundary rather than preprocessing the whole document, which would alter Markdown parsing, code content, links, and byte offsets.

## Dependency And Publishing Findings

- `emojis` 0.9.0 provides `get_by_shortcode(&str)` and `Emoji::as_str()`.
- Registry metadata declares Rust 1.66 and exact license expression `(MIT OR Apache-2.0) AND Unicode-3.0`; renderer workspace requires Rust 1.76, so MSRV is compatible.
- Root and vendored lockfiles contain `emojis` 0.9.0 and renderer packages at 0.25.0.
- `emojis` bundles gemoji lookup data under Unicode-3.0. Its `LICENSE-UNICODE` permission condition requires copyright and permission notice with copies or associated documentation. `THIRD_PARTY_NOTICES` therefore carries full notice; release archives and package manifests install it beside project license material.
- Vendored renderer crate package includes `THIRD_PARTY_NOTICES`, so crates.io source distribution retains notice for dependency-derived lookup behavior.

## Testing

TDD RED was captured before implementation: renderer tests initially failed to compile because scanner, eligibility, and overlap helpers did not exist. Review-fix RED then drove production `Event::Code` and failed with `left: "📌", right: ":pushpin:"`; GREEN routes inline code through literal-only range highlighting. Allocation-review RED failed because borrowed visitor APIs did not exist; GREEN adds pointer-identity checks for no-colon and unknown-only unchanged paths plus a `hello world :rocket:` query-boundary regression. Production-boundary tests also verify heading display accumulates emoji while raw identity retains shortcode spelling, and duplicate heading keys remain raw/source-authoritative. Broader GREEN coverage includes scanner/range behavior, UTF-8 boundaries, adjacent and unknown candidates, search precedence, ordinary/link/fenced/indented-code/image contexts, raw app search ranges, and literal Unicode search.

Validation run from repository root or explicit manifests:

- `cargo test --manifest-path crates/egui_commonmark/Cargo.toml -p egui_commonmark_extended --lib --locked`
- `cargo test --manifest-path Cargo.toml --locked` — 34 passed
- `cargo check --manifest-path Cargo.toml --locked`
- `cargo clippy --manifest-path Cargo.toml --locked --all-targets`
- `cargo clippy --manifest-path crates/egui_commonmark/Cargo.toml -p egui_commonmark_extended --lib --tests --locked`
- locked root and vendored `cargo metadata`
- `scripts/check-fork-publishable.sh`
- root and renderer `cargo package --no-verify --list` notice checks
- AUR `.SRCINFO`, shell syntax, release/Snap/Flatpak YAML parsing, `rustfmt`, and `git diff --check`

Known baseline warning: vendored `cargo check --workspace --all-targets` still fails because examples import upstream crate name `egui_commonmark` rather than renamed package `egui_commonmark_extended`; relevant library/tests and root targets pass. Full renderer package verification also cannot resolve fresh unpublished `egui_commonmark_backend_extended` 0.25.0 from crates.io; publishability check confirms all 0.25.0 fork crates are fresh and ready for ordered publication.

## Coding Style And Technique Rationale

A small explicit scanner was chosen over regex replacement because raw-byte cursor movement and absolute ranges are part of the contract. Segment metadata keeps rendered and source identities separate at one boundary. Existing renderer paths remain intact for plain text, minimizing regression surface and avoiding unrelated parser or app refactors.
