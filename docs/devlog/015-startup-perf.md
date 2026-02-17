# Feature: Startup Performance Optimization

**Status:** ✅ Complete
**Branch:** `feature/startup-perf`
**Date:** 2026-02-17
**Lines Changed:** +26 / -28 in `src/main.rs`, `crates/egui_commonmark/egui_commonmark_backend/src/misc.rs`

## Summary

Reduced startup time by eliminating wasteful I/O and redundant deserialization that occurred on every app launch and file load.

## Changes

- [x] Remove NotoColorEmoji font loading (~11MB I/O saved per startup)
- [x] Use `LazyLock<Regex>` for header/link parsing (compile once, not per file load)
- [x] Share `SyntaxSet` across `CommonMarkCache` instances via `Arc` (deserialize once, not per tab)

## Key Discoveries

### ThemeSet doesn't implement Clone

`syntect::ThemeSet` doesn't derive `Clone`, so it can't be shared via `Arc::make_mut`. However, `ThemeSet` is just a `BTreeMap<String, Theme>` — very cheap to create. Only `SyntaxSet` (which deserializes ~300 language grammars) is worth sharing.

### LazyLock triggers MSRV clippy warning

`std::sync::LazyLock` is stable since Rust 1.80, but the project's `rust-version` is set to 1.76. This is already inaccurate (the pre-existing `floor_char_boundary` call requires 1.91). The warning is informational only.

## Architecture

### Global statics (src/main.rs)

```rust
static HEADER_RE: LazyLock<Regex> = LazyLock::new(|| ...);
static LINK_RE: LazyLock<Regex> = LazyLock::new(|| ...);
```

### Global static (egui_commonmark_backend/src/misc.rs)

```rust
static GLOBAL_SYNTAX_SET: LazyLock<Arc<SyntaxSet>> = LazyLock::new(|| ...);
```

`CommonMarkCache.ps` changed from `SyntaxSet` to `Arc<SyntaxSet>`. `ts` remains owned.

## Future Improvements

- [ ] Bump `rust-version` in Cargo.toml to 1.80+ to match actual usage
- [ ] Consider lazy font loading (load fonts in background thread)
