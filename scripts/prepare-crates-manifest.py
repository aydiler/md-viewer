#!/usr/bin/env python3
"""Rewrite the root Cargo.toml into the manifest that crates.io receives.

Used by both `publish-crates` (the real publish) and `crates-package-check`
(the release dry run) in .github/workflows/release.yml, so the package the dry
run verifies is the package the tag publishes.

1. Strip the local-only MCP dependency. Anchored to start-of-line so commented
   `# mcp = ...` / `# egui-mcp-bridge = ...` lines stay untouched and the
   working tree stays clean (regression caught in the v0.1.9 publish).
2. Drop `color-emoji` from the default features. It needs
   `FontData::emoji_only`, which only the vendored epaint has; `cargo publish`
   ignores `[patch.crates-io]`, so the crates.io package builds against stock
   epaint (#215). Fails unless exactly one such line exists, so a renamed
   feature cannot slip through to a publish that no longer compiles.
"""
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "Cargo.toml")
text = path.read_text()

text = re.sub(r'(?m)^mcp\s*=\s*\["dep:egui-mcp-bridge"\]', 'mcp = []', text)
text = re.sub(r'(?m)^egui-mcp-bridge\s*=\s*.*\n', '', text)

text, n = re.subn(r'(?m)^default\s*=\s*\["color-emoji"\]', 'default = []', text)
if n != 1:
    sys.exit(f'expected one `default = ["color-emoji"]` line in {path}, found {n}')

path.write_text(text)
print(f"{path}: MCP dependency stripped, color-emoji dropped from default features")
