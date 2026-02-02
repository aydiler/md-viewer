# Build Commands

```bash
cargo build              # Debug build
cargo build --release    # Release build (optimized for size)
cargo run                # Run debug build
cargo run -- file.md     # Open a specific file
cargo run -- file.md     # Live reload is enabled by default
cargo run -- file.md --no-watch  # Disable live reload
cargo clippy             # Lint check
make install             # Build release and install to ~/.local/bin
```

The release profile is configured for minimal binary size (`opt-level = "z"`, LTO, strip symbols).
